//! Expense policy, mileage/per-diem, card import/match, reimbursements.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::journal::{ensure_ledger_accounts, expense_entry, post_journal};
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CardTransactionDto, CategoryLimitDto, CreateReimbursementBatchRequest,
    DecideReimbursementRequest, ExpenseDto, ExpensePolicyDto, ImportCardCsvRequest,
    ImportCardResponse, MatchCardsResponse, MileageCalculateRequest, MileageCalculateResponse,
    PerDiemRequest, PerDiemResponse, ReimbursementBatchDto, UpsertExpensePolicyRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/expense-policies",
            get(get_expense_policy).put(upsert_expense_policy),
        )
        .route(
            "/api/v1/finance/expenses/mileage",
            post(create_mileage_expense),
        )
        .route(
            "/api/v1/finance/expenses/per-diem",
            post(create_per_diem_expense),
        )
        .route(
            "/api/v1/finance/card-transactions/import",
            post(import_card_csv),
        )
        .route(
            "/api/v1/finance/card-transactions/auto-match",
            post(auto_match_cards),
        )
        .route(
            "/api/v1/finance/reimbursements",
            post(create_reimbursement_batch),
        )
        .route(
            "/api/v1/finance/reimbursements/{id}/decide",
            post(decide_reimbursement),
        )
}

#[derive(Debug, sqlx::FromRow)]
struct PolicyRow {
    id: Uuid,
    public_id: String,
    name: String,
    is_active: bool,
    require_receipt_over_minor: i64,
    auto_approve_under_minor: i64,
    over_limit_action: String,
    mileage_unit: String,
    mileage_rate_minor: i64,
    per_diem_minor: i64,
}

async fn load_active_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<Option<PolicyRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, public_id, name, is_active, require_receipt_over_minor,
               auto_approve_under_minor, over_limit_action, mileage_unit,
               mileage_rate_minor, per_diem_minor
        FROM finance_expense_policy
        WHERE org_id = $1 AND is_active = true
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_category_limits(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    policy_id: Uuid,
) -> Result<Vec<CategoryLimitDto>, sqlx::Error> {
    let rows: Vec<(String, i64, String)> = sqlx::query_as(
        r#"
        SELECT c.code, l.max_amount_minor, l.currency
        FROM finance_expense_category_limit l
        JOIN finance_expense_category c ON c.id = l.category_id
        WHERE l.org_id = $1 AND l.policy_id = $2
        ORDER BY c.code
        "#,
    )
    .bind(org_id)
    .bind(policy_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(category_code, max_amount_minor, currency)| CategoryLimitDto {
                category_code,
                max_amount_minor,
                currency,
            },
        )
        .collect())
}

