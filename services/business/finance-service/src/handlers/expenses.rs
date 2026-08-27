//! `/api/v1/finance/expenses` — submit, approve/reject, list.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{conflict, internal, normalize_paging, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::journal::{ensure_ledger_accounts, expense_entry, post_journal};
use crate::principal::{
    enforce_any_scope, enforce_scoped, load_membership_scope, required_scope_for_owner_row,
};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    DecideExpenseRequest, ExpenseDto, ExpenseListResponse, ListQuery, SubmitExpenseRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/expenses",
            get(list_expenses).post(submit_expense),
        )
        .route("/api/v1/finance/expenses/{id}/decide", post(decide_expense))
}

#[derive(Debug, sqlx::FromRow)]
struct ExpenseRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    owner_user_id: Uuid,
    status: String,
    currency: String,
    amount_minor: i64,
    description: String,
    category_code: Option<String>,
    receipt_url: Option<String>,
    incurred_at: NaiveDate,
    created_at: DateTime<Utc>,
    approval_id: Option<String>,
}

impl ExpenseRow {
    fn into_dto(self) -> ExpenseDto {
        ExpenseDto {
            id: self.public_id,
            status: self.status,
            currency: self.currency,
            amount_minor: self.amount_minor,
            description: self.description,
            category_code: self.category_code,
            receipt_url: self.receipt_url,
            incurred_at: self.incurred_at.to_string(),
            created_at: self.created_at.to_rfc3339(),
            approval_id: self.approval_id,
        }
    }
}

const EXPENSE_SELECT: &str = r#"
    e.id, e.public_id, e.owner_user_id, e.status, e.currency, e.amount_minor,
    e.description, cat.code AS category_code, e.receipt_url, e.incurred_at, e.created_at,
    e.approval_id
"#;

async fn fetch_expense(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    expense_id: Uuid,
) -> Result<Option<ExpenseRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {EXPENSE_SELECT}
         FROM finance_expense e
         LEFT JOIN finance_expense_category cat ON cat.id = e.category_id
         WHERE e.org_id = $1 AND e.id = $2"
    ))
    .bind(org_id)
    .bind(expense_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn post_expense_journal(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    expense_id: Uuid,
    currency: Currency,
    amount_minor: i64,
    request_id: &str,
) -> Result<(), AppError> {
    let journal = expense_entry(expense_id, currency, amount_minor)
        .map_err(|e| validation(request_id, format!("journal: {e}")))?;
    post_journal(tx, org_id, &journal, request_id).await?;
    sqlx::query(
        r#"
        UPDATE finance_expense SET status = 'posted', updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(expense_id)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    Ok(())
}

/// GET /api/v1/finance/expenses
#[utoipa::path(get, path = "/api/v1/finance/expenses", tag = "finance-expenses",
    params(
        ("status" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = ExpenseListResponse)))]
