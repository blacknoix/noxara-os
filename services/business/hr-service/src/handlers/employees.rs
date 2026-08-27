//! `/api/v1/people/employees` — employee CRUD (directory + sensitive detail).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::{decide_with_scope, perms, Decision, Scope};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::{
    conflict, crypto_err, internal, normalize_paging, not_found, parse_public_id, parse_user_ref,
    require_if_match, user_public, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::crypto::FieldEncryptor;
use crate::idempotency;
use crate::principal::{
    enforce_any_scope, enforce_scoped, load_membership_scope, required_scope_for_owner_row,
    MembershipScope,
};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateEmployeeRequest, EmployeeDto, EmployeeListResponse, ListQuery, UpdateEmployeeRequest,
    EMPLOYEE_STATUSES,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/people/employees",
            get(list_employees).post(create_employee),
        )
        .route(
            "/api/v1/people/employees/{id}",
            get(get_employee).patch(update_employee),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct EmployeeRow {
    pub(crate) id: Uuid,
    pub(crate) public_id: String,
    pub(crate) user_id: Option<Uuid>,
    pub(crate) display_name: String,
    pub(crate) legal_first_name: Option<String>,
    pub(crate) legal_last_name: Option<String>,
    pub(crate) work_email: Option<String>,
    pub(crate) personal_email: Option<String>,
    pub(crate) phone: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) status: String,
    pub(crate) start_date: Option<NaiveDate>,
    pub(crate) end_date: Option<NaiveDate>,
    pub(crate) location: Option<String>,
    pub(crate) department_id: Option<Uuid>,
    pub(crate) department_public_id: Option<String>,
    pub(crate) manager_employee_id: Option<Uuid>,
    pub(crate) owner_user_id: Uuid,
    pub(crate) government_id_ciphertext: Option<Vec<u8>>,
    pub(crate) bank_details_ciphertext: Option<Vec<u8>>,
    pub(crate) tax_id_ciphertext: Option<Vec<u8>>,
    #[allow(dead_code)]
    pub(crate) encryption_key_id: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) version: i32,
}

pub(crate) const EMPLOYEE_COLUMNS: &str = r#"
    id, public_id, user_id, display_name, legal_first_name, legal_last_name,
    work_email, personal_email, phone, title, status, start_date, end_date,
    location, department_id, department_public_id, manager_employee_id, owner_user_id,
    government_id_ciphertext, bank_details_ciphertext, tax_id_ciphertext,
    encryption_key_id, created_at, updated_at, version
"#;

impl EmployeeRow {
    /// Directory projection — never includes restricted fields.
    pub(crate) fn into_directory_dto(self) -> EmployeeDto {
        let manager_public = self
            .manager_employee_id
            .map(|u| PublicId::new(IdKind::Employee, u).as_str());
        let dept_public = self.department_public_id.or_else(|| {
            self.department_id
                .map(|u| PublicId::new(IdKind::Department, u).as_str())
        });

        EmployeeDto {
            id: self.public_id,
            user_id: self.user_id.map(user_public),
            display_name: self.display_name,
            legal_first_name: self.legal_first_name,
            legal_last_name: self.legal_last_name,
            work_email: self.work_email,
            personal_email: self.personal_email,
            phone: self.phone,
            title: self.title,
            status: self.status,
            start_date: self.start_date.map(|d| d.to_string()),
            end_date: self.end_date.map(|d| d.to_string()),
            location: self.location,
            department_id: dept_public,
            manager_employee_id: manager_public,
            owner_user_id: user_public(self.owner_user_id),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
            government_id: None,
            bank_details: None,
            tax_id: None,
        }
    }

    pub(crate) fn into_sensitive_dto(
        self,
        encryptor: &FieldEncryptor,
        request_id: &str,
    ) -> Result<EmployeeDto, AppError> {
        let manager_public = self
            .manager_employee_id
            .map(|u| PublicId::new(IdKind::Employee, u).as_str());
        let dept_public = self.department_public_id.clone().or_else(|| {
            self.department_id
                .map(|u| PublicId::new(IdKind::Department, u).as_str())
        });

        Ok(EmployeeDto {
            id: self.public_id,
            user_id: self.user_id.map(user_public),
            display_name: self.display_name,
            legal_first_name: self.legal_first_name,
            legal_last_name: self.legal_last_name,
            work_email: self.work_email,
            personal_email: self.personal_email,
            phone: self.phone,
            title: self.title,
            status: self.status,
            start_date: self.start_date.map(|d| d.to_string()),
            end_date: self.end_date.map(|d| d.to_string()),
            location: self.location,
            department_id: dept_public,
            manager_employee_id: manager_public,
            owner_user_id: user_public(self.owner_user_id),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
            government_id: decrypt_opt_str(
                encryptor,
                self.government_id_ciphertext.as_deref(),
                request_id,
            )?,
            bank_details: decrypt_opt_str(
                encryptor,
                self.bank_details_ciphertext.as_deref(),
                request_id,
            )?,
            tax_id: decrypt_opt_str(encryptor, self.tax_id_ciphertext.as_deref(), request_id)?,
        })
    }
}

