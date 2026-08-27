//! `/api/v1/finance/accounts` — chart of accounts tree + manage.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::{conflict, internal, not_found, parse_optional_public_id, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::journal::ensure_ledger_accounts;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CreateLedgerAccountRequest, LedgerAccountDto, LedgerAccountNode, LedgerAccountTreeResponse,
    UpdateLedgerAccountRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/finance/accounts",
            get(list_accounts_tree).post(create_account),
        )
        .route("/api/v1/finance/accounts/{id}", patch(update_account))
}

#[derive(Debug, sqlx::FromRow)]
struct AccountRow {
    id: Uuid,
    public_id: String,
    code: String,
    name: String,
    account_type: String,
    normal_balance: String,
    parent_id: Option<Uuid>,
    parent_public_id: Option<String>,
    is_active: bool,
    description: Option<String>,
    sort_order: i32,
}

impl AccountRow {
    fn into_dto(self) -> LedgerAccountDto {
        LedgerAccountDto {
            id: self.public_id,
            code: self.code,
            name: self.name,
            account_type: self.account_type,
            normal_balance: self.normal_balance,
            parent_id: self.parent_public_id,
            is_active: self.is_active,
            description: self.description,
            sort_order: self.sort_order,
        }
    }
}

const ACCOUNT_SELECT: &str = r#"
    a.id, a.public_id, a.code, a.name, a.account_type, a.normal_balance,
    a.parent_id, p.public_id AS parent_public_id, a.is_active, a.description, a.sort_order
"#;

fn default_normal_balance(account_type: &str) -> &'static str {
    match account_type {
        "asset" | "expense" => "debit",
        "liability" | "equity" | "revenue" | "income" => "credit",
        _ => "debit",
    }
}

fn validate_account_type(request_id: &str, ty: &str) -> Result<(), AppError> {
    match ty {
        "asset" | "liability" | "equity" | "revenue" | "income" | "expense" => Ok(()),
        _ => Err(validation(
            request_id,
            "account_type must be asset|liability|equity|revenue|income|expense",
        )),
    }
}

fn build_tree(rows: Vec<AccountRow>) -> Vec<LedgerAccountNode> {
    use std::collections::HashMap;

    let mut by_parent: HashMap<Option<Uuid>, Vec<AccountRow>> = HashMap::new();
    for row in rows {
        by_parent.entry(row.parent_id).or_default().push(row);
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.code.cmp(&b.code))
        });
    }

    fn recurse(
        parent: Option<Uuid>,
        by_parent: &HashMap<Option<Uuid>, Vec<AccountRow>>,
    ) -> Vec<LedgerAccountNode> {
        let Some(children) = by_parent.get(&parent) else {
            return Vec::new();
        };
        children
            .iter()
            .map(|row| {
                let id = row.id;
                LedgerAccountNode {
                    account: LedgerAccountDto {
                        id: row.public_id.clone(),
                        code: row.code.clone(),
                        name: row.name.clone(),
                        account_type: row.account_type.clone(),
                        normal_balance: row.normal_balance.clone(),
                        parent_id: row.parent_public_id.clone(),
                        is_active: row.is_active,
                        description: row.description.clone(),
                        sort_order: row.sort_order,
                    },
                    children: recurse(Some(id), by_parent),
                }
            })
            .collect()
    }

    recurse(None, &by_parent)
}

async fn fetch_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    account_id: Uuid,
) -> Result<Option<AccountRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {ACCOUNT_SELECT}
         FROM finance_ledger_account a
         LEFT JOIN finance_ledger_account p ON p.id = a.parent_id
         WHERE a.org_id = $1 AND a.id = $2"
    ))
    .bind(org_id)
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await
}

/// GET /api/v1/finance/accounts
#[utoipa::path(
    get,
    path = "/api/v1/finance/accounts",
    tag = "finance-accounts",
    responses((status = 200, body = LedgerAccountTreeResponse))
)]
pub async fn list_accounts_tree(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<LedgerAccountTreeResponse>, AppError> {
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
        perms::finance_ledger_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let rows: Vec<AccountRow> = sqlx::query_as(&format!(
        "SELECT {ACCOUNT_SELECT}
         FROM finance_ledger_account a
         LEFT JOIN finance_ledger_account p ON p.id = a.parent_id
         WHERE a.org_id = $1
         ORDER BY a.sort_order, a.code"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(LedgerAccountTreeResponse {
        roots: build_tree(rows),
    }))
}

