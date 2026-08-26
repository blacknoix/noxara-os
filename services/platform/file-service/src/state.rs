//! File service state.

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    pub minio_endpoint: Option<String>,
    pub bucket: String,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        Self {
            pool,
            keyring,
            minio_endpoint: std::env::var("MINIO_ENDPOINT").ok().filter(|s| !s.is_empty()),
            bucket: std::env::var("FILE_BUCKET").unwrap_or_else(|_| "companyos-files".into()),
        }
    }
}