fn decrypt_opt_str(
    encryptor: &FieldEncryptor,
    blob: Option<&[u8]>,
    request_id: &str,
) -> Result<Option<String>, AppError> {
    match blob {
        None => Ok(None),
        Some([]) => Ok(None),
        Some(b) => encryptor
            .decrypt_str(b)
            .map(Some)
            .map_err(|e| crypto_err(request_id, e)),
    }
}

pub(crate) fn validate_employee_status(status: &str, request_id: &str) -> Result<(), AppError> {
    if EMPLOYEE_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(validation(
            request_id,
            format!("status must be one of: {}", EMPLOYEE_STATUSES.join("|")),
        ))
    }
}

pub(crate) fn parse_optional_date(
    raw: Option<&str>,
    field: &str,
    request_id: &str,
) -> Result<Option<NaiveDate>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| validation(request_id, format!("{field} must be YYYY-MM-DD"))),
    }
}

/// Opaque department public id → (uuid, original public id text).
pub(crate) fn parse_department_link(
    raw: Option<&str>,
    request_id: &str,
) -> Result<Option<(Uuid, String)>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            let uuid = parse_public_id(IdKind::Department, s, request_id)?;
            Ok(Some((uuid, s.to_string())))
        }
    }
}

pub(crate) async fn fetch_employee_row(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
) -> Result<Option<EmployeeRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {EMPLOYEE_COLUMNS} FROM people_employee
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(employee_id)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn fetch_employee_by_user(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<EmployeeRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {EMPLOYEE_COLUMNS} FROM people_employee
         WHERE org_id = $1 AND user_id = $2 AND deleted_at IS NULL
         ORDER BY created_at DESC LIMIT 1"
    ))
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn enforce_employee_scope(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    membership: &MembershipScope,
    permission: companyos_authz::PermissionId,
    owner_user_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let required = required_scope_for_owner_row(
        tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        Some(owner_user_id),
    )
    .await
    .map_err(internal(request_id))?;
    enforce_scoped(&membership.principal, permission, required, request_id)
}

pub(crate) fn can_read_sensitive(membership: &MembershipScope) -> bool {
    decide_with_scope(
        &membership.principal,
        &perms::hr_employee_read_sensitive(),
        Scope::Own,
    )
    .decision
        == Decision::Allow
}

pub(crate) async fn resolve_manager_id(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    raw: Option<&str>,
    request_id: &str,
) -> Result<Option<Uuid>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            let mid = parse_public_id(IdKind::Employee, s, request_id)?;
            let exists: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM people_employee WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(org_id)
            .bind(mid)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal(request_id))?;
            if exists.is_none() {
                return Err(validation(request_id, "manager_employee_id not found"));
            }
            Ok(Some(mid))
        }
    }
}