pub async fn list_expenses(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<ExpenseListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::finance_expense_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM finance_expense WHERE org_id = ");
    count_qb.push_bind(org_id);
    push_owner_predicate(
        &mut count_qb,
        scope,
        org_id,
        actor,
        membership.team_id,
        membership.department_id,
    );
    if let Some(status) = q.status.as_deref() {
        count_qb.push(" AND status = ");
        count_qb.push_bind(status);
    }
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {EXPENSE_SELECT}
         FROM finance_expense e
         LEFT JOIN finance_expense_category cat ON cat.id = e.category_id
         WHERE e.org_id = "
    ));
    qb.push_bind(org_id);
    match scope {
        companyos_authz::Scope::Organization => {}
        companyos_authz::Scope::Own => {
            qb.push(" AND e.owner_user_id = ");
            qb.push_bind(actor);
        }
        companyos_authz::Scope::Team => {
            qb.push(" AND (e.owner_user_id = ");
            qb.push_bind(actor);
            if let Some(team) = membership.team_id {
                qb.push(" OR e.owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ");
                qb.push_bind(org_id);
                qb.push(" AND team_id = ");
                qb.push_bind(team);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
        companyos_authz::Scope::Department => {
            qb.push(" AND (e.owner_user_id = ");
            qb.push_bind(actor);
            if let Some(dept) = membership.department_id {
                qb.push(" OR e.owner_user_id IN (SELECT user_id FROM membership WHERE org_id = ");
                qb.push_bind(org_id);
                qb.push(" AND department_id = ");
                qb.push_bind(dept);
                qb.push(" AND revoked_at IS NULL)");
            }
            qb.push(")");
        }
    }
    if let Some(status) = q.status.as_deref() {
        qb.push(" AND e.status = ");
        qb.push_bind(status);
    }
    qb.push(" ORDER BY e.created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<ExpenseRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ExpenseListResponse {
        items: rows.into_iter().map(ExpenseRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/finance/expenses
#[utoipa::path(post, path = "/api/v1/finance/expenses", tag = "finance-expenses",
    request_body = SubmitExpenseRequest,
    responses((status = 201, body = ExpenseDto)))]
pub async fn submit_expense(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<SubmitExpenseRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_create(),
        &request_id,
    )?;

    if body.amount_minor <= 0 {
        return Err(validation(&request_id, "amount_minor must be positive"));
    }
    let currency = Currency::new(&body.currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let incurred_at = body
        .incurred_at
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(&request_id, "incurred_at must be YYYY-MM-DD"))?
        .unwrap_or_else(|| Utc::now().date_naive());

    let public_id = PublicId::generate(IdKind::Expense);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "expense.submit", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    // Phase 2.4: active expense policy category limits.
    let mut policy_force_approval = false;
    let cat_code_for_policy = body.category_code.as_deref().unwrap_or("general");
    let policy_limit: Option<(String, i64)> = sqlx::query_as(
        r#"
        SELECT p.over_limit_action, l.max_amount_minor
        FROM finance_expense_policy p
        JOIN finance_expense_category_limit l ON l.policy_id = p.id
        JOIN finance_expense_category c ON c.id = l.category_id
        WHERE p.org_id = $1 AND p.is_active = true AND c.code = $2
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(cat_code_for_policy)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some((action, max_minor)) = policy_limit {
        if body.amount_minor > max_minor {
            if action == "reject" {
                return Err(validation(
                    &request_id,
                    format!(
                        "amount_minor {} exceeds category limit {} for {cat_code_for_policy}",
                        body.amount_minor, max_minor
                    ),
                ));
            }
            // require_approval (default) → force pending_approval path.
            policy_force_approval = true;
        }
    }

    let limit_row: Option<(Option<i64>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT r.approval_limit_amount_minor, r.approval_limit_currency
        FROM membership m
        LEFT JOIN org_role r ON r.id = m.role_id
        WHERE m.org_id = $1 AND m.user_id = $2 AND m.revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(auth.ctx.actor.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let role_needs_approval = match limit_row {
        Some((Some(limit), _)) if body.amount_minor > limit => true,
        Some((None, _)) | None => false, // NULL limit → unlimited (auto-approve)
        Some((Some(_), _)) => false,
    };
    let needs_approval = role_needs_approval || policy_force_approval;

    let initial_status = if needs_approval {
        "pending_approval"
    } else {
        "submitted"
    };

    let category_id: Option<Uuid> = if let Some(code) = body.category_code.as_deref() {
        sqlx::query_scalar(
            "SELECT id FROM finance_expense_category WHERE org_id = $1 AND code = $2",
        )
        .bind(org_id)
        .bind(code)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    } else {
        sqlx::query_scalar(
            "SELECT id FROM finance_expense_category WHERE org_id = $1 AND code = 'general'",
        )
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?
    };

    sqlx::query(
        r#"
        INSERT INTO finance_expense (
            id, org_id, public_id, owner_user_id, category_id, status,
            currency, amount_minor, description, receipt_url, incurred_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(auth.ctx.actor.user_id)
    .bind(category_id)
    .bind(initial_status)
    .bind(&body.currency)
    .bind(body.amount_minor)
    .bind(&body.description)
    .bind(&body.receipt_url)
    .bind(incurred_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let submitted_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "expense",
        "submitted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "amount_minor": body.amount_minor,
            "status": initial_status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &submitted_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if !needs_approval {
        post_expense_journal(
            &mut tx,
            org_id,
            id,
            currency,
            body.amount_minor,
            &request_id,
        )
        .await?;
        let approved_env = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "expense",
            "approved",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": public_id.as_str(),
                "amount_minor": body.amount_minor,
                "auto": true,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &approved_env)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.expense.submit",
        "expense",
        &public_id.as_str(),
        serde_json::json!({ "status": initial_status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_expense(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "expense missing after insert",
            )
        })?
        .into_dto();

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "expense.submit",
            key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    // Phase 1.7: when above approval_limit, request via Operations approval engine.
    // Falls back to the 1.5 pending_approval + finance decide path if the engine
    // is unreachable (tests / local without project-service).
    let mut dto = dto;
    if needs_approval {
        if let Some(apr_id) = request_expense_approval(
            &auth,
            &public_id.as_str(),
            body.amount_minor,
            &body.currency,
            body.category_code.as_deref(),
            &body.description,
        )
        .await
        {
            if let Ok(mut link_tx) = state.pool.begin().await {
                let _ = set_session_org_id(&mut link_tx, auth.ctx.org_id).await;
                let _ = sqlx::query(
                    "UPDATE finance_expense SET approval_id = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
                )
                .bind(org_id)
                .bind(id)
                .bind(&apr_id)
                .execute(&mut *link_tx)
                .await;
                let _ = link_tx.commit().await;
                dto.approval_id = Some(apr_id);
            }
        }
    }

    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// Call Operations approval API (no cross-context table reads).
async fn request_expense_approval(
    auth: &AuthCtx,
    expense_public_id: &str,
    amount_minor: i64,
    currency: &str,
    category: Option<&str>,
    description: &str,
) -> Option<String> {
    let project_url =
        std::env::var("PROJECT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into());
    let url = format!(
        "{}/api/v1/operations/approvals",
        project_url.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "subject_type": "expense",
        "subject_id": expense_public_id,
        "title": format!("Expense: {description}"),
        "summary": description,
        "amount_minor": amount_minor,
        "currency": currency,
        "category": category,
    }));
    req = req
        .header(
            "x-companyos-dev-org-id",
            auth.ctx.org_id.to_public().as_str(),
        )
        .header(
            "x-companyos-dev-user-id",
            PublicId::new(IdKind::User, auth.ctx.actor.user_id).as_str(),
        );
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// POST /api/v1/finance/expenses/{id}/decide
#[utoipa::path(post, path = "/api/v1/finance/expenses/{id}/decide", tag = "finance-expenses",
    request_body = DecideExpenseRequest,
    responses((status = 200, body = ExpenseDto)))]
pub async fn decide_expense(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DecideExpenseRequest>,
) -> Result<Json<ExpenseDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let expense_id = parse_public_id(IdKind::Expense, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    // Approvers: finance.expense.approve (Admin/Finance/Owner via role defaults).
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_approve(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let row = fetch_expense(&mut tx, org_id, expense_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "expense"))?;

    let required = required_scope_for_owner_row(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        Some(row.owner_user_id),
    )
    .await
    .map_err(internal(&request_id))?;
    enforce_scoped(
        &membership.principal,
        perms::finance_expense_approve(),
        required,
        &request_id,
    )?;

    if row.status != "pending_approval" {
        return Err(conflict(
            &request_id,
            format!("expense status {} is not pending_approval", row.status),
        ));
    }

    if body.approve {
        sqlx::query(
            r#"
            UPDATE finance_expense SET
                status = 'approved',
                decided_by = $3,
                decided_at = now(),
                decision_note = $4,
                updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(expense_id)
        .bind(auth.ctx.actor.user_id)
        .bind(&body.note)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let currency = Currency::new(&row.currency)
            .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
        post_expense_journal(
            &mut tx,
            org_id,
            expense_id,
            currency,
            row.amount_minor,
            &request_id,
        )
        .await?;

        let env = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "expense",
            "approved",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({
                "id": row.public_id,
                "amount_minor": row.amount_minor,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &env)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    } else {
        sqlx::query(
            r#"
            UPDATE finance_expense SET
                status = 'rejected',
                decided_by = $3,
                decided_at = now(),
                decision_note = $4,
                updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(expense_id)
        .bind(auth.ctx.actor.user_id)
        .bind(&body.note)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        let env = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Finance,
            "expense",
            "rejected",
            1,
            auth.ctx.actor.clone(),
            serde_json::json!({ "id": row.public_id }),
        );
        companyos_outbox::insert_event(&mut *tx, &env)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        if body.approve {
            "finance.expense.approve"
        } else {
            "finance.expense.reject"
        },
        "expense",
        &row.public_id,
        serde_json::json!({ "note": body.note }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_expense(&mut tx, org_id, expense_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "expense"))?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
