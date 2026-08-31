//! Public API scope vocabulary and route allowlist (Phase 3.3).
//!
//! Organization API key `scopes` must be permission IDs from this set (or a
//! documented alias). At authentication time, scopes are intersected with the
//! key owner's role permissions — the narrower of the two wins.

use companyos_authz::{perms, PermissionId};

/// Canonical public scopes (permission IDs) a third-party key may request.
pub const PUBLIC_SCOPES: &[&str] = &[
    "sales.customer.read",
    "sales.customer.create",
    "sales.customer.update",
    "sales.deal.read",
    "sales.deal.create",
    "sales.deal.update",
    "sales.quote.read",
    "sales.quote.create",
    "sales.quote.update",
    "finance.invoice.read",
    "finance.invoice.create",
    "finance.invoice.update",
    "finance.invoice.issue",
    "finance.invoice.send",
    "finance.payment.read",
    "finance.payment.create",
    "admin.api_key.manage",
    "admin.webhook.read",
    "admin.webhook.write",
    "admin.webhook.replay",
];

/// Returns true when `scope` is a known public permission id.
pub fn is_public_scope(scope: &str) -> bool {
    PUBLIC_SCOPES.contains(&scope)
}

/// Validate requested scopes; returns only the valid public ones.
/// Unknown scopes are rejected (strict) — callers map that to 400.
pub fn validate_requested_scopes(scopes: &[String]) -> Result<Vec<String>, String> {
    if scopes.is_empty() {
        return Err("at least one scope is required".into());
    }
    let mut out = Vec::with_capacity(scopes.len());
    for s in scopes {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("empty scope is not allowed".into());
        }
        if !is_public_scope(trimmed) {
            return Err(format!(
                "scope `{trimmed}` is not a public API scope; see docs/developers/scopes.md"
            ));
        }
        if !out.iter().any(|x: &String| x == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    Ok(out)
}

/// Public HTTP paths that accept organization API keys (method + path prefix).
///
/// Internal-only admin, Temporal, AI commit, and cross-tenant routes are excluded.
#[derive(Debug, Clone, Copy)]
pub struct PublicRoute {
    pub method: &'static str,
    pub path_prefix: &'static str,
    /// Permission required (checked against effective API-key scopes).
    pub permission: fn() -> PermissionId,
}

pub const PUBLIC_ROUTES: &[PublicRoute] = &[
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/sales/customers",
        permission: perms::sales_customer_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/sales/customers",
        permission: perms::sales_customer_create,
    },
    PublicRoute {
        method: "PATCH",
        path_prefix: "/api/v1/sales/customers/",
        permission: perms::sales_customer_update,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/sales/deals",
        permission: perms::sales_deal_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/sales/deals",
        permission: perms::sales_deal_create,
    },
    PublicRoute {
        method: "PATCH",
        path_prefix: "/api/v1/sales/deals/",
        permission: perms::sales_deal_update,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/sales/quotes",
        permission: perms::sales_quote_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/sales/quotes",
        permission: perms::sales_quote_create,
    },
    PublicRoute {
        method: "PATCH",
        path_prefix: "/api/v1/sales/quotes/",
        permission: perms::sales_quote_update,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/finance/invoices",
        permission: perms::finance_invoice_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/finance/invoices",
        permission: perms::finance_invoice_create,
    },
    PublicRoute {
        method: "PATCH",
        path_prefix: "/api/v1/finance/invoices/",
        permission: perms::finance_invoice_update,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/finance/invoices/",
        permission: perms::finance_invoice_issue, // issue/send under this prefix; gateway refines
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/finance/payments",
        permission: perms::finance_payment_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/finance/payments",
        permission: perms::finance_payment_create,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/governance/api-keys",
        permission: perms::admin_api_key_manage,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/governance/api-keys",
        permission: perms::admin_api_key_manage,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/governance/webhooks",
        permission: perms::admin_webhook_read,
    },
    PublicRoute {
        method: "POST",
        path_prefix: "/api/v1/governance/webhooks",
        permission: perms::admin_webhook_write,
    },
    PublicRoute {
        method: "PATCH",
        path_prefix: "/api/v1/governance/webhooks/",
        permission: perms::admin_webhook_write,
    },
    PublicRoute {
        method: "GET",
        path_prefix: "/api/v1/openapi.public.json",
        permission: perms::sales_customer_read, // any valid key; gateway special-cases
    },
];

/// Match a request against the public allowlist; return required permission if public.
pub fn public_permission_for(method: &str, path: &str) -> Option<PermissionId> {
    let path = path.split('?').next().unwrap_or(path);
    // More specific invoice sub-actions.
    if method.eq_ignore_ascii_case("POST") && path.contains("/api/v1/finance/invoices/") {
        if path.ends_with("/issue") {
            return Some(perms::finance_invoice_issue());
        }
        if path.ends_with("/send") {
            return Some(perms::finance_invoice_send());
        }
    }
    if method.eq_ignore_ascii_case("POST")
        && path.contains("/api/v1/governance/webhooks/")
        && path.ends_with("/replay")
    {
        return Some(perms::admin_webhook_replay());
    }
    if method.eq_ignore_ascii_case("POST")
        && path.contains("/api/v1/governance/webhooks/")
        && (path.ends_with("/rotate") || path.ends_with("/disable"))
    {
        return Some(perms::admin_webhook_write());
    }
    for route in PUBLIC_ROUTES {
        if !method.eq_ignore_ascii_case(route.method) {
            continue;
        }
        if path == route.path_prefix || path.starts_with(route.path_prefix) {
            return Some((route.permission)());
        }
    }
    None
}

/// True when the path is on the public catalogue (any method).
pub fn is_public_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    if path == "/api/v1/openapi.public.json" || path.starts_with("/api/v1/openapi.public.json") {
        return true;
    }
    PUBLIC_ROUTES.iter().any(|r| {
        path == r.path_prefix
            || path.starts_with(r.path_prefix)
            || (r.path_prefix.ends_with('/')
                && path.starts_with(r.path_prefix.trim_end_matches('/')))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_scope() {
        assert!(validate_requested_scopes(&["not.a.scope".into()]).is_err());
    }

    #[test]
    fn accepts_customer_read() {
        let v = validate_requested_scopes(&["sales.customer.read".into()]).unwrap();
        assert_eq!(v, vec!["sales.customer.read".to_string()]);
    }

    #[test]
    fn public_route_customers_get() {
        let p = public_permission_for("GET", "/api/v1/sales/customers").unwrap();
        assert_eq!(p.as_str(), "sales.customer.read");
    }

    #[test]
    fn invoice_issue_needs_issue_perm() {
        let p = public_permission_for("POST", "/api/v1/finance/invoices/inv_x/issue").unwrap();
        assert_eq!(p.as_str(), "finance.invoice.issue");
    }
}
