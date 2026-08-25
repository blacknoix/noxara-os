//! Shared application state for companyos-core.

use companyos_auth_token::KeyRing;
use sqlx::PgPool;
use std::sync::Arc;

use crate::auth::rate_limit::RateLimiter;
use crate::auth::tokens::AuthKeys;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_keys: AuthKeys,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn new(pool: PgPool, ring: KeyRing) -> Self {
        Self {
            pool,
            auth_keys: AuthKeys::new(ring),
            rate_limiter: Arc::new(RateLimiter::auth_strict()),
        }
    }
}
