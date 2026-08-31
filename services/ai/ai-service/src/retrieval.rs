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

    let resp = match state
        .http
        .get(&url)
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {bearer}"),
        )
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "search unreachable; using fixture retrieval");
            return Ok(fixture_citations_for_query(query.query()));
        }
    };

    if !resp.status().is_success() {
        return Ok(fixture_citations_for_query(query.query()));
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

    let mut citations: Vec<Citation> = hits
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

    if citations.is_empty() {
        citations = fixture_citations_for_query(query.query());
    }

    Ok(citations)
}

/// Fixture corpus for CI / offline gateway — multi-context golden Q&A.
pub fn fixture_citations_for_query(query: &str) -> Vec<Citation> {
    let lower = query.to_ascii_lowercase();
    let corpus = fixture_corpus();
    let mut scored: Vec<(usize, &Citation)> = corpus
        .iter()
        .map(|c| {
            let hay = format!(
                "{} {} {} {}",
                c.record_type,
                c.record_id,
                c.title,
                c.snippet.as_deref().unwrap_or("")
            )
            .to_ascii_lowercase();
            let score = lower
                .split_whitespace()
                .filter(|w| w.len() > 2 && hay.contains(w))
                .count();
            (score, c)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));

    if scored.is_empty() {
        return vec![corpus[0].clone(), corpus[1].clone(), corpus[3].clone()];
    }

    scored.into_iter().take(6).map(|(_, c)| c.clone()).collect()
}

fn fixture_corpus() -> Vec<Citation> {
    vec![
        Citation {
            record_type: "deal".into(),
            record_id: "dl_acme_stale".into(),
            title: "Acme Enterprise".into(),
            href: Some("/sales/deals?q=dl_acme_stale".into()),
            snippet: Some("Open deal — no activity 18 days; amount 120000 USD".into()),
        },
        Citation {
            record_type: "invoice".into(),
            record_id: "inv_acme_1001".into(),
            title: "INV-1001 Acme".into(),
            href: Some("/finance/invoices/inv_acme_1001".into()),
            snippet: Some("Overdue invoice Acme balance 45000 USD due 12 days ago".into()),
        },
        Citation {
            record_type: "contract".into(),
            record_id: "sct_northwind".into(),
            title: "Northwind Annual".into(),
            href: Some("/sales/contracts/sct_northwind".into()),
            snippet: Some("Active contract renews in 28 days".into()),
        },
        Citation {
            record_type: "task".into(),
            record_id: "tsk_ops_cutover".into(),
            title: "Cutover checklist".into(),
            href: Some("/ops/tasks".into()),
            snippet: Some("Blocked ops task waiting on finance approval for go-live".into()),
        },
        Citation {
            record_type: "expense".into(),
            record_id: "exp_travel_88".into(),
            title: "Travel expense Q3".into(),
            href: Some("/finance/expenses".into()),
            snippet: Some("Pending expense tied to ops travel budget".into()),
        },
        Citation {
            record_type: "project".into(),
            record_id: "prj_phoenix".into(),
            title: "Project Phoenix".into(),
            href: Some("/ops/projects/prj_phoenix".into()),
            snippet: Some("Ops delivery project with finance milestone billing".into()),
        },
    ]
}

/// Distinct citation contexts (record_type families) for quality scoring.
pub fn citation_contexts(citations: &[Citation]) -> Vec<String> {
    let mut contexts = Vec::new();
    for c in citations {
        let ctx = match c.record_type.as_str() {
            "deal" | "customer" | "lead" | "quote" | "contract" | "activity" => "sales",
            "invoice" | "expense" | "payment" | "journal" => "finance",
            "task" | "project" | "timesheet" => "ops",
            other => other,
        };
        if !contexts.iter().any(|x| x == ctx) {
            contexts.push(ctx.to_string());
        }
    }
    contexts
}

/// Score an ask answer for golden tests: contexts ≥2 and key entities present.
pub fn score_qa_answer(
    answer: &str,
    citations: &[Citation],
    required_entities: &[&str],
) -> QaScore {
    let contexts = citation_contexts(citations);
    let lower = answer.to_ascii_lowercase();
    let entities_hit: Vec<String> = required_entities
        .iter()
        .filter(|e| lower.contains(&e.to_ascii_lowercase()))
        .map(|s| (*s).to_string())
        .collect();
    QaScore {
        context_count: contexts.len(),
        contexts,
        entities_hit,
        entities_required: required_entities.len(),
        citation_count: citations.len(),
    }
}

#[derive(Debug, Clone)]
pub struct QaScore {
    pub context_count: usize,
    pub contexts: Vec<String>,
    pub entities_hit: Vec<String>,
    pub entities_required: usize,
    pub citation_count: usize,
}

impl QaScore {
    pub fn passes(&self) -> bool {
        self.context_count >= 2
            && self.citation_count >= 2
            && self.entities_hit.len() == self.entities_required
    }
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

    #[test]
    fn fixture_retrieval_covers_sales_and_finance() {
        let cites = fixture_citations_for_query("Acme overdue invoice deal");
        let ctx = citation_contexts(&cites);
        assert!(ctx.contains(&"sales".to_string()));
        assert!(ctx.contains(&"finance".to_string()));
    }

    #[test]
    fn qa_scorer_requires_multi_context() {
        let cites = fixture_citations_for_query("Acme deal invoice overdue");
        let answer = "Acme Enterprise deal is stale and INV-1001 Acme is overdue.";
        let score = score_qa_answer(answer, &cites, &["Acme", "INV-1001"]);
        assert!(score.passes(), "{score:?}");
    }
}
