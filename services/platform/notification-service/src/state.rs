//! Shared application state for companyos-notification.

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    /// Optional Redis URL for in-app SSE fan-out (`companyos:notifications:{org}:{user}`).
    pub redis_url: Option<String>,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        Self {
            pool,
            keyring,
            redis_url: std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty()),
        }
    }
}

/// Publish a notification payload to the per-user Redis channel when configured.
pub async fn publish_notification_event(
    redis_url: Option<&str>,
    org_public_id: &str,
    user_id: uuid::Uuid,
    payload: &serde_json::Value,
) {
    let Some(url) = redis_url else {
        return;
    };
    let channel = format!("companyos:notifications:{org_public_id}:{user_id}");
    let Ok(client) = redis::Client::open(url) else {
        tracing::warn!(%channel, "invalid REDIS_URL; skip publish");
        return;
    };
    let Ok(mut conn) = client.get_multiplexed_async_connection().await else {
        tracing::warn!(%channel, "redis connect failed; skip publish");
        return;
    };
    let body = payload.to_string();
    let result: Result<(), _> = redis::cmd("PUBLISH")
        .arg(&channel)
        .arg(&body)
        .query_async(&mut conn)
        .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, %channel, "redis PUBLISH failed");
    }
}
