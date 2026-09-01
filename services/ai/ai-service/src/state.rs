//! Shared application state for companyos-ai.

use std::sync::Arc;

use companyos_auth_token::KeyRing;
use sqlx::PgPool;

use crate::agents::kill_switch::KillSwitchCache;
use crate::provider::{build_provider, LlmProvider};

pub const PROMPT_TEMPLATE_VERSION: &str = "ai.chat.v1";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keyring: KeyRing,
    pub http: reqwest::Client,
    pub gateway_url: String,
    pub search_url: String,
    pub provider: Arc<dyn LlmProvider>,
    pub prompt_template_version: String,
    pub kill_switch_cache: KillSwitchCache,
}

impl AppState {
    pub fn new(pool: PgPool, keyring: KeyRing) -> Self {
        let gateway_url =
            std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        let search_url =
            std::env::var("SEARCH_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8086".into());
        Self {
            pool,
            keyring,
            http: reqwest::Client::new(),
            gateway_url,
            search_url,
            provider: build_provider(),
            prompt_template_version: PROMPT_TEMPLATE_VERSION.into(),
            kill_switch_cache: KillSwitchCache::default(),
        }
    }
}
