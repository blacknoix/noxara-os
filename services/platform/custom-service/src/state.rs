use companyos_auth_token::KeyRing;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: Arc<KeyRing>,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        Self {
            pool,
            keyring: Arc::new(keyring),
        }
    }
}
