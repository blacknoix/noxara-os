//! `/api/v1/operations/tasks/{id}/comments` — comments with authz-filtered mentions.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::tasks::{fetch_task_row, TaskRow};
use super::{internal, not_found, parse_public_id, user_public, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::mentions::{filter_mention_recipients, parse_mention_user_ids};
use crate::principal::{
    enforce_any_scope, enforce_scoped, load_membership_scope, required_scope_for_owner_row,
};
use crate::state::AppState;
use crate::types::{CreateCommentRequest, TaskCommentDto};
use companyos_ids::IdKind;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/operations/tasks/{id}/comments",
        get(list_comments).post(create_comment),
    )
}

#[derive(Debug, sqlx::FromRow)]
struct CommentRow {
    id: Uuid,
    author_user_id: Uuid,
    body: String,
    created_at: DateTime<Utc>,
}

async fn mentioned_for_comment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    comment_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT recipient_user_id
        FROM operations_notification_intent
        WHERE org_id = $1 AND resource_id = $2 AND kind = 'mention'
        ORDER BY created_at ASC
        "#,
    )
    .bind(org_id)
    .bind(comment_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(u,)| user_public(u)).collect())
}

fn comment_dto(row: CommentRow, mentioned_user_ids: Vec<String>) -> TaskCommentDto {
    TaskCommentDto {
        id: row.id.to_string(),
        author_user_id: user_public(row.author_user_id),
        body: row.body,
        created_at: row.created_at.to_rfc3339(),
        mentioned_user_ids,
    }
}

async fn enforce_task_perm(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    auth: &AuthCtx,
    membership: &crate::principal::MembershipScope,
    permission: companyos_authz::PermissionId,
    row: &TaskRow,
    request_id: &str,
) -> Result<(), AppError> {
    let required = required_scope_for_owner_row(
        tx,
        org_id,
        auth.ctx.actor.user_id,
        membership.team_id,
        membership.department_id,
        Some(row.owner_user_id),
    )
    .await
    .map_err(internal(request_id))?;
    enforce_scoped(&membership.principal, permission, required, request_id)
}

/// GET /api/v1/operations/tasks/{id}/comments
#[utoipa::path(get, path = "/api/v1/operations/tasks/{id}/comments", tag = "operations-comments",
    responses((status = 200, body = Vec<TaskCommentDto>)))]
pub async fn list_comments(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskCommentDto>>, AppError> {
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

    let task = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_perm(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_read(),
        &task,
        &request_id,
    )
    .await?;

    let rows: Vec<CommentRow> = sqlx::query_as(
        r#"
        SELECT id, author_user_id, body, created_at
        FROM operations_task_comment
        WHERE org_id = $1 AND task_id = $2 AND deleted_at IS NULL
        ORDER BY created_at ASC
        "#,
    )
    .bind(org_id)
    .bind(task_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mentioned = mentioned_for_comment(&mut tx, org_id, row.id)
            .await
            .map_err(internal(&request_id))?;
        out.push(comment_dto(row, mentioned));
    }
    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(out))
}

/// POST /api/v1/operations/tasks/{id}/comments
#[utoipa::path(post, path = "/api/v1/operations/tasks/{id}/comments", tag = "operations-comments",
    request_body = CreateCommentRequest,
    responses((status = 201, body = TaskCommentDto)))]
pub async fn create_comment(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateCommentRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let task_id = parse_public_id(IdKind::Task, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    if body.body.trim().is_empty() {
        return Err(validation(&request_id, "body must not be empty"));
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
        perms::operations_task_comment(),
        &request_id,
    )?;

    let candidates = parse_mention_user_ids(&body.body);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) = idempotency::get(&mut *tx, org_id, "task.comment", key)
            .await
            .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let task = fetch_task_row(&mut tx, org_id, task_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "task"))?;
    enforce_task_perm(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::operations_task_comment(),
        &task,
        &request_id,
    )
    .await?;

    // Filter mentions against read scope BEFORE inserting intents.
    let allowed = filter_mention_recipients(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        task.owner_user_id,
        task.assignee_id,
        &candidates,
        &request_id,
    )
    .await?;

    let comment_id = new_uuid_v7();
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO operations_task_comment (id, org_id, task_id, author_user_id, body)
        VALUES ($1,$2,$3,$4,$5)
        RETURNING created_at
        "#,
    )
    .bind(comment_id)
    .bind(org_id)
    .bind(task_id)
    .bind(auth.ctx.actor.user_id)
    .bind(body.body.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let preview: String = body.body.chars().take(200).collect();
    for recipient in &allowed {
        sqlx::query(
            r#"
            INSERT INTO operations_notification_intent (
                id, org_id, kind, resource_type, resource_id,
                recipient_user_id, actor_user_id, body_preview
            ) VALUES ($1,$2,'mention','task_comment',$3,$4,$5,$6)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(comment_id)
        .bind(recipient)
        .bind(auth.ctx.actor.user_id)
        .bind(&preview)
        .execute(&mut *tx)
        .await
        .map_err(internal(&request_id))?;
    }

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "operations.task.comment",
        "task",
        &task.public_id,
        serde_json::json!({
            "comment_id": comment_id.to_string(),
            "mentioned": allowed.iter().map(|u| user_public(*u)).collect::<Vec<_>>(),
        }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = TaskCommentDto {
        id: comment_id.to_string(),
        author_user_id: user_public(auth.ctx.actor.user_id),
        body: body.body.trim().to_string(),
        created_at: created_at.to_rfc3339(),
        mentioned_user_ids: allowed.iter().map(|u| user_public(*u)).collect(),
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "task.comment",
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
