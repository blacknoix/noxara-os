//! Shared application state for companyos-search.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use companyos_auth_token::KeyRing;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDoc {
    pub org_id: Uuid,
    pub doc_id: String,
    pub doc_type: String,
    pub title: String,
    pub body: String,
    pub href: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    /// In-memory store when OPENSEARCH_URL is unset.
    pub memory: Arc<Mutex<HashMap<(Uuid, String), SearchDoc>>>,
    pub opensearch_url: Option<String>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        let opensearch_url = std::env::var("OPENSEARCH_URL").ok().filter(|s| !s.is_empty());
        Self {
            pool,
            keyring,
            memory: Arc::new(Mutex::new(HashMap::new())),
            opensearch_url,
            http: reqwest::Client::new(),
        }
    }
}