/// POST /api/v1/finance/accounts
#[utoipa::path(
    post,
    path = "/api/v1/finance/accounts",
    tag = "finance-accounts",
    request_body = CreateLedgerAccountRequest,
    responses((status = 201, body = LedgerAccountDto))
)]
pub async fn create_account(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateLedgerAccountRequest>,
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
        perms::finance_account_manage(),
        &request_id,
    )?;

    let code = body.code.trim();
    if code.is_empty() {
        return Err(validation(&request_id, "code required"));
    }
    let name = body.name.trim();
    if name.is_empty() {
        return Err(validation(&request_id, "name required"));
    }
    let account_type = body.account_type.trim().to_ascii_lowercase();
    validate_account_type(&request_id, &account_type)?;
    let normal = body
        .normal_balance
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| default_normal_balance(&account_type).to_string());
    if normal != "debit" && normal != "credit" {
        return Err(validation(
            &request_id,
            "normal_balance must be debit or credit",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    ensure_ledger_accounts(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;

    let parent_uuid = parse_optional_public_id(
        IdKind::LedgerAccount,
        body.parent_id.as_deref(),
        &request_id,
    )?;
    if let Some(pid) = parent_uuid {
        let exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM finance_ledger_account WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(pid)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal(&request_id))?;
        if exists.is_none() {
            return Err(not_found(&request_id, "parent account"));
        }
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::LedgerAccount, id);
    let sort_order = body.sort_order.unwrap_or(0);

    let result = sqlx::query(
        r#"
        INSERT INTO finance_ledger_account (
            id, org_id, public_id, code, name, account_type, normal_balance,
            parent_id, is_active, description, sort_order
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,true,$9,$10)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(code)
    .bind(name)
    .bind(&account_type)
    .bind(&normal)
    .bind(parent_uuid)
    .bind(&body.description)
    .bind(sort_order)
    .execute(&mut *tx)
    .await;

    if let Err(e) = result {
        if super::is_unique_violation(&e, "finance_ledger_account_org_id_code_key") {
            return Err(conflict(
                &request_id,
                format!("account code {code} already exists"),
            ));
        }
        return Err(internal(&request_id)(e));
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.account.create",
        "ledger_account",
        &public_id.as_str(),
        serde_json::json!({ "code": code, "account_type": account_type }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_account(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "account missing after insert",
            )
        })?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// PATCH /api/v1/finance/accounts/{id}
#[utoipa::path(
    patch,
    path = "/api/v1/finance/accounts/{id}",
    tag = "finance-accounts",
    request_body = UpdateLedgerAccountRequest,
    responses((status = 200, body = LedgerAccountDto))
)]
pub async fn update_account(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateLedgerAccountRequest>,
) -> Result<Json<LedgerAccountDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let account_id = parse_public_id(IdKind::LedgerAccount, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::finance_account_manage(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let existing = fetch_account(&mut tx, org_id, account_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "ledger account"))?;

    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.name);

    let parent_uuid = if body.parent_id.is_some() {
        let parsed = parse_optional_public_id(
            IdKind::LedgerAccount,
            body.parent_id.as_deref(),
            &request_id,
        )?;
        if let Some(pid) = parsed {
            if pid == account_id {
                return Err(validation(&request_id, "account cannot be its own parent"));
            }
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM finance_ledger_account WHERE org_id = $1 AND id = $2",
            )
            .bind(org_id)
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if exists.is_none() {
                return Err(not_found(&request_id, "parent account"));
            }
        }
        parsed
    } else {
        existing.parent_id
    };

    let is_active = body.is_active.unwrap_or(existing.is_active);
    let description = body
        .description
        .clone()
        .or_else(|| existing.description.clone());
    let sort_order = body.sort_order.unwrap_or(existing.sort_order);

    sqlx::query(
        r#"
        UPDATE finance_ledger_account SET
            name = $3,
            parent_id = $4,
            is_active = $5,
            description = $6,
            sort_order = $7
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(account_id)
    .bind(name)
    .bind(parent_uuid)
    .bind(is_active)
    .bind(&description)
    .bind(sort_order)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "finance.account.update",
        "ledger_account",
        &existing.public_id,
        serde_json::json!({ "is_active": is_active }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_account(&mut tx, org_id, account_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "ledger account"))?
        .into_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
