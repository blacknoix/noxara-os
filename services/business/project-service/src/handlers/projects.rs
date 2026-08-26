//! `/api/v1/operations/projects` — project CRUD (soft delete, opaque Sales links).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{
    conflict, internal, normalize_paging, not_found, parse_public_id, parse_user_ref,
    require_if_match, user_public, validation,
};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{
    enforce_any_scope, enforce_scoped, load_membership_scope, required_scope_for_owner_row,
    MembershipScope,
};
use crate::scope::{push_owner_predicate, scope_for_permission};
use crate::state::AppState;
use crate::types::{
    CreateProjectRequest, ListQuery, ProjectDto, ProjectListResponse, UpdateProjectRequest,
    PROJECT_STATUSES,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/operations/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/v1/operations/projects/{id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ProjectRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    name: String,
    description: Option<String>,
    status: String,
    owner_user_id: Uuid,
    customer_id: Option<Uuid>,
    deal_id: Option<Uuid>,
    deal_public_id: Option<String>,
    customer_public_id: Option<String>,
    starts_at: Option<NaiveDate>,
    due_at: Option<NaiveDate>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i32,
}

const PROJECT_COLUMNS: &str = r#"
    id, public_id, name, description, status, owner_user_id,
    customer_id, deal_id, deal_public_id, customer_public_id,
    starts_at, due_at, created_at, updated_at, version
"#;

impl ProjectRow {
    fn into_dto(self) -> ProjectDto {
        ProjectDto {
            id: self.public_id,
            name: self.name,
            description: self.description,
            status: self.status,
            owner_user_id: user_public(self.owner_user_id),
            customer_id: self
                .customer_public_id
                .or_else(|| {
                    self.customer_id
                        .map(|u| PublicId::new(IdKind::Customer, u).as_str())
                }),
            deal_id: self.deal_public_id.or_else(|| {
                self.deal_id
                    .map(|u| PublicId::new(IdKind::Deal, u).as_str())
            }),
            starts_at: self.starts_at.map(|d| d.to_string()),
            due_at: self.due_at.map(|d| d.to_string()),
            created_at: self.created_at.to_rfc3339(),
            updated_at: self.updated_at.to_rfc3339(),
            version: self.version,
        }
    }
}

async fn fetch_project_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    project_id: Uuid,
) -> Result<Option<ProjectRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {PROJECT_COLUMNS} FROM operations_project
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn enforce_project_scope(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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

fn parse_optional_date(
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

fn validate_project_status(status: &str, request_id: &str) -> Result<(), AppError> {
    if PROJECT_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(validation(
            request_id,
            format!("status must be one of: {}", PROJECT_STATUSES.join("|")),
        ))
    }
}

/// Opaque Sales public id → (uuid, original public id text). Never joins sales_*.
fn parse_opaque_link(
    kind: IdKind,
    raw: Option<&str>,
    request_id: &str,
) -> Result<Option<(Uuid, String)>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            let uuid = parse_public_id(kind, s, request_id)?;
            Ok(Some((uuid, s.to_string())))
        }
    }
}

/// GET /api/v1/operations/projects
#[utoipa::path(get, path = "/api/v1/operations/projects", tag = "operations-projects",
    params(
        ("status" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = ProjectListResponse)))]
pub async fn list_projects(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<ProjectListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_project_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::operations_project_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    if let Some(status) = q.status.as_deref() {
        validate_project_status(status, &request_id)?;
    }

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
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND name ILIKE ");
            qb.push_bind(format!("%{term}%"));
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM operations_project WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {PROJECT_COLUMNS} FROM operations_project WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    build_filters(&mut qb);
    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<ProjectRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ProjectListResponse {
        items: rows.into_iter().map(ProjectRow::into_dto).collect(),
        total,
    }))
}

/// POST /api/v1/operations/projects
#[utoipa::path(post, path = "/api/v1/operations/projects", tag = "operations-projects",
    request_body = CreateProjectRequest,
    responses((status = 201, body = ProjectDto)))]
