//! In-app notification feed.

use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{FeedResponse, MessageResponse, NotificationItemDto};

#[utoipa::path(
    get,
    path = "/api/v1/notifications/feed",
    responses((status = 200, body = FeedResponse)),
    tag = "notifications"
)]
pub async fn feed(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<FeedResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_notification_read(), &request_id)?;
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, public_id, title, body, href, resource_type, resource_id, read_at, created_at
        FROM notification_item
        WHERE org_id = $1 AND user_id = $2
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let items = rows
        .into_iter()
        .map(
            |(
                _id,
                public_id,
                title,
                body,
                href,
                resource_type,
                resource_id,
                read_at,
                created_at,
            )| NotificationItemDto {
                id: public_id,
                title,
                body,
                href,
                resource_type,
                resource_id,
                read_at,
                created_at,
            },
        )
        .collect();

    Ok(Json(FeedResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/{id}/read",
    params(("id" = String, Path, description = "notification public id")),
    responses((status = 200, body = MessageResponse)),
    tag = "notifications"
)]
pub async fn mark_read(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_notification_read(), &request_id)?;
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let result = sqlx::query(
        r#"
        UPDATE notification_item SET read_at = now()
        WHERE org_id = $1 AND user_id = $2 AND public_id = $3 AND read_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    if result.rows_affected() == 0 {
        // Idempotent: already read or not found for this user.
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM notification_item WHERE org_id = $1 AND user_id = $2 AND public_id = $3",
        )
        .bind(org_id.as_uuid())
        .bind(user_id)
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        if exists.is_none() {
            return Err(AppError::new(
                ErrorCode::NotFound,
                &request_id,
                "notification not found",
            ));
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "marked read".into(),
    }))
}
