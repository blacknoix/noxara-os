//! App state — ADR-011: facts derive from events only (never OLTP table scans).

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    pub clickhouse_url: Option<String>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        Self {
            pool,
            keyring,
            clickhouse_url: std::env::var("CLICKHOUSE_URL").ok().filter(|s| !s.is_empty()),
            http: reqwest::Client::new(),
        }
    }
}