pub(crate) async fn insert_timeline(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    employee_id: Uuid,
    event_type: &str,
    summary: &str,
    metadata: serde_json::Value,
    actor_user_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO people_timeline_event (
            id, org_id, employee_id, event_type, summary, metadata, actor_user_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(companyos_ids::new_uuid_v7())
    .bind(org_id)
    .bind(employee_id)
    .bind(event_type)
    .bind(summary)
    .bind(metadata)
    .bind(actor_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn encrypt_opt_str(
    encryptor: &FieldEncryptor,
    value: Option<&str>,
    request_id: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    match value {
        None => Ok(None),
        Some("") => Ok(None),
        Some(s) => encryptor
            .encrypt_str(s)
            .map(Some)
            .map_err(|e| crypto_err(request_id, e)),
    }
}

/// GET /api/v1/people/employees
#[utoipa::path(
    get,
    path = "/api/v1/people/employees",
    tag = "people-employees",
    params(
        ("status" = Option<String>, Query),
        ("department_id" = Option<String>, Query),
        ("q" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = EmployeeListResponse))
)]
pub async fn list_employees(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<EmployeeListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_employee_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::hr_employee_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    if let Some(status) = q.status.as_deref() {
        validate_employee_status(status, &request_id)?;
    }
    let dept_filter = parse_department_link(q.department_id.as_deref(), &request_id)?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let build_filters = |qb: &mut QueryBuilder<Postgres>| {
        push_owner_predicate(
            qb,
            scope,
            org_id,
            actor,
            membership.team_id,
            membership.department_id,
        );
        if let Some(status) = q.status.as_deref() {
            qb.push(" AND status = ");
            qb.push_bind(status.to_string());
        }
        if let Some((dept_uuid, _)) = &dept_filter {
            qb.push(" AND department_id = ");
            qb.push_bind(*dept_uuid);
        }
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND display_name ILIKE ");
            qb.push_bind(format!("%{term}%"));
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM people_employee WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {EMPLOYEE_COLUMNS} FROM people_employee WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<EmployeeRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    // Directory list NEVER returns government_id / bank_details / tax_id.
    Ok(Json(EmployeeListResponse {
        items: rows
            .into_iter()
            .map(EmployeeRow::into_directory_dto)
            .collect(),
        total,
    }))
}

/// POST /api/v1/people/employees
#[utoipa::path(
    post,
    path = "/api/v1/people/employees",
    tag = "people-employees",
    request_body = CreateEmployeeRequest,
    responses((status = 201, body = EmployeeDto))
)]
pub async fn create_employee(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateEmployeeRequest>,
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
        perms::hr_employee_write(),
        &request_id,
    )?;

    if body.display_name.trim().is_empty() {
        return Err(validation(&request_id, "display_name must not be empty"));
    }
    let status = body.status.as_deref().unwrap_or("active");
    validate_employee_status(status, &request_id)?;

    let user_id = match body.user_id.as_deref() {
        Some(s) => Some(parse_user_ref(s, &request_id)?),
        None => None,
    };
    let owner_user_id = user_id.unwrap_or(auth.ctx.actor.user_id);
    let dept = parse_department_link(body.department_id.as_deref(), &request_id)?;
    let start_date = parse_optional_date(body.start_date.as_deref(), "start_date", &request_id)?;

    let gov_ct = encrypt_opt_str(&state.encryptor, body.government_id.as_deref(), &request_id)?;
    let bank_ct = encrypt_opt_str(&state.encryptor, body.bank_details.as_deref(), &request_id)?;
    let tax_ct = encrypt_opt_str(&state.encryptor, body.tax_id.as_deref(), &request_id)?;
    let key_id = if gov_ct.is_some() || bank_ct.is_some() || tax_ct.is_some() {
        Some(state.encryptor.key_id().to_string())
    } else {
        None
    };

    let public_id = PublicId::generate(IdKind::Employee);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "employee.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let manager_id =
        resolve_manager_id(&mut tx, org_id, body.manager_employee_id.as_deref(), &request_id)
            .await?;

    let (department_id, department_public_id) = match &dept {
        Some((u, p)) => (Some(*u), Some(p.clone())),
        None => (None, None),
    };

    sqlx::query(
        r#"
        INSERT INTO people_employee (
            id, org_id, public_id, user_id, display_name, legal_first_name, legal_last_name,
            work_email, personal_email, phone, title, status, start_date, location,
            department_id, department_public_id, manager_employee_id, owner_user_id,
            government_id_ciphertext, bank_details_ciphertext, tax_id_ciphertext,
            encryption_key_id
        ) VALUES (
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22
        )
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(user_id)
    .bind(body.display_name.trim())
    .bind(&body.legal_first_name)
    .bind(&body.legal_last_name)
    .bind(&body.work_email)
    .bind(&body.personal_email)
    .bind(&body.phone)
    .bind(&body.title)
    .bind(status)
    .bind(start_date)
    .bind(&body.location)
    .bind(department_id)
    .bind(&department_public_id)
    .bind(manager_id)
    .bind(owner_user_id)
    .bind(&gov_ct)
    .bind(&bank_ct)
    .bind(&tax_ct)
    .bind(&key_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "employee",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "display_name": body.display_name.trim(),
            "status": status,
            "user_id": user_id.map(user_public),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    insert_timeline(
        &mut tx,
        org_id,
        id,
        "employee.created",
        &format!("Employee {} created", body.display_name.trim()),
        serde_json::json!({ "status": status }),
        Some(auth.ctx.actor.user_id),
    )
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.employee.create",
        "employee",
        &public_id.as_str(),
        serde_json::json!({ "status": status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_employee_row(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "employee missing after insert",
            )
        })?
        .into_directory_dto();

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "employee.create",
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

/// GET /api/v1/people/employees/{id}
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}",
    tag = "people-employees",
    params(("id" = String, Path, description = "Employee public id (emp_…)")),
    responses((status = 200, body = EmployeeDto), (status = 404))
)]
pub async fn get_employee(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<EmployeeDto>, AppError> {
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

    let row = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    let include_sensitive = can_read_sensitive(&membership);
    let dto = if include_sensitive {
        // Audit every sensitive read.
        insert_audit(
            &mut *tx,
            org_id,
            auth.ctx.actor.user_id,
            auth.ctx.actor.on_behalf_of,
            auth.ctx.actor.is_ai,
            "hr.employee.read_sensitive",
            "employee",
            &row.public_id,
            serde_json::json!({ "fields": ["government_id", "bank_details", "tax_id"] }),
        )
        .await
        .map_err(internal(&request_id))?;
        row.into_sensitive_dto(&state.encryptor, &request_id)?
    } else {
        row.into_directory_dto()
    };

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// PATCH /api/v1/people/employees/{id}
#[utoipa::path(
    patch,
    path = "/api/v1/people/employees/{id}",
    tag = "people-employees",
    request_body = UpdateEmployeeRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = EmployeeDto), (status = 404), (status = 409))
)]
pub async fn update_employee(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateEmployeeRequest>,
) -> Result<Json<EmployeeDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;
    let expected_version = require_if_match(&headers, &request_id)?;

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

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_employee_write(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    if expected_version != row.version {
        return Err(conflict(
            &request_id,
            format!(
                "version mismatch: expected {expected_version}, current {}",
                row.version
            ),
        ));
    }

    let display_name = body.display_name.unwrap_or(row.display_name);
    if display_name.trim().is_empty() {
        return Err(validation(&request_id, "display_name must not be empty"));
    }
    let status = body.status.unwrap_or(row.status);
    validate_employee_status(&status, &request_id)?;

    let user_id = if body.user_id.is_some() {
        match body.user_id.as_deref() {
            Some(s) if !s.trim().is_empty() => Some(parse_user_ref(s, &request_id)?),
            _ => None,
        }
    } else {
        row.user_id
    };
    let owner_user_id = user_id.unwrap_or(row.owner_user_id);

    let (department_id, department_public_id) = if body.department_id.is_some() {
        match parse_department_link(body.department_id.as_deref(), &request_id)? {
            Some((u, p)) => (Some(u), Some(p)),
            None => (None, None),
        }
    } else {
        (row.department_id, row.department_public_id)
    };

    let manager_employee_id = if body.manager_employee_id.is_some() {
        resolve_manager_id(
            &mut tx,
            org_id,
            body.manager_employee_id.as_deref(),
            &request_id,
        )
        .await?
    } else {
        row.manager_employee_id
    };

    let start_date = if body.start_date.is_some() {
        parse_optional_date(body.start_date.as_deref(), "start_date", &request_id)?
    } else {
        row.start_date
    };
    let end_date = if body.end_date.is_some() {
        parse_optional_date(body.end_date.as_deref(), "end_date", &request_id)?
    } else {
        row.end_date
    };

    let gov_ct = if body.government_id.is_some() {
        encrypt_opt_str(&state.encryptor, body.government_id.as_deref(), &request_id)?
    } else {
        row.government_id_ciphertext
    };
    let bank_ct = if body.bank_details.is_some() {
        encrypt_opt_str(&state.encryptor, body.bank_details.as_deref(), &request_id)?
    } else {
        row.bank_details_ciphertext
    };
    let tax_ct = if body.tax_id.is_some() {
        encrypt_opt_str(&state.encryptor, body.tax_id.as_deref(), &request_id)?
    } else {
        row.tax_id_ciphertext
    };
    let key_id = if gov_ct.is_some() || bank_ct.is_some() || tax_ct.is_some() {
        Some(state.encryptor.key_id().to_string())
    } else {
        row.encryption_key_id
    };

    let updated: EmployeeRow = sqlx::query_as(&format!(
        r#"
        UPDATE people_employee SET
            display_name = $3, user_id = $4, legal_first_name = $5, legal_last_name = $6,
            work_email = $7, personal_email = $8, phone = $9, title = $10, status = $11,
            start_date = $12, end_date = $13, location = $14,
            department_id = $15, department_public_id = $16, manager_employee_id = $17,
            owner_user_id = $18,
            government_id_ciphertext = $19, bank_details_ciphertext = $20, tax_id_ciphertext = $21,
            encryption_key_id = $22,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING {EMPLOYEE_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(employee_id)
    .bind(display_name.trim())
    .bind(user_id)
    .bind(body.legal_first_name.or(row.legal_first_name))
    .bind(body.legal_last_name.or(row.legal_last_name))
    .bind(body.work_email.or(row.work_email))
    .bind(body.personal_email.or(row.personal_email))
    .bind(body.phone.or(row.phone))
    .bind(body.title.or(row.title))
    .bind(&status)
    .bind(start_date)
    .bind(end_date)
    .bind(body.location.or(row.location))
    .bind(department_id)
    .bind(&department_public_id)
    .bind(manager_employee_id)
    .bind(owner_user_id)
    .bind(&gov_ct)
    .bind(&bank_ct)
    .bind(&tax_ct)
    .bind(&key_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::People,
        "employee",
        "updated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": updated.public_id,
            "status": status,
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
        "hr.employee.update",
        "employee",
        &updated.public_id,
        serde_json::json!({ "status": status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = updated.into_directory_dto();
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}
