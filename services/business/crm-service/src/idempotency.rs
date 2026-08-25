//! `Idempotency-Key` support for POSTs that create deals/quotes/lead-conversions.
//!
//! Backed by `sales_idempotency (org_id, scope, key)`. Callers store the
//! successful response body + status so a retried request with the same key
//! replays the original response instead of creating a duplicate row.

use axum::http::HeaderMap;
use uuid::Uuid;

pub fn header_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Look up a previously stored response for (org, scope, key). Must run with
/// the session `app.org_id` already bound for RLS.
pub async fn get<'e, E>(
    executor: E,
    org_id: Uuid,
    scope: &str,
    key: &str,
) -> Result<Option<(i32, serde_json::Value)>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as(
        r#"
        SELECT response_status, response_body
        FROM sales_idempotency
        WHERE org_id = $1 AND scope = $2 AND key = $3
        "#,
    )
    .bind(org_id)
    .bind(scope)
    .bind(key)
    .fetch_optional(executor)
    .await
}

/// Store the response for (org, scope, key). Must run in the same transaction
/// as the domain write it guards.
pub async fn put<'e, E>(
    executor: E,
    org_id: Uuid,
    scope: &str,
    key: &str,
    status: i32,
    body: serde_json::Value,
) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        r#"
        INSERT INTO sales_idempotency (id, org_id, scope, key, response_status, response_body)
        VALUES ($1,$2,$3,$4,$5,$6)
        ON CONFLICT (org_id, scope, key) DO NOTHING
        "#,
    )
    .bind(companyos_ids::new_uuid_v7())
    .bind(org_id)
    .bind(scope)
    .bind(key)
    .bind(status)
    .bind(body)
    .execute(executor)
    .await?;
    Ok(())
}