fn policy_to_dto(row: PolicyRow, limits: Vec<CategoryLimitDto>) -> ExpensePolicyDto {
    ExpensePolicyDto {
        id: row.public_id,
        name: row.name,
        is_active: row.is_active,
        require_receipt_over_minor: row.require_receipt_over_minor,
        auto_approve_under_minor: row.auto_approve_under_minor,
        over_limit_action: row.over_limit_action,
        mileage_unit: row.mileage_unit,
        mileage_rate_minor: row.mileage_rate_minor,
        per_diem_minor: row.per_diem_minor,
        category_limits: limits,
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ExpenseRow {
    public_id: String,
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

async fn fetch_expense_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    expense_id: Uuid,
) -> Result<Option<ExpenseDto>, sqlx::Error> {
    let row: Option<ExpenseRow> = sqlx::query_as(
        r#"
        SELECT e.public_id, e.status, e.currency, e.amount_minor, e.description,
               cat.code AS category_code, e.receipt_url, e.incurred_at, e.created_at,
               e.approval_id
        FROM finance_expense e
        LEFT JOIN finance_expense_category cat ON cat.id = e.category_id
        WHERE e.org_id = $1 AND e.id = $2
        "#,
    )
    .bind(org_id)
    .bind(expense_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(ExpenseRow::into_dto))
}

async fn category_id_by_code(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM finance_expense_category WHERE org_id = $1 AND code = $2")
        .bind(org_id)
        .bind(code)
        .fetch_optional(&mut **tx)
        .await
}

fn parse_csv_line(line: &str) -> Vec<String> {
    line.split(',')
        .map(|c| c.trim().trim_matches('"').to_string())
        .collect()
}

fn parse_amount_to_minor(raw: &str, request_id: &str) -> Result<i64, AppError> {
    let s = raw.trim().replace(',', "");
    if s.is_empty() {
        return Err(validation(request_id, "amount required"));
    }
    if let Some(dot) = s.find('.') {
        let whole = &s[..dot];
        let frac = &s[dot + 1..];
        if frac.len() > 2 {
            return Err(validation(
                request_id,
                "amount supports at most 2 decimal places",
            ));
        }
        let whole_i: i64 = whole
            .parse()
            .map_err(|_| validation(request_id, format!("invalid amount: {raw}")))?;
        let frac_padded = format!("{:0<2}", frac);
        let frac_i: i64 = frac_padded
            .parse()
            .map_err(|_| validation(request_id, format!("invalid amount: {raw}")))?;
        let sign = if whole_i < 0 || s.starts_with('-') {
            -1
        } else {
            1
        };
        Ok(sign * (whole_i.abs() * 100 + frac_i))
    } else {
        s.parse()
            .map_err(|_| validation(request_id, format!("invalid amount: {raw}")))
    }
}

/// GET /api/v1/finance/expense-policies
#[utoipa::path(
    get,
    path = "/api/v1/finance/expense-policies",
    tag = "finance-policy",
    responses((status = 200, body = ExpensePolicyDto))
)]
pub async fn get_expense_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<ExpensePolicyDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_policy_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let policy = load_active_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "expense policy"))?;
    let limits = load_category_limits(&mut tx, org_id, policy.id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(policy_to_dto(policy, limits)))
}

