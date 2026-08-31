//! Filtered public OpenAPI document (Phase 3.3).
//!
//! Publishes only the third-party catalogue paths. Internal governance admin
//! (access review, SSO, retention) and non-public domains are omitted.

use serde_json::{json, Value};
use utoipa::OpenApi;

use crate::public_scopes::{is_public_path, PUBLIC_SCOPES};

/// Paths that belong on the public contract (prefix match).
const PUBLIC_PATH_PREFIXES: &[&str] = &[
    "/api/v1/sales/customers",
    "/api/v1/sales/deals",
    "/api/v1/sales/quotes",
    "/api/v1/finance/invoices",
    "/api/v1/finance/payments",
    "/api/v1/governance/api-keys",
    "/api/v1/governance/webhooks",
    "/api/v1/openapi.public.json",
];

/// Build a public OpenAPI 3.1 document from core's ApiDoc filtered to public prefixes.
pub fn public_openapi() -> Value {
    let raw = crate::openapi::ApiDoc::openapi()
        .to_pretty_json()
        .unwrap_or_else(|_| "{}".into());
    let mut doc: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|_| json!({ "openapi": "3.1.0", "paths": {}, "components": {} }));

    if let Some(info) = doc.get_mut("info") {
        info["title"] = json!("CompanyOS Public API");
        info["version"] = json!("v1");
        info["description"] = json!(
            "Stable public REST surface for third-party integrations. Authenticate with an organization API key (Authorization: Bearer <key> or X-Api-Key). See docs/developers/."
        );
    }

    if let Some(paths) = doc.get_mut("paths").and_then(|p| p.as_object_mut()) {
        paths.retain(|path, _| {
            PUBLIC_PATH_PREFIXES.iter().any(|prefix| {
                path == prefix
                    || path.starts_with(&format!("{prefix}/"))
                    || path.starts_with(prefix)
            }) || is_public_path(path)
        });
        for (_path, item) in paths.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                for (_method, op) in obj.iter_mut() {
                    if let Some(op_obj) = op.as_object_mut() {
                        op_obj.insert("x-companyos-public".into(), json!(true));
                    }
                }
            }
        }
    }

    doc["x-companyos-public-scopes"] = json!(PUBLIC_SCOPES);
    doc["x-companyos-deprecation-policy"] = json!({
        "window_days": 180,
        "dual_publish": true,
        "headers": ["Deprecation", "Sunset", "Link"],
        "doc": "docs/developers/deprecation.md"
    });

    if let Some(schemas) = doc
        .pointer_mut("/components/schemas/ApiKeyExchangeResponse/properties")
        .and_then(|p| p.as_object_mut())
    {
        if let Some(rpm) = schemas.get_mut("rate_limit_rpm") {
            rpm["deprecated"] = json!(true);
            rpm["description"] = json!(
                "Deprecated alias of rate_limit_per_minute. Dual-published for 180 days; remove after Sunset."
            );
        }
        if let Some(primary) = schemas.get_mut("rate_limit_per_minute") {
            primary["description"] = json!(
                "Per-key rate limit (requests per minute). Prefer this over deprecated rate_limit_rpm."
            );
        }
    }

    doc
}

pub fn public_openapi_json() -> String {
    serde_json::to_string_pretty(&public_openapi()).expect("public openapi json")
}
