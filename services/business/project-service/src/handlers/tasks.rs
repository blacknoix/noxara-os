//! `/api/v1/operations/tasks` — task CRUD, board move, attachments.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::projects::project_exists;
use super::{
    conflict, internal, not_found, parse_public_id, parse_user_ref, require_if_match, user_public,
    validation, normalize_paging,
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
    ChecklistItemDto, CreateAttachmentRequest, CreateTaskRequest, ListQuery, MoveTaskRequest,
    TaskAttachmentDto, TaskDto, TaskListResponse, UpdateTaskRequest, TASK_PRIORITIES, TASK_STATUSES,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/operations/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/v1/operations/tasks/{id}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/api/v1/operations/tasks/{id}/move", axum::routing::post(move_task))
        .route(
            "/api/v1/operations/tasks/{id}/attachments",
            axum::routing::post(create_attachment),
        )
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct TaskRow {
    pub(crate) id: Uuid,
    pub(crate) public_id: String,
    pub(crate) project_id: Uuid,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) status: String,
    pub(crate) priority: String,
    pub(crate) owner_user_id: Uuid,
    pub(crate) assignee_id: Option<Uuid>,
    pub(crate) due_at: Option<DateTime<Utc>>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
    pub(crate) labels: Vec<String>,
    pub(crate) position: f64,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) version: i32,
}

pub(crate) const TASK_COLUMNS: &str = r#"
    id, public_id, project_id, title, description, status, priority,
    owner_user_id, assignee_id, due_at, completed_at, labels, position,
    created_at, updated_at, version
"#;

fn validate_status(status: &str, request_id: &str) -> Result<(), AppError> {
    if TASK_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(validation(
            request_id,
            format!("status must be one of: {}", TASK_STATUSES.join("|")),
        ))
    }
}

fn validate_priority(priority: &str, request_id: &str) -> Result<(), AppError> {
    if TASK_PRIORITIES.contains(&priority) {
        Ok(())
    } else {
        Err(validation(
            request_id,
            format!("priority must be one of: {}", TASK_PRIORITIES.join("|")),
        ))
    }
}

fn parse_due_at(raw: Option<&str>, request_id: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|_| validation(request_id, "due_at must be RFC3339")),
    }
}

async fn enforce_task_scope(
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

pub(crate) async fn fetch_task_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
) -> Result<Option<TaskRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {TASK_COLUMNS} FROM operations_task
         WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL"
    ))
    .bind(org_id)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_checklist(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
) -> Result<Vec<ChecklistItemDto>, sqlx::Error> {
    let rows: Vec<(Uuid, String, bool, i32)> = sqlx::query_as(
        r#"
        SELECT id, title, is_done, position
        FROM operations_task_checklist
        WHERE org_id = $1 AND task_id = $2 AND deleted_at IS NULL
        ORDER BY position ASC, created_at ASC
        "#,
    )
    .bind(org_id)
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, title, is_done, position)| ChecklistItemDto {
            id: id.to_string(),
            title,
            is_done,
            position,
        })
        .collect())
}

async fn load_attachments(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
) -> Result<Vec<TaskAttachmentDto>, sqlx::Error> {
    let rows: Vec<(Uuid, String, Option<String>, Option<i64>, String, DateTime<Utc>)> =
        sqlx::query_as(
            r#"
            SELECT id, file_name, content_type, byte_size, url, created_at
            FROM operations_task_attachment
            WHERE org_id = $1 AND task_id = $2 AND deleted_at IS NULL
            ORDER BY created_at ASC
            "#,
        )
        .bind(org_id)
        .bind(task_id)
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, file_name, content_type, byte_size, url, created_at)| TaskAttachmentDto {
                id: id.to_string(),
                file_name,
                content_type,
                byte_size,
                url,
                created_at: created_at.to_rfc3339(),
            },
        )
        .collect())
}

