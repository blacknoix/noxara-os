//! Push device token registration (Phase 1.11).
//!
//! Stores tokens for later FCM/APNs delivery. Live push providers are **not**
//! wired in CI — clients use FakePushService.

use axum::extract::{Path, State};
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::set_session_org_id;

use companyos_authz::perms;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    DeviceDto, DeviceListResponse, MessageResponse, RegisterDeviceRequest, RegisterDeviceResponse,
};

fn validate_platform(platform: &str) -> bool {
    matches!(
        platform,
        "ios" | "android" | "web" | "desktop" | "fake" | "test"
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/devices",
    responses((status = 200, body = DeviceListResponse)),
    tag = "notifications"
)]
pub async fn list_devices(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<DeviceListResponse>, AppError> {
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

    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT public_id, platform, push_token, device_label
        FROM notification_device
        WHERE org_id = $1 AND user_id = $2
        ORDER BY updated_at DESC
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

    Ok(Json(DeviceListResponse {
        items: rows
            .into_iter()
            .map(|(id, platform, push_token, device_label)| DeviceDto {
                id,
                platform,
                push_token,
                device_label,
            })
            .collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/devices",
    request_body = RegisterDeviceRequest,
    responses((status = 201, body = RegisterDeviceResponse)),
    tag = "notifications"
)]
pub async fn register_device(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<RegisterDeviceRequest>,
) -> Result<(axum::http::StatusCode, Json<RegisterDeviceResponse>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id;
    let user_id = auth.ctx.actor.user_id;

    if !auth.local_bypass {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        // Self-service device registration uses the same gate as preference updates.
        enforce(&principal, perms::platform_notification_read(), &request_id)?;
    }

    let platform = body.platform.trim().to_ascii_lowercase();
    if !validate_platform(&platform) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "platform must be ios|android|web|desktop|fake|test",
        ));
    }
    let token = body.push_token.trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "push_token required (max 4096 chars)",
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    // Upsert on (org, user, token) — same token re-registers without duplicating.
    let existing: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id FROM notification_device
        WHERE org_id = $1 AND user_id = $2 AND push_token = $3
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(token)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let (public_id, created) = if let Some((id, public_id)) = existing {
        sqlx::query(
            r#"
            UPDATE notification_device
            SET platform = $1, device_label = $2, updated_at = now()
            WHERE id = $3
            "#,
        )
        .bind(&platform)
        .bind(&body.device_label)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        (public_id, false)
    } else {
        let id = new_uuid_v7();
        // Device tokens are not in IdKind catalogue — use ndev_ + uuid (no cross-context FK).
        let public_id = format!("ndev_{}", id.simple());
        sqlx::query(
            r#"
            INSERT INTO notification_device (
                id, org_id, user_id, public_id, platform, push_token, device_label
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(id)
        .bind(org_id.as_uuid())
        .bind(user_id)
        .bind(&public_id)
        .bind(&platform)
        .bind(token)
        .bind(&body.device_label)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        (public_id, true)
    };
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let status = if created {
        axum::http::StatusCode::CREATED
    } else {
        axum::http::StatusCode::OK
    };
    Ok((
        status,
        Json(RegisterDeviceResponse {
            id: public_id,
            platform,
            registered: true,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/notifications/devices/{id}",
    responses((status = 200, body = MessageResponse)),
    tag = "notifications"
)]
pub async fn unregister_device(
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

    let res = sqlx::query(
        r#"
        DELETE FROM notification_device
        WHERE org_id = $1 AND user_id = $2 AND public_id = $3
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    if res.rows_affected() == 0 {
        return Err(AppError::new(
            ErrorCode::NotFound,
            &request_id,
            "device not found",
        ));
    }

    Ok(Json(MessageResponse {
        message: "unregistered".into(),
    }))
}
