//! Internal event ingest → fan-out notifications with authz + prefs.

use axum::extract::State;
use axum::Json;
use chrono::{NaiveTime, Timelike, Utc};
use companyos_events::EventEnvelope;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{set_session_org_id, OrgId};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::mail;
use crate::mapping::{render_in_app, required_permission_for_event, resource_refs};
use crate::principal::{can_receive, load_principal};
use crate::state::AppState;
use crate::types::IngestResponse;

async fn set_ingest_session(tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
    sqlx::query("SELECT set_config('app.notification_ingest', '1', true)")
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "ingest", e.to_string()))?;
    Ok(())
}

fn in_quiet_hours(now: NaiveTime, start: Option<NaiveTime>, end: Option<NaiveTime>) -> bool {
    match (start, end) {
        (Some(s), Some(e)) if s != e => {
            if s < e {
                now >= s && now < e
            } else {
                // Wraps midnight.
                now >= s || now < e
            }
        }
        _ => false,
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/internal/ingest",
    responses((status = 200, body = IngestResponse)),
    tag = "notifications-internal"
)]
pub async fn ingest(
    State(state): State<AppState>,
    Json(envelope): Json<EventEnvelope>,
) -> Result<Json<IngestResponse>, AppError> {
    let request_id = envelope.event_id.to_string();
    let org_id = envelope.org_id;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_ingest_session(&mut tx).await?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    // Idempotency: skip if already processed.
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT idempotency_key FROM notification_processed WHERE idempotency_key = $1")
            .bind(&envelope.idempotency_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    if existing.is_some() {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        return Ok(Json(IngestResponse {
            accepted: true,
            duplicate: true,
            notified: 0,
            skipped: 0,
        }));
    }

    sqlx::query(
        "INSERT INTO notification_processed (idempotency_key, org_id, processed_at) VALUES ($1, $2, now())",
    )
    .bind(&envelope.idempotency_key)
    .bind(org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let Some(required_perm) = required_permission_for_event(&envelope) else {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
        return Ok(Json(IngestResponse {
            accepted: true,
            duplicate: false,
            notified: 0,
            skipped: 0,
        }));
    };

    // Candidate recipients: active memberships in the org.
    let members: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT user_id FROM membership
        WHERE org_id = $1 AND revoked_at IS NULL AND status = 'active'
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let (title, body, href) = render_in_app(&envelope);
    let (resource_type, resource_id) = resource_refs(&envelope);

    let mut notified = 0u32;
    let mut skipped = 0u32;

    for (user_id,) in members {
        // Skip the actor who caused the event (no self-notify for create).
        if user_id == envelope.actor.on_behalf_of {
            skipped += 1;
            continue;
        }

        let (principal, _, _) =
            match load_principal(&state.pool, org_id, user_id, &request_id).await {
                Ok(p) => p,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };

        if !can_receive(&principal, required_perm.clone()) {
            skipped += 1;
            continue;
        }

        match deliver_to_user(
            &state,
            org_id,
            user_id,
            &title,
            &body,
            href.as_deref(),
            resource_type.as_deref(),
            resource_id.as_deref(),
            &request_id,
        )
        .await
        {
            Ok(true) => notified += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                tracing::warn!(error = %e.detail, %user_id, "notification deliver failed");
                skipped += 1;
            }
        }
    }

    Ok(Json(IngestResponse {
        accepted: true,
        duplicate: false,
        notified,
        skipped,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn deliver_to_user(
    state: &AppState,
    org_id: OrgId,
    user_id: Uuid,
    title: &str,
    body: &str,
    href: Option<&str>,
    resource_type: Option<&str>,
    resource_id: Option<&str>,
    request_id: &str,
) -> Result<bool, AppError> {
    let pool = &state.pool;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_ingest_session(&mut tx).await?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let prefs: Vec<(String, bool, Option<NaiveTime>, Option<NaiveTime>)> = sqlx::query_as(
        r#"
        SELECT channel, enabled,
               quiet_hours_start::time,
               quiet_hours_end::time
        FROM notification_preference
        WHERE org_id = $1 AND user_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let in_app_enabled = prefs
        .iter()
        .find(|(c, _, _, _)| c == "in_app")
        .map(|(_, e, _, _)| *e)
        .unwrap_or(true);
    let email_pref = prefs.iter().find(|(c, _, _, _)| c == "email");
    let email_enabled = email_pref.map(|(_, e, _, _)| *e).unwrap_or(true);
    let (qhs, qhe) = email_pref
        .map(|(_, _, s, e)| (*s, *e))
        .unwrap_or((None, None));

    if !in_app_enabled && !email_enabled {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return Ok(false);
    }

    let item_id = new_uuid_v7();
    let public_id = format!("ntf_{}", item_id.simple());

    sqlx::query(
        r#"
        INSERT INTO notification_item
            (id, org_id, user_id, public_id, title, body, href, resource_type, resource_id, created_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,now())
        "#,
    )
    .bind(item_id)
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(&public_id)
    .bind(title)
    .bind(body)
    .bind(href)
    .bind(resource_type)
    .bind(resource_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if in_app_enabled {
        let delivery_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT INTO notification_delivery (id, org_id, item_id, channel, status, created_at)
            VALUES ($1,$2,$3,'in_app','delivered',now())
            "#,
        )
        .bind(delivery_id)
        .bind(org_id.as_uuid())
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    }

    if email_enabled {
        let now = Utc::now().time();
        // NaiveTime from Utc::now().time() loses timezone; compare clock parts.
        let now_naive =
            NaiveTime::from_hms_opt(now.hour(), now.minute(), now.second()).unwrap_or(now);
        let defer = in_quiet_hours(now_naive, qhs, qhe);
        let status = if defer { "deferred_digest" } else { "sent" };
        let delivery_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT INTO notification_delivery (id, org_id, item_id, channel, status, created_at)
            VALUES ($1,$2,$3,'email',$4,now())
            "#,
        )
        .bind(delivery_id)
        .bind(org_id.as_uuid())
        .bind(item_id)
        .bind(status)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

        if !defer {
            // Resolve email from user_identity when available.
            let email: Option<(String,)> =
                sqlx::query_as("SELECT email FROM user_identity WHERE id = $1")
                    .bind(user_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .ok()
                    .flatten();
            if let Some((to,)) = email {
                let _ = mail::send_email(&to, title, body).await;
            } else {
                let _ = mail::send_email(&format!("{user_id}@local"), title, body).await;
            }
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    if in_app_enabled {
        let payload = serde_json::json!({
            "id": public_id,
            "title": title,
            "body": body,
            "href": href,
            "resource_type": resource_type,
            "resource_id": resource_id,
        });
        crate::state::publish_notification_event(
            state.redis_url.as_deref(),
            &org_id.to_public().as_str(),
            user_id,
            &payload,
        )
        .await;
    }

    Ok(true)
}
