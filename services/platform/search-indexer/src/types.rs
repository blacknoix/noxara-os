//! DTOs for search API.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchHit {
    pub doc_id: String,
    pub doc_type: String,
    pub title: String,
    pub body: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueryResponse {
    pub hits: Vec<SearchHit>,
    /// True when OpenSearch was unavailable and Postgres list fallback served results.
    #[serde(default)]
    pub degraded: bool,
    /// Operator/UI banner key when degraded (e.g. `search_opensearch_fallback`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    pub upserted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReindexRequest {
    pub org_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReindexResponse {
    pub job_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}
