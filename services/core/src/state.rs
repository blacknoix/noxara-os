//! Shared application state for companyos-core.

use companyos_auth_token::KeyRing;
use companyos_authz::PermissionSetCache;
use sqlx::PgPool;
use std::sync::Arc;

use crate::auth::rate_limit::RateLimiter;
use crate::auth::tokens::AuthKeys;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_keys: AuthKeys,
    pub rate_limiter: Arc<RateLimiter>,
    pub perm_cache: Arc<PermissionSetCache>,
    pub webhook_crypto: crate::webhook_crypto::WebhookEncryptor,
}

impl AppState {
    pub fn new(pool: PgPool, ring: KeyRing) -> Self {
        Self {
            pool,
            auth_keys: AuthKeys::new(ring),
            rate_limiter: Arc::new(RateLimiter::auth_strict()),
            perm_cache: Arc::new(PermissionSetCache::default_5s()),
            webhook_crypto: crate::webhook_crypto::WebhookEncryptor::from_env()
                .expect("WEBHOOK_ENCRYPTION_KEY or local-dev fallback"),
        }
    }
}
