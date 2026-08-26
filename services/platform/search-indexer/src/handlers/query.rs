//! Search query — org_id required; re-verify authz per hit.

use axum::extract::{Query, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::PublicId;
use companyos_tenancy::OrgId;
use serde::Deserialize;

use crate::auth::AuthCtx;
use crate::mapping::permission_for_doc_type;
use crate::principal::{can_receive, enforce, load_principal};
use crate::state::{AppState, SearchDoc};
use crate::types::{QueryResponse, SearchHit};

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub q: Option<String>,
    pub org_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/search/query",
    params(
        ("q" = Option<String>, Query, description = "search text"),
        ("org_id" = String, Query, description = "required tenant org public id"),
    ),
    responses((status = 200, body = QueryResponse)),
    tag = "search"
)]
pub async fn query(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();

    let org_raw = params
        .org_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                &request_id,
                "org_id query parameter is required",
            )
        })?;

    let org_public: PublicId = org_raw
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid org_id"))?;
    let org_id = OrgId::from_public(&org_public).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "org_id must be org_…",
        )
    })?;

    if auth.ctx.org_id != org_id {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "org_id does not match authenticated tenant",
        ));
    }

    let user_id = auth.ctx.actor.user_id;
    let principal = if auth.local_bypass {
        companyos_authz::Principal::with_roles(vec![companyos_authz::Role::Owner])
    } else {
        let (principal, _, _) = load_principal(&state.pool, org_id, user_id, &request_id).await?;
        enforce(&principal, perms::platform_search_read(), &request_id)?;
        principal
    };

    let q = params.q.unwrap_or_default().to_ascii_lowercase();
    let candidates = fetch_candidates(&state, org_id, &q).await?;

    let mut hits = Vec::new();
    for doc in candidates {
        let Some(perm) = permission_for_doc_type(&doc.doc_type) else {
            continue;
        };
        if !can_receive(&principal, perm) {
            continue;
        }
        hits.push(SearchHit {
            doc_id: doc.doc_id,
            doc_type: doc.doc_type,
            title: doc.title,
            body: doc.body,
            href: doc.href,
        });
    }

    Ok(Json(QueryResponse { hits }))
}

async fn fetch_candidates(
    state: &AppState,
    org_id: OrgId,
    q: &str,
) -> Result<Vec<SearchDoc>, AppError> {
    if let Some(base) = &state.opensearch_url {
        let url = format!("{}/companyos/_search", base.trim_end_matches('/'));
        let query_str = if q.is_empty() {
            "*".to_string()
        } else {
            q.to_string()
        };
        let body = serde_json::json!({
            "query": {
                "bool": {
                    "must": [{ "query_string": { "query": query_str } }],
                    "filter": [{ "term": { "org_id": org_id.as_uuid().to_string() } }]
                }
            },
            "size": 50
        });
        let resp = state
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::new(ErrorCode::ServiceUnavailable, "search", e.to_string()))?;
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
        let mut out = Vec::new();
        if let Some(hits) = v.pointer("/hits/hits").and_then(|h| h.as_array()) {
            for h in hits {
                if let Some(src) = h.get("_source") {
                    if let Ok(doc) = serde_json::from_value::<SearchDoc>(src.clone()) {
                        out.push(doc);
                    }
                }
            }
        }
        return Ok(out);
    }

    let map = state
        .memory
        .lock()
        .map_err(|e| AppError::new(ErrorCode::Internal, "search", e.to_string()))?;
    let mut out = Vec::new();
    for ((oid, _), doc) in map.iter() {
        if *oid != org_id.as_uuid() {
            continue;
        }
        if q.is_empty()
            || doc.title.to_ascii_lowercase().contains(q)
            || doc.body.to_ascii_lowercase().contains(q)
        {
            out.push(doc.clone());
        }
    }
    Ok(out)
}
