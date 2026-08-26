//! Tenant-scoped hybrid retrieval — org_id required at construction.

use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::OrgId;
use urlencoding::encode;

use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::Citation;

pub struct RetrievalQuery {
    org_id: OrgId,
    query: String,
}

impl RetrievalQuery {
    /// Compile/runtime error path: returns Err if org public id missing/empty.
    pub fn new(org_id: Option<&str>, query: &str) -> Result<Self, AppError> {
        let org_raw = org_id.filter(|s| !s.is_empty()).ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                "retrieval",
                "org_id is required for retrieval",
            )
        })?;
        let org_id = crate::auth::parse_org_public_id(org_raw)?;
        Ok(Self {
            org_id,
            query: query.to_string(),
        })
    }

    pub fn org_id(&self) -> OrgId {
        self.org_id
    }

    pub fn query(&self) -> &str {
        &self.query
    }
}

pub async fn hybrid_retrieve(
    state: &AppState,
    auth: &AuthCtx,
    query: RetrievalQuery,
    bearer: &str,
) -> Result<Vec<Citation>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    if auth.ctx.org_id != query.org_id() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "org_id does not match authenticated tenant",
        ));
    }

    let org_public = auth.ctx.org_id.to_public().as_str();
    let q = encode(query.query());
    let search_base = if !state.search_url.is_empty() {
        state.search_url.trim_end_matches('/').to_string()
    } else {
        format!("{}/api/v1/search", state.gateway_url.trim_end_matches('/'))
    };
    let url = if search_base.contains("/api/v1/search") {
        format!("{}/query?q={}&org_id={}", search_base, q, org_public)
    } else {
        format!(
            "{}/api/v1/search/query?q={}&org_id={}",
            search_base, q, org_public
        )
    };

    let resp = state
        .http
        .get(&url)
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {bearer}"),
        )
        .send()
        .await
        .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, &request_id, e.to_string()))?;

    if !resp.status().is_success() {
        return Ok(Vec::new());
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let hits = body
        .get("hits")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();

    let citations = hits
        .into_iter()
        .filter_map(|hit| {
            let doc_type = hit
                .get("doc_type")
                .and_then(|v| v.as_str())
                .unwrap_or("record");
            let doc_id = hit.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
            let title = hit.get("title").and_then(|v| v.as_str()).unwrap_or(doc_id);
            let href = hit.get("href").and_then(|v| v.as_str()).map(String::from);
            let snippet = hit
                .get("body")
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(200).collect());
            if doc_id.is_empty() {
                return None;
            }
            Some(Citation {
                record_type: doc_type.to_string(),
                record_id: doc_id.to_string(),
                title: title.to_string(),
                href,
                snippet,
            })
        })
        .collect();

    Ok(citations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_ids::new_uuid_v7;

    #[test]
    fn retrieval_query_requires_org_id() {
        assert!(RetrievalQuery::new(None, "x").is_err());
        assert!(RetrievalQuery::new(Some(""), "x").is_err());
    }

    #[test]
    fn retrieval_query_accepts_valid_org_public_id() {
        let org = OrgId::new(new_uuid_v7());
        let org_public = org.to_public().as_str();
        let q = RetrievalQuery::new(Some(&org_public), "hello").unwrap();
        assert_eq!(q.org_id(), org);
        assert_eq!(q.query(), "hello");
    }
}
