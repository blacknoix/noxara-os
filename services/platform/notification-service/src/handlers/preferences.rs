//! Notification channel preferences.

use axum::extract::State;
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;

use companyos_authz::perms;

use crate::auth::AuthCtx;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{PreferenceDto, PreferencesResponse, PutPreferencesRequest};

#[utoipa::path(
    get,
    path = "/api/v1/notifications/preferences",
    responses((status = 200, body = PreferencesResponse)),
    tag = "notifications"
)]
pub async fn get_preferences(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<PreferencesResponse>, AppError> {
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
        String,
        bool,
        Option<chrono::NaiveTime>,
        Option<chrono::NaiveTime>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT channel, enabled,
               quiet_hours_start::time,
               quiet_hours_end::time,
               digest_cron
        FROM notification_preference
        WHERE org_id = $1 AND user_id = $2
        ORDER BY channel
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

    let preferences = if rows.is_empty() {
        // Defaults: in_app + email enabled.
        vec![
            PreferenceDto {
                channel: "in_app".into(),
                enabled: true,
                quiet_hours_start: None,
                quiet_hours_end: None,
                digest_cron: None,
            },
            PreferenceDto {
                channel: "email".into(),
                enabled: true,
                quiet_hours_start: None,
                quiet_hours_end: None,
                digest_cron: Some("0 8 * * *".into()),
            },
        ]
    } else {
        rows.into_iter()
            .map(|(channel, enabled, qhs, qhe, digest_cron)| PreferenceDto {
                channel,
                enabled,
                quiet_hours_start: qhs.map(|t| t.format("%H:%M:%S").to_string()),
                quiet_hours_end: qhe.map(|t| t.format("%H:%M:%S").to_string()),
                digest_cron,
            })
            .collect()
    };

    Ok(Json(PreferencesResponse { preferences }))
}

#[utoipa::path(
    put,
    path = "/api/v1/notifications/preferences",
    request_body = PutPreferencesRequest,
    responses((status = 200, body = PreferencesResponse)),
    tag = "notifications"
)]
pub async fn put_preferences(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<PutPreferencesRequest>,
) -> Result<Json<PreferencesResponse>, AppError> {
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

    for pref in &body.preferences {
        sqlx::query(
            r#"
            INSERT INTO notification_preference
                (org_id, user_id, channel, enabled, quiet_hours_start, quiet_hours_end, digest_cron)
            VALUES ($1, $2, $3, $4, $5::timetz, $6::timetz, $7)
            ON CONFLICT (org_id, user_id, channel) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                quiet_hours_start = EXCLUDED.quiet_hours_start,
                quiet_hours_end = EXCLUDED.quiet_hours_end,
                digest_cron = EXCLUDED.digest_cron
            "#,
        )
        .bind(org_id.as_uuid())
        .bind(user_id)
        .bind(&pref.channel)
        .bind(pref.enabled)
        .bind(pref.quiet_hours_start.as_deref())
        .bind(pref.quiet_hours_end.as_deref())
        .bind(pref.digest_cron.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    get_preferences(State(state), auth).await
}