async fn load_blocked_by(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT t.public_id
        FROM operations_task_dependency d
        JOIN operations_task t ON t.id = d.blocked_by_task_id AND t.org_id = d.org_id
        WHERE d.org_id = $1 AND d.task_id = $2 AND d.deleted_at IS NULL
          AND t.deleted_at IS NULL
        ORDER BY t.public_id
        "#,
    )
    .bind(org_id)
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Load a full [`TaskDto`] including checklist, attachments, and blocked_by (`tsk_` ids).
pub(crate) async fn load_task_dto(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    row: TaskRow,
) -> Result<TaskDto, sqlx::Error> {
    let checklist = load_checklist(tx, org_id, row.id).await?;
    let attachments = load_attachments(tx, org_id, row.id).await?;
    let blocked_by = load_blocked_by(tx, org_id, row.id).await?;
    Ok(TaskDto {
        id: row.public_id,
        project_id: PublicId::new(IdKind::Project, row.project_id).as_str(),
        title: row.title,
        description: row.description,
        status: row.status,
        priority: row.priority,
        owner_user_id: user_public(row.owner_user_id),
        assignee_id: row.assignee_id.map(user_public),
        due_at: row.due_at.map(|t| t.to_rfc3339()),
        completed_at: row.completed_at.map(|t| t.to_rfc3339()),
        labels: row.labels,
        position: row.position,
        blocked_by,
        checklist,
        attachments,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        version: row.version,
    })
}

pub(crate) async fn fetch_task_dto_by_id(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
) -> Result<Option<TaskDto>, sqlx::Error> {
    match fetch_task_row(tx, org_id, task_id).await? {
        Some(row) => Ok(Some(load_task_dto(tx, org_id, row).await?)),
        None => Ok(None),
    }
}

async fn replace_blocked_by(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    task_id: Uuid,
    blocked_by: &[Uuid],
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE operations_task_dependency
        SET deleted_at = now()
        WHERE org_id = $1 AND task_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    for blocker in blocked_by {
        if *blocker == task_id {
            return Err(validation(request_id, "task cannot block itself"));
        }
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM operations_task WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .bind(blocker)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal(request_id))?;
        if exists.is_none() {
            return Err(not_found(request_id, "blocked_by task"));
        }
        sqlx::query(
            r#"
            INSERT INTO operations_task_dependency (id, org_id, task_id, blocked_by_task_id)
            VALUES ($1,$2,$3,$4)
            ON CONFLICT (org_id, task_id, blocked_by_task_id)
            DO UPDATE SET deleted_at = NULL
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(task_id)
        .bind(blocker)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }
    Ok(())
}

async fn emit_task_assigned(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    auth: &AuthCtx,
    task_public_id: &str,
    assignee_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "task",
        "assigned",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": task_public_id,
            "assignee_id": user_public(assignee_id),
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

async fn emit_task_completed(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    auth: &AuthCtx,
    task_public_id: &str,
    request_id: &str,
) -> Result<(), AppError> {
    let env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "task",
        "completed",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({ "id": task_public_id }),
    );
    companyos_outbox::insert_event(&mut **tx, &env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

/// GET /api/v1/operations/tasks
#[utoipa::path(get, path = "/api/v1/operations/tasks", tag = "operations-tasks",
    params(
        ("project_id" = Option<String>, Query),
        ("status" = Option<String>, Query),
        ("assignee_id" = Option<String>, Query),
        ("q" = Option<String>, Query),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = TaskListResponse)))]
