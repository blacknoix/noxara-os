//! `/api/v1/people/employees/{id}/contracts` — employment contracts.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::employees::{enforce_employee_scope, fetch_employee_row, parse_optional_date};
use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{ContractDto, ContractListResponse, CreateContractRequest};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/employees/{id}/contracts",
        get(list_contracts).post(create_contract),
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ContractRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    employee_id: Uuid,
    contract_type: String,
    title: Option<String>,
    effective_from: NaiveDate,
    effective_to: Option<NaiveDate>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    version: i32,
}

impl ContractRow {
    fn into_dto(self, employee_public: &str) -> ContractDto {
        ContractDto {
            id: self.public_id,
            employee_id: employee_public.to_string(),
            contract_type: self.contract_type,
            title: self.title,
            effective_from: self.effective_from.to_string(),
            effective_to: self.effective_to.map(|d| d.to_string()),
            notes: self.notes,
            created_at: self.created_at.to_rfc3339(),
            version: self.version,
        }
    }
}

/// GET /api/v1/people/employees/{id}/contracts
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/contracts",
    tag = "people-contracts",
    params(("id" = String, Path)),
    responses((status = 200, body = ContractListResponse), (status = 404))
)]
pub async fn list_contracts(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ContractListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_read(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let rows: Vec<ContractRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, employee_id, contract_type, title,
               effective_from, effective_to, notes, created_at, version
        FROM people_employment_contract
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
        ORDER BY effective_from DESC, created_at DESC
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(ContractListResponse {
        items: rows
            .into_iter()
            .map(|r| r.into_dto(&emp.public_id))
            .collect(),
    }))
}

/// POST /api/v1/people/employees/{id}/contracts
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/{id}/contracts",
    tag = "people-contracts",
    request_body = CreateContractRequest,
    params(("id" = String, Path)),
    responses((status = 201, body = ContractDto))
)]
pub async fn create_contract(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateContractRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;
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
        perms::hr_employee_write(),
        &request_id,
    )?;

    let effective_from = parse_optional_date(Some(body.effective_from.as_str()), "effective_from", &request_id)?
        .ok_or_else(|| validation(&request_id, "effective_from is required"))?;
    let effective_to =
        parse_optional_date(body.effective_to.as_deref(), "effective_to", &request_id)?;
    let contract_type = body
        .contract_type
        .as_deref()
        .unwrap_or("full_time")
        .to_string();

    let public_id = PublicId::generate(IdKind::EmploymentContract);
    let id_uuid = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "contract.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_write(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    sqlx::query(
        r#"
        INSERT INTO people_employment_contract (
            id, org_id, public_id, employee_id, contract_type, title,
            effective_from, effective_to, notes
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(employee_id)
    .bind(&contract_type)
    .bind(&body.title)
    .bind(effective_from)
    .bind(effective_to)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.contract.create",
        "contract",
        &public_id.as_str(),
        serde_json::json!({ "employee_id": emp.public_id, "contract_type": contract_type }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = ContractDto {
        id: public_id.as_str(),
        employee_id: emp.public_id,
        contract_type,
        title: body.title,
        effective_from: effective_from.to_string(),
        effective_to: effective_to.map(|d| d.to_string()),
        notes: body.notes,
        created_at: Utc::now().to_rfc3339(),
        version: 1,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "contract.create",
            key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}
