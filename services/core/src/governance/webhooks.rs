//! Outbound webhook endpoint + delivery log domain logic (Phase 3.3).

use chrono::{DateTime, Utc};
use companyos_auth_token::generate_opaque_token;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::types::{WebhookDeliveryView, WebhookEndpointView};
use super::{internal, not_found, tenancy_internal, validation};
use crate::webhook_crypto::WebhookEncryptor;

const SECRET_PREFIX_LEN: usize = 8;
const AUTO_PAUSE_FAILURES: i32 = 10;
const RESPONSE_BODY_MAX: usize = 2048;

type EndpointRow = (
    String,
    String,
    String,
    Value,
    String,
    String,
    i32,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
);

type RotateEndpointRow = (
    Uuid,
    String,
    String,
    Value,
    String,
    i32,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

type DeliveryListRow = (
    String,
    String,
    String,
    i32,
    String,
    Option<i32>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

pub async fn list_endpoints(
    pool: &PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<WebhookEndpointView>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let rows: Vec<EndpointRow> = sqlx::query_as(
        r#"
        SELECT public_id, url, description, event_types, secret_prefix, status,
               failure_count, last_delivery_at, created_at, disabled_at, disabled_reason
        FROM webhook_endpoint
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(rows.into_iter().map(row_to_view).collect())
}

fn row_to_view(r: EndpointRow) -> WebhookEndpointView {
    let (
        id,
        url,
        description,
        event_types,
        secret_prefix,
        status,
        failure_count,
        last_delivery_at,
        created_at,
        disabled_at,
        disabled_reason,
    ) = r;
    WebhookEndpointView {
        id,
        url,
        description,
        event_types: serde_json::from_value(event_types).unwrap_or_default(),
        secret_prefix,
        status,
        failure_count,
        last_delivery_at: last_delivery_at.map(|d| d.to_rfc3339()),
        created_at: created_at.to_rfc3339(),
        disabled_at: disabled_at.map(|d| d.to_rfc3339()),
        disabled_reason,
    }
}

pub async fn create_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    created_by: Uuid,
    url: &str,
    description: &str,
    event_types: &[String],
    encryptor: &WebhookEncryptor,
    request_id: &str,
) -> Result<(WebhookEndpointView, String), AppError> {
    validate_url_shape(url, request_id)?;
    if event_types.is_empty() {
        return Err(validation(
            request_id,
            "at least one event_type is required",
        ));
    }

    let secret = format!("whsec_{}", generate_opaque_token());
    let ciphertext = encryptor.encrypt(secret.as_bytes()).map_err(|e| {
        AppError::new(
            companyos_errors::ErrorCode::Internal,
            request_id,
            e.to_string(),
        )
    })?;
    let secret_prefix: String = secret.chars().take(SECRET_PREFIX_LEN).collect();
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::WebhookEndpoint, id);
    let events_json = serde_json::to_value(event_types).unwrap_or_default();

    let (created_at,): (DateTime<Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO webhook_endpoint (
            id, org_id, public_id, url, description, event_types,
            secret_ciphertext, secret_prefix, status, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'active',$9)
        RETURNING created_at
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(url)
    .bind(description)
    .bind(&events_json)
    .bind(&ciphertext)
    .bind(&secret_prefix)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok((
        WebhookEndpointView {
            id: public_id.as_str(),
            url: url.to_string(),
            description: description.to_string(),
            event_types: event_types.to_vec(),
            secret_prefix,
            status: "active".into(),
            failure_count: 0,
            last_delivery_at: None,
            created_at: created_at.to_rfc3339(),
            disabled_at: None,
            disabled_reason: None,
        },
        secret,
    ))
}

pub async fn rotate_secret(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    endpoint_public_id: &str,
    encryptor: &WebhookEncryptor,
    request_id: &str,
) -> Result<(WebhookEndpointView, String), AppError> {
    let existing: Option<RotateEndpointRow> =
        sqlx::query_as(
            r#"
            SELECT id, url, description, event_types, status, failure_count, last_delivery_at, created_at
            FROM webhook_endpoint
            WHERE org_id = $1 AND public_id = $2
            "#,
        )
        .bind(org_id.as_uuid())
        .bind(endpoint_public_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal(request_id))?;

    let Some((
        id,
        url,
        description,
        event_types,
        status,
        failure_count,
        last_delivery_at,
        created_at,
    )) = existing
    else {
        return Err(not_found(request_id, "webhook endpoint"));
    };

    let secret = format!("whsec_{}", generate_opaque_token());
    let ciphertext = encryptor.encrypt(secret.as_bytes()).map_err(|e| {
        AppError::new(
            companyos_errors::ErrorCode::Internal,
            request_id,
            e.to_string(),
        )
    })?;
    let secret_prefix: String = secret.chars().take(SECRET_PREFIX_LEN).collect();

    sqlx::query(
        r#"
        UPDATE webhook_endpoint
        SET secret_ciphertext = $1, secret_prefix = $2, updated_at = now(),
            status = CASE WHEN status = 'paused' THEN 'active' ELSE status END,
            failure_count = CASE WHEN status = 'paused' THEN 0 ELSE failure_count END
        WHERE id = $3
        "#,
    )
    .bind(&ciphertext)
    .bind(&secret_prefix)
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok((
        WebhookEndpointView {
            id: endpoint_public_id.to_string(),
            url,
            description,
            event_types: serde_json::from_value(event_types).unwrap_or_default(),
            secret_prefix,
            status: if status == "paused" {
                "active".into()
            } else {
                status.clone()
            },
            failure_count: if status == "paused" { 0 } else { failure_count },
            last_delivery_at: last_delivery_at.map(|d| d.to_rfc3339()),
            created_at: created_at.to_rfc3339(),
            disabled_at: None,
            disabled_reason: None,
        },
        secret,
    ))
}

pub async fn disable_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    endpoint_public_id: &str,
    reason: &str,
    request_id: &str,
) -> Result<WebhookEndpointView, AppError> {
    let updated: Option<EndpointRow> = sqlx::query_as(
        r#"
        UPDATE webhook_endpoint
        SET status = 'disabled', disabled_at = now(), disabled_reason = $3, updated_at = now()
        WHERE org_id = $1 AND public_id = $2
        RETURNING public_id, url, description, event_types, secret_prefix, status,
                  failure_count, last_delivery_at, created_at, disabled_at, disabled_reason
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(endpoint_public_id)
    .bind(reason)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    updated
        .map(row_to_view)
        .ok_or_else(|| not_found(request_id, "webhook endpoint"))
}

pub async fn list_deliveries(
    pool: &PgPool,
    org_id: OrgId,
    endpoint_public_id: &str,
    request_id: &str,
) -> Result<Vec<WebhookDeliveryView>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let endpoint_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM webhook_endpoint WHERE org_id = $1 AND public_id = $2")
            .bind(org_id.as_uuid())
            .bind(endpoint_public_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(request_id))?;
    let Some((endpoint_id,)) = endpoint_id else {
        return Err(not_found(request_id, "webhook endpoint"));
    };

    let rows: Vec<DeliveryListRow> = sqlx::query_as(
        r#"
        SELECT public_id, event_subject, event_type, attempt, status, status_code,
               response_body, delivered_at, next_retry_at, created_at
        FROM webhook_delivery
        WHERE org_id = $1 AND endpoint_id = $2
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(endpoint_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                event_subject,
                event_type,
                attempt,
                status,
                status_code,
                response_body,
                delivered_at,
                next_retry_at,
                created_at,
            )| WebhookDeliveryView {
                id,
                endpoint_id: endpoint_public_id.to_string(),
                event_subject,
                event_type,
                attempt,
                status,
                status_code,
                response_body,
                delivered_at: delivered_at.map(|d| d.to_rfc3339()),
                next_retry_at: next_retry_at.map(|d| d.to_rfc3339()),
                created_at: created_at.to_rfc3339(),
            },
        )
        .collect())
}