pub async fn list_tasks(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<TaskListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_read(),
        &request_id,
    )?;
    let scope = scope_for_permission(&membership.principal, &perms::operations_task_read());
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let project_id = match q.project_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::Project, s, &request_id)?),
        None => None,
    };
    if let Some(status) = q.status.as_deref() {
        validate_status(status, &request_id)?;
    }
    let assignee_id = match q.assignee_id.as_deref() {
        Some(s) => Some(parse_user_ref(s, &request_id)?),
        None => None,
    };

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
        if let Some(pid) = project_id {
            qb.push(" AND project_id = ");
            qb.push_bind(pid);
        }
        if let Some(status) = q.status.as_deref() {
            qb.push(" AND status = ");
            qb.push_bind(status.to_string());
        }
        if let Some(aid) = assignee_id {
            qb.push(" AND assignee_id = ");
            qb.push_bind(aid);
        }
        if let Some(term) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND title ILIKE ");
            qb.push_bind(format!("%{term}%"));
        }
    };

    let mut count_qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM operations_task WHERE org_id = ");
    count_qb.push_bind(org_id);
    count_qb.push(" AND deleted_at IS NULL");
    build_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {TASK_COLUMNS} FROM operations_task WHERE org_id = "
    ));
    qb.push_bind(org_id);
    qb.push(" AND deleted_at IS NULL");
    build_filters(&mut qb);
    qb.push(" ORDER BY position ASC, created_at DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows: Vec<TaskRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(&request_id))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(
            load_task_dto(&mut tx, org_id, row)
                .await
                .map_err(internal(&request_id))?,
        );
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(TaskListResponse { items, total }))
}

/// POST /api/v1/operations/tasks
#[utoipa::path(post, path = "/api/v1/operations/tasks", tag = "operations-tasks",
    request_body = CreateTaskRequest,
    responses((status = 201, body = TaskDto)))]
pub async fn create_task(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Json(body): Json<CreateTaskRequest>,
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
        perms::operations_task_create(),
        &request_id,
    )?;

    if body.title.trim().is_empty() {
        return Err(validation(&request_id, "title must not be empty"));
    }
    let project_id = parse_public_id(IdKind::Project, &body.project_id, &request_id)?;
    let status = body.status.as_deref().unwrap_or("backlog");
    validate_status(status, &request_id)?;
    let priority = body.priority.as_deref().unwrap_or("medium");
    validate_priority(priority, &request_id)?;
    let due_at = parse_due_at(body.due_at.as_deref(), &request_id)?;
    let assignee_id = match body.assignee_id.as_deref() {
        Some(s) => Some(parse_user_ref(s, &request_id)?),
        None => None,
    };
    if assignee_id.is_some() {
        enforce_any_scope(
            &membership.principal,
            perms::operations_task_assign(),
            &request_id,
        )?;
    }
    let labels = body.labels.clone().unwrap_or_default();
    let blocked_by_ids: Vec<Uuid> = body
        .blocked_by
        .as_ref()
        .map(|ids| {
            ids.iter()
                .map(|s| parse_public_id(IdKind::Task, s, &request_id))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    let public_id = PublicId::generate(IdKind::Task);
    let id = public_id.uuid();
    let completed_at = if status == "done" {
        Some(Utc::now())
    } else {
        None
    };

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "task.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    if !project_exists(&mut tx, org_id, project_id)
        .await
        .map_err(internal(&request_id))?
    {
        return Err(not_found(&request_id, "project"));
    }

    sqlx::query(
        r#"
        INSERT INTO operations_task (
            id, org_id, public_id, project_id, title, description, status, priority,
            owner_user_id, assignee_id, due_at, completed_at, labels, position, board_column
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,0,$14)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(project_id)
    .bind(body.title.trim())
    .bind(&body.description)
    .bind(status)
    .bind(priority)
    .bind(auth.ctx.actor.user_id)
    .bind(assignee_id)
    .bind(due_at)
    .bind(completed_at)
    .bind(&labels)
    .bind(status)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some(items) = &body.checklist {
        for (i, title) in items.iter().enumerate() {
            if title.trim().is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO operations_task_checklist (id, org_id, task_id, title, position)
                VALUES ($1,$2,$3,$4,$5)
                "#,
            )
            .bind(new_uuid_v7())
            .bind(org_id)
            .bind(id)
            .bind(title.trim())
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
        }
    }

    if !blocked_by_ids.is_empty() {
        replace_blocked_by(&mut tx, org_id, id, &blocked_by_ids, &request_id).await?;
    }

    let created_env = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Operations,
        "task",
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "project_id": body.project_id,
            "title": body.title.trim(),
            "status": status,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &created_env)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(aid) = assignee_id {
        emit_task_assigned(&mut tx, &auth, &public_id.as_str(), aid, &request_id).await?;
    }
    if status == "done" {
        emit_task_completed(&mut tx, &auth, &public_id.as_str(), &request_id).await?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.create",
        "task",
        &public_id.as_str(),
        serde_json::json!({ "status": status, "project_id": body.project_id }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = fetch_task_dto_by_id(&mut tx, org_id, id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                request_id.clone(),
                "task missing after insert",
            )
        })?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "task.create",
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

/// GET /api/v1/operations/tasks/{id}
#[utoipa::path(get, path = "/api/v1/operations/tasks/{id}", tag = "operations-tasks",
    responses((status = 200, body = TaskDto), (status = 404)))]
