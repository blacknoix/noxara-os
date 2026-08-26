//! `/api/v1/operations/my-work` — assignee-centric inbox + recent mentions.
//!
//! Access path for assigned tasks MUST lead with `org_id` then `assignee_id`
//! so Postgres can use `operations_task_my_work_idx`. CI asserts that index
//! exists rather than loading 50k rows to prove the plan.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::tasks::{load_task_dto, TaskRow, TASK_COLUMNS};
use super::{internal, normalize_paging, user_public};
use crate::auth::AuthCtx;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{ListQuery, MyWorkResponse, TaskCommentDto};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/operations/my-work", get(my_work))
}

/// GET /api/v1/operations/my-work
#[utoipa::path(get, path = "/api/v1/operations/my-work", tag = "operations-my-work",
    params(
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = 200, body = MyWorkResponse)))]
pub async fn my_work(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<MyWorkResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let actor = auth.ctx.actor.user_id;
    let (limit, offset) = normalize_paging(q.limit, q.offset);

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, actor, &request_id).await?;
    enforce_any_scope(
        &membership.principal,
        perms::operations_task_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    // Index-friendly predicates: org_id AND assignee_id first.
    let total_assigned: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM operations_task
        WHERE org_id = $1 AND assignee_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let rows: Vec<TaskRow> = sqlx::query_as(&format!(
        r#"
        SELECT {TASK_COLUMNS} FROM operations_task
        WHERE org_id = $1 AND assignee_id = $2 AND deleted_at IS NULL
        ORDER BY due_at NULLS LAST, position ASC, created_at DESC
        LIMIT $3 OFFSET $4
        "#
    ))
    .bind(org_id)
    .bind(actor)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut assigned = Vec::with_capacity(rows.len());
    for row in rows {
        assigned.push(
            load_task_dto(&mut tx, org_id, row)
                .await
                .map_err(internal(&request_id))?,
        );
    }

    // Recent mention intents for the current user as comment-lite DTOs.
    #[derive(sqlx::FromRow)]
    struct MentionRow {
        comment_id: Uuid,
        author_user_id: Uuid,
        body: String,
        created_at: DateTime<Utc>,
    }

    let mention_rows: Vec<MentionRow> = sqlx::query_as(
        r#"
        SELECT c.id AS comment_id, c.author_user_id, c.body, c.created_at
        FROM operations_notification_intent ni
        JOIN operations_task_comment c
          ON c.id = ni.resource_id AND c.org_id = ni.org_id
        WHERE ni.org_id = $1
          AND ni.recipient_user_id = $2
          AND ni.kind = 'mention'
          AND c.deleted_at IS NULL
        ORDER BY ni.created_at DESC
        LIMIT 50
        "#,
    )
    .bind(org_id)
    .bind(actor)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let me = user_public(actor);
    let mentions: Vec<TaskCommentDto> = mention_rows
        .into_iter()
        .map(|r| TaskCommentDto {
            id: r.comment_id.to_string(),
            author_user_id: user_public(r.author_user_id),
            body: r.body,
            created_at: r.created_at.to_rfc3339(),
            mentioned_user_ids: vec![me.clone()],
        })
        .collect();

    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(MyWorkResponse {
        assigned,
        mentions,
        total_assigned,
    }))
}
