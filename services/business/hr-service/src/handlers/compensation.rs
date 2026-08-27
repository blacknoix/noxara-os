//! `/api/v1/people/employees/{id}/compensation` — encrypted compensation components.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_money::Currency;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::employees::{
    can_read_sensitive, enforce_employee_scope, fetch_employee_row, parse_optional_date,
};
use super::{crypto_err, internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    CompensationComponentDto, CompensationListResponse, CreateCompensationRequest,
};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/employees/{id}/compensation",
        get(list_compensation).post(create_compensation),
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CompRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    employee_id: Uuid,
    contract_id: Option<Uuid>,
    component_type: String,
    label: String,
    amount_minor_ciphertext: Vec<u8>,
    currency: String,
    #[allow(dead_code)]
    encryption_key_id: String,
    effective_from: NaiveDate,
    effective_to: Option<NaiveDate>,
    created_at: DateTime<Utc>,
    version: i32,
}

impl CompRow {
    fn into_dto(
        self,
        encryptor: &crate::crypto::FieldEncryptor,
        employee_public: &str,
        request_id: &str,
    ) -> Result<CompensationComponentDto, AppError> {
        let amount_minor = encryptor
            .decrypt_i64(&self.amount_minor_ciphertext)
            .map_err(|e| crypto_err(request_id, e))?;
        Ok(CompensationComponentDto {
            id: self.public_id,
            employee_id: employee_public.to_string(),
            contract_id: self
                .contract_id
                .map(|u| PublicId::new(IdKind::EmploymentContract, u).as_str()),
            component_type: self.component_type,
            label: self.label,
            amount_minor,
            currency: self.currency,
            effective_from: self.effective_from.to_string(),
            effective_to: self.effective_to.map(|d| d.to_string()),
            created_at: self.created_at.to_rfc3339(),
            version: self.version,
        })
    }
}

/// GET /api/v1/people/employees/{id}/compensation
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/compensation",
    tag = "people-compensation",
    params(("id" = String, Path)),
    responses((status = 200, body = CompensationListResponse), (status = 403), (status = 404))
)]
pub async fn list_compensation(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<CompensationListResponse>, AppError> {
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
        perms::hr_employee_read_sensitive(),
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
        perms::hr_employee_read_sensitive(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    if !can_read_sensitive(&membership) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "missing permission hr.employee.read_sensitive",
        ));
    }

    let rows: Vec<CompRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, employee_id, contract_id, component_type, label,
               amount_minor_ciphertext, currency, encryption_key_id,
               effective_from, effective_to, created_at, version
        FROM people_compensation_component
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
        ORDER BY effective_from DESC, created_at DESC
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.employee.read_sensitive",
        "employee",
        &emp.public_id,
        serde_json::json!({ "resource": "compensation", "count": rows.len() }),
    )
    .await
    .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_dto(&state.encryptor, &emp.public_id, &request_id)?);
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(CompensationListResponse { items }))
}

/// POST /api/v1/people/employees/{id}/compensation
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/{id}/compensation",
    tag = "people-compensation",
    request_body = CreateCompensationRequest,
    params(("id" = String, Path)),
    responses((status = 201, body = CompensationComponentDto))
)]
pub async fn create_compensation(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateCompensationRequest>,
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

    if body.label.trim().is_empty() {
        return Err(validation(&request_id, "label must not be empty"));
    }
    let currency = Currency::new(&body.currency)
        .map_err(|_| validation(&request_id, "currency must be ISO 4217 (3-letter uppercase)"))?;
    let effective_from = parse_optional_date(Some(body.effective_from.as_str()), "effective_from", &request_id)?
        .ok_or_else(|| validation(&request_id, "effective_from is required"))?;
    let effective_to =
        parse_optional_date(body.effective_to.as_deref(), "effective_to", &request_id)?;
    let component_type = body
        .component_type
        .as_deref()
        .unwrap_or("base_salary")
        .to_string();

    let amount_ct = state
        .encryptor
        .encrypt_i64(body.amount_minor)
        .map_err(|e| crypto_err(&request_id, e))?;

    let public_id = PublicId::generate(IdKind::CompensationComponent);
    let id_uuid = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "compensation.create", key)
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

    let contract_id = match body.contract_id.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            let cid = parse_public_id(IdKind::EmploymentContract, s, &request_id)?;
            let ok: Option<(Uuid,)> = sqlx::query_as(
                r#"
                SELECT id FROM people_employment_contract
                WHERE org_id = $1 AND id = $2 AND employee_id = $3 AND deleted_at IS NULL
                "#,
            )
            .bind(org_id)
            .bind(cid)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
            if ok.is_none() {
                return Err(validation(&request_id, "contract_id not found for employee"));
            }
            Some(cid)
        }
        _ => None,
    };

    sqlx::query(
        r#"
        INSERT INTO people_compensation_component (
            id, org_id, public_id, employee_id, contract_id, component_type, label,
            amount_minor_ciphertext, currency, encryption_key_id, effective_from, effective_to
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(employee_id)
    .bind(contract_id)
    .bind(&component_type)
    .bind(body.label.trim())
    .bind(&amount_ct)
    .bind(currency.as_str())
    .bind(state.encryptor.key_id())
    .bind(effective_from)
    .bind(effective_to)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.compensation.create",
        "compensation",
        &public_id.as_str(),
        serde_json::json!({
            "employee_id": emp.public_id,
            "component_type": component_type,
            "currency": currency.as_str(),
            // Never log amount_minor plaintext.
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = CompensationComponentDto {
        id: public_id.as_str(),
        employee_id: emp.public_id,
        contract_id: contract_id.map(|u| PublicId::new(IdKind::EmploymentContract, u).as_str()),
        component_type,
        label: body.label.trim().to_string(),
        amount_minor: body.amount_minor,
        currency: currency.as_str().to_string(),
        effective_from: effective_from.to_string(),
        effective_to: effective_to.map(|d| d.to_string()),
        created_at: Utc::now().to_rfc3339(),
        version: 1,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "compensation.create",
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