pub async fn get_task(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<TaskDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_read(),
        row.owner_user_id,
        &request_id,
    )
    .await?;
    let dto = load_task_dto(&mut tx, org_id, row)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// PATCH /api/v1/operations/tasks/{id}
#[utoipa::path(patch, path = "/api/v1/operations/tasks/{id}", tag = "operations-tasks",
    request_body = UpdateTaskRequest,
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub async fn update_task(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;
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
        perms::operations_task_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_update(),
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

    let title = body.title.unwrap_or(row.title);
    if title.trim().is_empty() {
        return Err(validation(&request_id, "title must not be empty"));
    }
    let description = body.description.or(row.description);
    let status = body.status.unwrap_or_else(|| row.status.clone());
    validate_status(&status, &request_id)?;
    let priority = body.priority.unwrap_or_else(|| row.priority.clone());
    validate_priority(&priority, &request_id)?;
    let due_at = if body.due_at.is_some() {
        parse_due_at(body.due_at.as_deref(), &request_id)?
    } else {
        row.due_at
    };
    let labels = body.labels.unwrap_or(row.labels);
    let position = body.position.unwrap_or(row.position);

    let new_assignee = if body.assignee_id.is_some() {
        match body.assignee_id.as_deref() {
            Some(s) if !s.trim().is_empty() => Some(parse_user_ref(s, &request_id)?),
            _ => None,
        }
    } else {
        row.assignee_id
    };
    if new_assignee != row.assignee_id {
        enforce_any_scope(
            &membership.principal,
            perms::operations_task_assign(),
            &request_id,
        )?;
    }

    let becoming_done = status == "done" && row.status != "done";
    let completed_at = if status == "done" {
        if becoming_done {
            Some(Utc::now())
        } else {
            row.completed_at
        }
    } else {
        None
    };

    let updated: TaskRow = sqlx::query_as(&format!(
        r#"
        UPDATE operations_task SET
            title = $3, description = $4, status = $5, priority = $6,
            assignee_id = $7, due_at = $8, completed_at = $9, labels = $10,
            position = $11, board_column = $5,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING {TASK_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(task_id)
    .bind(title.trim())
    .bind(&description)
    .bind(&status)
    .bind(&priority)
    .bind(new_assignee)
    .bind(due_at)
    .bind(completed_at)
    .bind(&labels)
    .bind(position)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if let Some(blocked) = &body.blocked_by {
        let ids: Vec<Uuid> = blocked
            .iter()
            .map(|s| parse_public_id(IdKind::Task, s, &request_id))
            .collect::<Result<Vec<_>, _>>()?;
        replace_blocked_by(&mut tx, org_id, task_id, &ids, &request_id).await?;
    }

    if new_assignee != row.assignee_id {
        if let Some(aid) = new_assignee {
            emit_task_assigned(&mut tx, &auth, &updated.public_id, aid, &request_id).await?;
        }
    }
    if becoming_done {
        emit_task_completed(&mut tx, &auth, &updated.public_id, &request_id).await?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.update",
        "task",
        &updated.public_id,
        serde_json::json!({ "status": status }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = load_task_dto(&mut tx, org_id, updated)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto))
}

/// DELETE /api/v1/operations/tasks/{id} — soft delete.
#[utoipa::path(delete, path = "/api/v1/operations/tasks/{id}", tag = "operations-tasks",
    responses((status = 204), (status = 404)))]
pub async fn delete_task(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_delete(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let row = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_delete(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    sqlx::query(
        r#"
        UPDATE operations_task
        SET deleted_at = now(), updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.delete",
        "task",
        &row.public_id,
        serde_json::json!({}),
    )
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/operations/tasks/{id}/move — board drag (status + optional position).
#[utoipa::path(post, path = "/api/v1/operations/tasks/{id}/move", tag = "operations-tasks",
    request_body = MoveTaskRequest,
    responses((status = 200, body = TaskDto), (status = 409)))]
pub async fn move_task(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<MoveTaskRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;
    let expected_version = require_if_match(&headers, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    validate_status(&body.status, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "task.move", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::OK);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let row = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_update(),
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

    let becoming_done = body.status == "done" && row.status != "done";
    let completed_at = if body.status == "done" {
        if becoming_done {
            Some(Utc::now())
        } else {
            row.completed_at
        }
    } else {
        None
    };
    let position = body.position.unwrap_or(row.position);

    let updated: TaskRow = sqlx::query_as(&format!(
        r#"
        UPDATE operations_task SET
            status = $3, board_column = $3, position = $4, completed_at = $5,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING {TASK_COLUMNS}
        "#
    ))
    .bind(org_id)
    .bind(task_id)
    .bind(&body.status)
    .bind(position)
    .bind(completed_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    if becoming_done {
        emit_task_completed(&mut tx, &auth, &updated.public_id, &request_id).await?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.move",
        "task",
        &updated.public_id,
        serde_json::json!({ "status": body.status, "position": position }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = load_task_dto(&mut tx, org_id, updated)
        .await
        .map_err(internal(&request_id))?;

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "task.move",
            key,
            200,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(dto).into_response())
}

/// POST /api/v1/operations/tasks/{id}/attachments — URL metadata only.
#[utoipa::path(post, path = "/api/v1/operations/tasks/{id}/attachments", tag = "operations-tasks",
    request_body = CreateAttachmentRequest,
    responses((status = 201, body = TaskAttachmentDto)))]
pub async fn create_attachment(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateAttachmentRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    if body.file_name.trim().is_empty() {
        return Err(validation(&request_id, "file_name must not be empty"));
    }
    if body.url.trim().is_empty() {
        return Err(validation(&request_id, "url must not be empty"));
    }

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_update(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "task.attachment", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let row = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_update(),
        row.owner_user_id,
        &request_id,
    )
    .await?;

    let att_id = new_uuid_v7();
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO operations_task_attachment (
            id, org_id, task_id, uploaded_by, file_name, content_type, byte_size, url
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING created_at
        "#,
    )
    .bind(att_id)
    .bind(org_id)
    .bind(task_id)
    .bind(auth.ctx.actor.user_id)
    .bind(body.file_name.trim())
    .bind(&body.content_type)
    .bind(body.byte_size)
    .bind(body.url.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.attachment",
        "task",
        &row.public_id,
        serde_json::json!({ "attachment_id": att_id.to_string(), "file_name": body.file_name }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = TaskAttachmentDto {
        id: att_id.to_string(),
        file_name: body.file_name.trim().to_string(),
        content_type: body.content_type,
        byte_size: body.byte_size,
        url: body.url.trim().to_string(),
        created_at: created_at.to_rfc3339(),
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "task.attachment",
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
