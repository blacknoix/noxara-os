//! Public API route allowlist (Phase 3.3).
//!
//! Duplicated from `services/core/src/public_scopes.rs` so the gateway does not
//! depend on `companyos-core` (would create a cycle). Keep in sync when the
//! public catalogue changes.

use companyos_authz::{perms, PermissionId};

/// Public HTTP paths that accept organization API keys (method + path prefix).
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
        permission: perms::finance_invoice_issue, // issue/send refined below
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
        permission: perms::sales_customer_read, // any valid key; special-cased
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
#[allow(dead_code)] // Kept in sync with core; useful for diagnostics / future checks.
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

/// Whether effective scopes include the required permission string.
pub fn scopes_allow(scopes: &[String], required: &PermissionId) -> bool {
    let need = required.as_str();
    scopes.iter().any(|s| s == need)
}

/// OpenAPI public doc — any authenticated API key may fetch it (or unauthenticated).
pub fn is_openapi_public(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path == "/api/v1/openapi.public.json" || path.starts_with("/api/v1/openapi.public.json")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn non_public_path() {
        assert!(public_permission_for("GET", "/api/v1/hello").is_none());
        assert!(!is_public_path("/api/v1/ai/chat"));
    }
}