pub async fn create_project(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateProjectRequest>,
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
        perms::operations_project_create(),
        &request_id,
    )?;

    if body.name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }
    let status = body.status.as_deref().unwrap_or("active");
    validate_project_status(status, &request_id)?;

    let customer = parse_opaque_link(IdKind::Customer, body.customer_id.as_deref(), &request_id)?;
    let deal = parse_opaque_link(IdKind::Deal, body.deal_id.as_deref(), &request_id)?;
    let starts_at = parse_optional_date(body.starts_at.as_deref(), "starts_at", &request_id)?;
    let due_at = parse_optional_date(body.due_at.as_deref(), "due_at", &request_id)?;
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_user_ref(s, &request_id)?,
        None => auth.ctx.actor.user_id,
    };

    let public_id = PublicId::generate(IdKind::Project);
    let id = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "project.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let (customer_id, customer_public_id) = match &customer {
        Some((u, p)) => (Some(*u), Some(p.clone())),
        None => (None, None),
    };
    let (deal_id, deal_public_id) = match &deal {
        Some((u, p)) => (Some(*u), Some(p.clone())),
        None => (None, None),
    };

    sqlx::query(
        r#"
        INSERT INTO operations_project (
            id, org_id, public_id, name, description, status, owner_user_id,
            customer_id, deal_id, customer_public_id, deal_public_id, starts_at, due_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(status)
    .bind(owner_user_id)
    .bind(customer_id)
    .bind(deal_id)
    .bind(&customer_public_id)
    .bind(&deal_public_id)
    .bind(starts_at)
    .bind(due_at)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "project",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "name": body.name.trim(),
            "deal_id": deal_public_id,
            "customer_id": customer_public_id,
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
        "operations.project.create",
        "project",
        &public_id.as_str(),
        serde_json::json!({ "name": body.name.trim(), "status": status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_project_row(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "project missing after insert",
            )
        })?
        .into_dto();

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "project.create",
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

/// GET /api/v1/operations/projects/{id}
#[utoipa::path(get, path = "/api/v1/operations/projects/{id}", tag = "operations-projects",
    responses((status = 200, body = ProjectDto), (status = 404)))]
pub async fn get_project(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<ProjectDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let project_id = parse_public_id(IdKind::Project, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_project_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_project_row(&mut tx, org_id, project_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "project"))?;
    enforce_project_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_project_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(row.into_dto()))
}

/// PATCH /api/v1/operations/projects/{id}
#[utoipa::path(patch, path = "/api/v1/operations/projects/{id}", tag = "operations-projects",
    request_body = UpdateProjectRequest,
    responses((status = 200, body = ProjectDto), (status = 404), (status = 409)))]
pub async fn update_project(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let project_id = parse_public_id(IdKind::Project, &id, &request_id)?;
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
        perms::operations_project_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_project_row(&mut tx, org_id, project_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "project"))?;
    enforce_project_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_project_update(),
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

    let name = body.name.unwrap_or(row.name);
    if name.trim().is_empty() {
        return Err(validation(&request_id, "name must not be empty"));
    }
    let description = body.description.or(row.description);
    let status = body.status.unwrap_or(row.status);
    validate_project_status(&status, &request_id)?;

    let (customer_id, customer_public_id) = if body.customer_id.is_some() {
        match parse_opaque_link(IdKind::Customer, body.customer_id.as_deref(), &request_id)? {
            Some((u, p)) => (Some(u), Some(p)),
            None => (None, None),
        }
    } else {
        (row.customer_id, row.customer_public_id)
    };
    let (deal_id, deal_public_id) = if body.deal_id.is_some() {
        match parse_opaque_link(IdKind::Deal, body.deal_id.as_deref(), &request_id)? {
            Some((u, p)) => (Some(u), Some(p)),
            None => (None, None),
        }
    } else {
        (row.deal_id, row.deal_public_id)
    };

    let starts_at = if body.starts_at.is_some() {
        parse_optional_date(body.starts_at.as_deref(), "starts_at", &request_id)?
    } else {
        row.starts_at
    };
    let due_at = if body.due_at.is_some() {
        parse_optional_date(body.due_at.as_deref(), "due_at", &request_id)?
    } else {
        row.due_at
    };
    let owner_user_id = match body.owner_user_id.as_deref() {
        Some(s) => parse_user_ref(s, &request_id)?,
        None => row.owner_user_id,
    };

    let updated: ProjectRow = sqlx::query_as(&format!(
        r#"
        UPDATE operations_project SET
            name = $3, description = $4, status = $5, owner_user_id = $6,
            customer_id = $7, deal_id = $8, customer_public_id = $9, deal_public_id = $10,
            starts_at = $11, due_at = $12, version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING {PROJECT_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(project_id)
    .bind(name.trim())
    .bind(&description)
    .bind(&status)
    .bind(owner_user_id)
    .bind(customer_id)
    .bind(deal_id)
    .bind(&customer_public_id)
    .bind(&deal_public_id)
    .bind(starts_at)
    .bind(due_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.project.update",
        "project",
        &updated.public_id,
        serde_json::json!({ "status": status }),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(updated.into_dto()))
}

/// DELETE /api/v1/operations/projects/{id} — soft delete.
#[utoipa::path(delete, path = "/api/v1/operations/projects/{id}", tag = "operations-projects",
    responses((status = 204), (status = 404)))]
pub async fn delete_project(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let project_id = parse_public_id(IdKind::Project, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_project_delete(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_project_row(&mut tx, org_id, project_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "project"))?;
    enforce_project_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_project_delete(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    sqlx::query(
        r#"
        UPDATE operations_project
        SET deleted_at = now(), updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.project.delete",
        "project",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

// Re-export for board/summary helpers that may need project existence checks.
pub(crate) async fn project_exists(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    project_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM operations_project WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.is_some())
}