/// Re-enqueue a logged delivery (replay tool). Creates a new pending attempt
/// with a fresh delivery public id but the same event_id (receivers dedupe).
pub async fn replay_delivery(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    delivery_public_id: &str,
    request_id: &str,
) -> Result<WebhookDeliveryView, AppError> {
    let existing: Option<(Uuid, Uuid, Uuid, String, String, Value)> = sqlx::query_as(
        r#"
        SELECT id, endpoint_id, event_id, event_subject, event_type, payload
        FROM webhook_delivery
        WHERE org_id = $1 AND public_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(delivery_public_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let Some((_old_id, endpoint_id, event_id, event_subject, event_type, payload)) = existing
    else {
        return Err(not_found(request_id, "webhook delivery"));
    };

    let endpoint_pub: Option<(String,)> =
        sqlx::query_as("SELECT public_id FROM webhook_endpoint WHERE id = $1 AND org_id = $2")
            .bind(endpoint_id)
            .bind(org_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal(request_id))?;
    let endpoint_public = endpoint_pub
        .map(|(p,)| p)
        .ok_or_else(|| not_found(request_id, "webhook endpoint"))?;

    // Reset the existing row to pending for replay (same event_id for receiver dedupe).
    let (public_id, attempt, created_at): (String, i32, DateTime<Utc>) = sqlx::query_as(
        r#"
        UPDATE webhook_delivery
        SET status = 'pending', attempt = attempt + 1, next_retry_at = now(),
            status_code = NULL, response_body = NULL, last_error = NULL,
            delivered_at = NULL, updated_at = now()
        WHERE org_id = $1 AND endpoint_id = $2 AND event_id = $3
        RETURNING public_id, attempt, created_at
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(endpoint_id)
    .bind(event_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let _ = (event_subject.clone(), event_type.clone(), payload);

    Ok(WebhookDeliveryView {
        id: public_id,
        endpoint_id: endpoint_public,
        event_subject,
        event_type,
        attempt,
        status: "pending".into(),
        status_code: None,
        response_body: None,
        delivered_at: None,
        next_retry_at: Some(Utc::now().to_rfc3339()),
        created_at: created_at.to_rfc3339(),
    })
}

/// Shape-only URL check for admin create (full SSRF is enforced at dispatch).
pub fn validate_url_shape(url: &str, request_id: &str) -> Result<(), AppError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| validation(request_id, "url must be an absolute http(s) URL"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(validation(request_id, "url scheme must be http or https"));
    }
    if parsed.host_str().is_none() {
        return Err(validation(request_id, "url must include a host"));
    }
    Ok(())
}

pub fn truncate_response(body: &str) -> String {
    if body.len() <= RESPONSE_BODY_MAX {
        body.to_string()
    } else {
        format!("{}…", &body[..RESPONSE_BODY_MAX])
    }
}

pub fn auto_pause_threshold() -> i32 {
    AUTO_PAUSE_FAILURES
}
