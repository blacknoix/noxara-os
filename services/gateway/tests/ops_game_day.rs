//! TRD 8.2 game day — Redis down → rate-limit falls back to in-memory.
//! Authz remains per-request (Postgres PDP); idempotency is DB-backed elsewhere.

use companyos_gateway::api_key_auth::{check_rate_limit, ApiKeyRateLimiter};

#[tokio::test]
async fn redis_down_rate_limit_falls_back_to_memory() {
    let memory = ApiKeyRateLimiter::new();
    let info = check_rate_limit(&memory, Some("redis://127.0.0.1:9"), "gameday-key", 100)
        .await
        .expect("in-memory fallback must allow");
    assert!(info.limit >= 1);
    assert!(info.remaining <= info.limit);
    let _ = check_rate_limit(&memory, Some("redis://127.0.0.1:9"), "gameday-key", 100)
        .await
        .expect("second hit still works without Redis");
}