/// PUT /api/v1/finance/expense-policies
#[utoipa::path(
    put,
    path = "/api/v1/finance/expense-policies",
    tag = "finance-policy",
    request_body = UpsertExpensePolicyRequest,
    responses((status = 200, body = ExpensePolicyDto), (status = 201, body = ExpensePolicyDto))
)]
pub async fn upsert_expense_policy(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<UpsertExpensePolicyRequest>,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_policy_manage(),
        &request_id,
    )?;

    if let Some(ref action) = body.over_limit_action {
        if action != "require_approval" && action != "reject" {
            return Err(validation(
                &request_id,
                "over_limit_action must be require_approval or reject",
            ));
        }
    }
    if let Some(ref unit) = body.mileage_unit {
        if unit != "mile" && unit != "km" {
            return Err(validation(&request_id, "mileage_unit must be mile or km"));
        }
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let existing = load_active_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let (policy_id, public_id_str, created) = if let Some(row) = existing {
        sqlx::query(
            r#"
            UPDATE finance_expense_policy SET
                name = COALESCE($3, name),
                require_receipt_over_minor = COALESCE($4, require_receipt_over_minor),
                auto_approve_under_minor = COALESCE($5, auto_approve_under_minor),
                over_limit_action = COALESCE($6, over_limit_action),
                mileage_unit = COALESCE($7, mileage_unit),
                mileage_rate_minor = COALESCE($8, mileage_rate_minor),
                per_diem_minor = COALESCE($9, per_diem_minor),
                updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(row.id)
        .bind(&body.name)
        .bind(body.require_receipt_over_minor)
        .bind(body.auto_approve_under_minor)
        .bind(&body.over_limit_action)
        .bind(&body.mileage_unit)
        .bind(body.mileage_rate_minor)
        .bind(body.per_diem_minor)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        (row.id, row.public_id, false)
    } else {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::ExpensePolicy, id);
        sqlx::query(
            r#"
            INSERT INTO finance_expense_policy (
                id, org_id, public_id, name, is_active,
                require_receipt_over_minor, auto_approve_under_minor, over_limit_action,
                mileage_unit, mileage_rate_minor, per_diem_minor
            ) VALUES ($1,$2,$3,$4,true,$5,$6,$7,$8,$9,$10)
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(public_id.as_str())
        .bind(body.name.as_deref().unwrap_or("Default"))
        .bind(body.require_receipt_over_minor.unwrap_or(0))
        .bind(body.auto_approve_under_minor.unwrap_or(0))
        .bind(
            body.over_limit_action
                .as_deref()
                .unwrap_or("require_approval"),
        )
        .bind(body.mileage_unit.as_deref().unwrap_or("mile"))
        .bind(body.mileage_rate_minor.unwrap_or(0))
        .bind(body.per_diem_minor.unwrap_or(0))
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        (id, public_id.as_str().to_string(), true)
    };

    if let Some(ref limits) = body.category_limits {
        sqlx::query(
            "DELETE FROM finance_expense_category_limit WHERE org_id = $1 AND policy_id = $2",
        )
        .bind(org_id)
        .bind(policy_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        for lim in limits {
            if lim.max_amount_minor <= 0 {
                return Err(validation(&request_id, "max_amount_minor must be positive"));
            }
            let cat_id = category_id_by_code(&mut tx, org_id, &lim.category_code)
                .await
                .map_err(internal(&request_id))?
                .ok_or_else(|| {
                    validation(
                        &request_id,
                        format!("unknown category_code: {}", lim.category_code),
                    )
                })?;
            sqlx::query(
                r#"
                INSERT INTO finance_expense_category_limit (
                    id, org_id, policy_id, category_id, max_amount_minor, currency
                ) VALUES ($1,$2,$3,$4,$5,$6)
                "#,
            )
            .bind(new_uuid_v7())
            .bind(org_id)
            .bind(policy_id)
            .bind(cat_id)
            .bind(lim.max_amount_minor)
            .bind(&lim.currency)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.expense_policy.upsert",
        "expense_policy",
        &public_id_str,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    let policy = load_active_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "expense policy"))?;
    let limits = load_category_limits(&mut tx, org_id, policy.id)
        .await
        .map_err(internal(&request_id))?;
    let dto = policy_to_dto(policy, limits);
    tx.commit().await.map_err(internal(&request_id))?;

    if created {
        Ok((StatusCode::CREATED, Json(dto)).into_response())
    } else {
        Ok((StatusCode::OK, Json(dto)).into_response())
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_computed_expense(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    actor: Uuid,
    currency: &str,
    amount_minor: i64,
    description: &str,
    category_code: &str,
    incurred_at: NaiveDate,
    expense_kind: &str,
    miles_or_km: Option<f64>,
    per_diem_days: Option<i32>,
    request_id: &str,
) -> Result<(Uuid, String, ExpenseDto), AppError> {
    let public_id = PublicId::generate(IdKind::Expense);
    let id = public_id.uuid();
    let category_id = category_id_by_code(tx, org_id, category_code)
        .await
        .map_err(internal(request_id))?;

    // Role approval limit gate.
    let limit_row: Option<(Option<i64>,)> = sqlx::query_as(
        r#"
        SELECT r.approval_limit_amount_minor
        FROM membership m
        LEFT JOIN org_role r ON r.id = m.role_id
        WHERE m.org_id = $1 AND m.user_id = $2 AND m.revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let needs_approval = matches!(limit_row, Some((Some(limit),)) if amount_minor > limit);
    let status = if needs_approval {
        "pending_approval"
    } else {
        "submitted"
    };

    sqlx::query(
        r#"
        INSERT INTO finance_expense (
            id, org_id, public_id, owner_user_id, category_id, status,
            currency, amount_minor, description, incurred_at,
            expense_kind, miles_or_km, per_diem_days
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(actor)
    .bind(category_id)
    .bind(status)
    .bind(currency)
    .bind(amount_minor)
    .bind(description)
    .bind(incurred_at)
    .bind(expense_kind)
    .bind(miles_or_km)
    .bind(per_diem_days)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    if !needs_approval {
        let cur = Currency::new(currency)
            .map_err(|e| validation(request_id, format!("invalid currency: {e}")))?;
        let journal = expense_entry(id, cur, amount_minor)
            .map_err(|e| validation(request_id, format!("journal: {e}")))?;
        post_journal(tx, org_id, &journal, request_id).await?;
        sqlx::query(
            "UPDATE finance_expense SET status = 'posted', updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }

    let dto = fetch_expense_dto(tx, org_id, id)
        .await
        .map_err(internal(request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id,
                "expense missing after insert",
            )
        })?;
    Ok((id, public_id.as_str().to_string(), dto))
}

/// POST /api/v1/finance/expenses/mileage
#[utoipa::path(
    post,
    path = "/api/v1/finance/expenses/mileage",
    tag = "finance-policy",
    request_body = MileageCalculateRequest,
    responses((status = 201, body = MileageCalculateResponse))
)]
pub async fn create_mileage_expense(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<MileageCalculateRequest>,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

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

    if body.miles_or_km <= 0.0 {
        return Err(validation(&request_id, "miles_or_km must be positive"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let policy = load_active_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| validation(&request_id, "no active expense policy"))?;
    if policy.mileage_rate_minor <= 0 {
        return Err(validation(
            &request_id,
            "mileage_rate_minor not configured on policy",
        ));
    }

    let amount_minor = ((body.miles_or_km * policy.mileage_rate_minor as f64).round()) as i64;
    if amount_minor <= 0 {
        return Err(validation(
            &request_id,
            "computed mileage amount must be positive",
        ));
    }
    let currency = body.currency.clone().unwrap_or_else(|| "USD".into());
    let _ = Currency::new(&currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let incurred_at = body
        .incurred_at
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(&request_id, "incurred_at must be YYYY-MM-DD"))?
        .unwrap_or_else(|| Utc::now().date_naive());
    let description = body
        .description
        .clone()
        .unwrap_or_else(|| format!("Mileage ({:.3} {})", body.miles_or_km, policy.mileage_unit));

    let (_id, _pid, expense) = insert_computed_expense(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        &currency,
        amount_minor,
        &description,
        "mileage",
        incurred_at,
        "mileage",
        Some(body.miles_or_km),
        None,
        &request_id,
    )
    .await?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.expense.mileage",
        "expense",
        &expense.id,
        serde_json::json!({ "miles_or_km": body.miles_or_km }),
    )
    .await
    .map_err(internal(&request_id))?;

    let resp = MileageCalculateResponse {
        amount_minor,
        currency,
        rate_minor: policy.mileage_rate_minor,
        miles_or_km: body.miles_or_km,
        expense,
    };
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(resp)))
}

/// POST /api/v1/finance/expenses/per-diem
#[utoipa::path(
    post,
    path = "/api/v1/finance/expenses/per-diem",
    tag = "finance-policy",
    request_body = PerDiemRequest,
    responses((status = 201, body = PerDiemResponse))
)]
pub async fn create_per_diem_expense(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<PerDiemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

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

    if body.days <= 0 {
        return Err(validation(&request_id, "days must be positive"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let policy = load_active_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| validation(&request_id, "no active expense policy"))?;
    if policy.per_diem_minor <= 0 {
        return Err(validation(
            &request_id,
            "per_diem_minor not configured on policy",
        ));
    }

    let amount_minor = policy.per_diem_minor * i64::from(body.days);
    let currency = body.currency.clone().unwrap_or_else(|| "USD".into());
    let _ = Currency::new(&currency)
        .map_err(|e| validation(&request_id, format!("invalid currency: {e}")))?;
    let incurred_at = body
        .incurred_at
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| validation(&request_id, "incurred_at must be YYYY-MM-DD"))?
        .unwrap_or_else(|| Utc::now().date_naive());
    let description = body
        .description
        .clone()
        .unwrap_or_else(|| format!("Per diem ({} days)", body.days));

    let (_id, _pid, expense) = insert_computed_expense(
        &mut tx,
        org_id,
        auth.ctx.actor.user_id,
        &currency,
        amount_minor,
        &description,
        "per_diem",
        incurred_at,
        "per_diem",
        None,
        Some(body.days),
        &request_id,
    )
    .await?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.expense.per_diem",
        "expense",
        &expense.id,
        serde_json::json!({ "days": body.days }),
    )
    .await
    .map_err(internal(&request_id))?;

    let resp = PerDiemResponse {
        amount_minor,
        currency,
        per_diem_minor: policy.per_diem_minor,
        days: body.days,
        expense,
    };
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(resp)))
}

/// POST /api/v1/finance/card-transactions/import
#[utoipa::path(
    post,
    path = "/api/v1/finance/card-transactions/import",
    tag = "finance-policy",
    request_body = ImportCardCsvRequest,
    responses((status = 201, body = ImportCardResponse))
)]
pub async fn import_card_csv(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<ImportCardCsvRequest>,
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
        perms::finance_expense_policy_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) = idempotency::get(&mut *tx, org_id, "card.import", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    // CSV: date,amount,currency,merchant,reference,description
    let mut items = Vec::new();
    let mut header_seen = false;
    for (idx, raw_line) in body.csv.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cols = parse_csv_line(trimmed);
        if !header_seen {
            header_seen = true;
            let lower: Vec<String> = cols.iter().map(|c| c.to_ascii_lowercase()).collect();
            if lower.iter().any(|c| c == "date" || c == "amount") {
                continue;
            }
        }
        if cols.len() < 2 {
            return Err(validation(
                &request_id,
                format!("csv line {}: need date,amount", idx + 1),
            ));
        }
        let txn_date = NaiveDate::parse_from_str(&cols[0], "%Y-%m-%d").map_err(|_| {
            validation(
                &request_id,
                format!("csv line {}: date must be YYYY-MM-DD", idx + 1),
            )
        })?;
        let amount_minor = parse_amount_to_minor(&cols[1], &request_id)?;
        let currency = cols
            .get(2)
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "USD".into());
        let merchant = cols.get(3).cloned().filter(|s| !s.is_empty());
        let reference = cols.get(4).cloned().filter(|s| !s.is_empty());
        let description = cols.get(5).cloned().filter(|s| !s.is_empty());

        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CardTransaction, id);
        sqlx::query(
            r#"
            INSERT INTO finance_card_transaction (
                id, org_id, public_id, txn_date, amount_minor, currency,
                merchant, reference, description, status, import_batch_key
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'unmatched',$10)
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(public_id.as_str())
        .bind(txn_date)
        .bind(amount_minor)
        .bind(&currency)
        .bind(&merchant)
        .bind(&reference)
        .bind(&description)
        .bind(&idem_key)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

        items.push(CardTransactionDto {
            id: public_id.as_str().to_string(),
            txn_date: txn_date.to_string(),
            amount_minor,
            currency,
            merchant,
            reference,
            description,
            status: "unmatched".into(),
            matched_expense_id: None,
        });
    }
    if items.is_empty() {
        return Err(validation(&request_id, "csv contained no data rows"));
    }

    let resp = ImportCardResponse {
        imported: items.len() as i32,
        items,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "card.import",
            key,
            201,
            serde_json::to_value(&resp).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

/// POST /api/v1/finance/card-transactions/auto-match
#[utoipa::path(
    post,
    path = "/api/v1/finance/card-transactions/auto-match",
    tag = "finance-policy",
    responses((status = 200, body = MatchCardsResponse))
)]
pub async fn auto_match_cards(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<MatchCardsResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_expense_policy_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    #[derive(sqlx::FromRow)]
    struct CardRow {
        id: Uuid,
        txn_date: NaiveDate,
        amount_minor: i64,
        currency: String,
    }
    let cards: Vec<CardRow> = sqlx::query_as(
        r#"
        SELECT id, txn_date, amount_minor, currency
        FROM finance_card_transaction
        WHERE org_id = $1 AND status = 'unmatched'
        ORDER BY txn_date
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    #[derive(sqlx::FromRow)]
    struct ExpCand {
        id: Uuid,
        incurred_at: NaiveDate,
        amount_minor: i64,
        currency: String,
    }
    let expenses: Vec<ExpCand> = sqlx::query_as(
        r#"
        SELECT id, incurred_at, amount_minor, currency
        FROM finance_expense
        WHERE org_id = $1
          AND status IN ('submitted', 'pending_approval', 'approved', 'posted')
          AND card_txn_id IS NULL
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut used = std::collections::HashSet::new();
    let mut matched = 0i32;
    for card in &cards {
        let mut best: Option<&ExpCand> = None;
        for exp in &expenses {
            if used.contains(&exp.id) {
                continue;
            }
            if exp.amount_minor != card.amount_minor.abs() {
                continue;
            }
            if exp.currency != card.currency {
                continue;
            }
            let days = (card.txn_date - exp.incurred_at).num_days().abs();
            if days > 3 {
                continue;
            }
            best = Some(exp);
            break;
        }
        let Some(exp) = best else {
            continue;
        };
        used.insert(exp.id);
        sqlx::query(
            r#"
            UPDATE finance_card_transaction SET
                status = 'matched', matched_expense_id = $3
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(card.id)
        .bind(exp.id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        sqlx::query(
            r#"
            UPDATE finance_expense SET
                card_txn_id = $3, expense_kind = 'card', updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(exp.id)
        .bind(card.id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        matched += 1;
    }

    let unmatched = cards.len() as i32 - matched;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(MatchCardsResponse { matched, unmatched }))
}

async fn request_reimb_approval(
    auth: &AuthCtx,
    batch_public_id: &str,
    amount_minor: i64,
    currency: &str,
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
        "subject_type": "reimbursement",
        "subject_id": batch_public_id,
        "title": format!("Reimbursement batch {batch_public_id}"),
        "summary": "Expense reimbursement batch",
        "amount_minor": amount_minor,
        "currency": currency,
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

async fn fetch_reimb_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    batch_id: Uuid,
) -> Result<Option<ReimbursementBatchDto>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let row: Option<(String, String, String, i64, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        r#"
            SELECT public_id, status, currency, total_minor, approval_id, created_at
            FROM finance_reimbursement_batch
            WHERE org_id = $1 AND id = $2
            "#,
    )
    .bind(org_id)
    .bind(batch_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((public_id, status, currency, total_minor, approval_id, created_at)) = row else {
        return Ok(None);
    };
    let expense_ids: Vec<(String,)> = sqlx::query_as(
        "SELECT public_id FROM finance_expense WHERE org_id = $1 AND reimbursement_batch_id = $2",
    )
    .bind(org_id)
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(Some(ReimbursementBatchDto {
        id: public_id,
        status,
        currency,
        total_minor,
        expense_ids: expense_ids.into_iter().map(|(s,)| s).collect(),
        approval_id,
        created_at: created_at.to_rfc3339(),
    }))
}

/// POST /api/v1/finance/reimbursements
#[utoipa::path(
    post,
    path = "/api/v1/finance/reimbursements",
    tag = "finance-policy",
    request_body = CreateReimbursementBatchRequest,
    responses((status = 201, body = ReimbursementBatchDto), (status = 200, body = ReimbursementBatchDto))
)]
pub async fn create_reimbursement_batch(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateReimbursementBatchRequest>,
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
        perms::finance_reimbursement_manage(),
        &request_id,
    )?;

    if body.expense_ids.is_empty() {
        return Err(validation(&request_id, "expense_ids required"));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status, stored)) =
            idempotency::get(&mut *tx, org_id, "reimbursement.batch", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let mut expense_uuids = Vec::new();
    let mut total_minor: i64 = 0;
    let mut currency: Option<String> = body.currency.clone();

    for raw in &body.expense_ids {
        let eid = parse_public_id(IdKind::Expense, raw, &request_id)?;
        let row: Option<(String, i64, String, Option<Uuid>)> = sqlx::query_as(
            r#"
            SELECT status, amount_minor, currency, reimbursement_batch_id
            FROM finance_expense
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(org_id)
        .bind(eid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
        let Some((status, amount, cur, existing_batch)) = row else {
            return Err(not_found(&request_id, "expense"));
        };
        if status != "posted" && status != "approved" {
            return Err(validation(
                &request_id,
                format!("expense {raw} must be posted or approved (was {status})"),
            ));
        }
        if existing_batch.is_some() {
            return Err(validation(
                &request_id,
                format!("expense {raw} already in a reimbursement batch"),
            ));
        }
        if let Some(ref c) = currency {
            if c != &cur {
                return Err(validation(
                    &request_id,
                    "all expenses in a batch must share currency",
                ));
            }
        } else {
            currency = Some(cur);
        }
        total_minor = total_minor
            .checked_add(amount)
            .ok_or_else(|| validation(&request_id, "amount overflow"))?;
        expense_uuids.push(eid);
    }

    let currency = currency.unwrap_or_else(|| "USD".into());
    let batch_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::ReimbursementBatch, batch_id);

    sqlx::query(
        r#"
        INSERT INTO finance_reimbursement_batch (
            id, org_id, public_id, status, currency, total_minor, owner_user_id
        ) VALUES ($1,$2,$3,'pending_approval',$4,$5,$6)
        "#,
    )
    .bind(batch_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&currency)
    .bind(total_minor)
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    for eid in &expense_uuids {
        sqlx::query(
            "UPDATE finance_expense SET reimbursement_batch_id = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(eid)
        .bind(batch_id)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "reimbursement",
        "batched",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "total_minor": total_minor,
            "expense_count": expense_uuids.len(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.reimbursement.batch",
        "reimbursement_batch",
        &public_id.as_str(),
        serde_json::json!({ "total_minor": total_minor }),
    )
    .await
    .map_err(internal(&request_id))?;

    let mut dto = fetch_reimb_dto(&mut tx, org_id, batch_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "batch missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "reimbursement.batch",
            key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    if let Some(apr_id) =
        request_reimb_approval(&auth, &public_id.as_str(), total_minor, &currency).await
    {
        if let Ok(mut link_tx) = state.pool.begin().await {
            let _ = set_session_org_id(&mut link_tx, auth.ctx.org_id).await;
            let _ = sqlx::query(
                "UPDATE finance_reimbursement_batch SET approval_id = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
            )
            .bind(org_id)
            .bind(batch_id)
            .bind(&apr_id)
            .execute(&mut *link_tx)
            .await;
            let _ = link_tx.commit().await;
            dto.approval_id = Some(apr_id);
        }
    }

    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// POST /api/v1/finance/reimbursements/{id}/decide
#[utoipa::path(
    post,
    path = "/api/v1/finance/reimbursements/{id}/decide",
    tag = "finance-policy",
    request_body = DecideReimbursementRequest,
    responses((status = 200, body = ReimbursementBatchDto))
)]
pub async fn decide_reimbursement(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<DecideReimbursementRequest>,
) -> Result<Json<ReimbursementBatchDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let batch_id = parse_public_id(IdKind::ReimbursementBatch, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_reimbursement_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM finance_reimbursement_batch WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(batch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some(status) = status else {
        return Err(not_found(&request_id, "reimbursement batch"));
    };
    if status != "pending_approval" {
        return Err(validation(
            &request_id,
            format!("batch status {status} is not pending_approval"),
        ));
    }

    let new_status = if body.approve { "approved" } else { "rejected" };
    sqlx::query(
        r#"
        UPDATE finance_reimbursement_batch SET
            status = $3,
            decided_by = $4,
            decided_at = now(),
            decision_note = $5,
            updated_at = now()
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(batch_id)
    .bind(new_status)
    .bind(auth.ctx.actor.user_id)
    .bind(&body.note)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Finance,
        "reimbursement",
        if body.approve { "approved" } else { "rejected" },
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": id }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        if body.approve {
            "finance.reimbursement.approve"
        } else {
            "finance.reimbursement.reject"
        },
        "reimbursement_batch",
        &id,
        serde_json::json!({ "note": body.note }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_reimb_dto(&mut tx, org_id, batch_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "reimbursement batch"))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
