//! Map search doc types → authz permissions.

use companyos_authz::PermissionId;

pub fn permission_for_doc_type(doc_type: &str) -> Option<PermissionId> {
    let perm = match doc_type {
        "customer" => "sales.customer.read",
        "deal" => "sales.deal.read",
        "invoice" => "finance.invoice.read",
        "task" => "operations.task.read",
        "project" => "operations.project.read",
        _ => return None,
    };
    Some(PermissionId::from(perm))
}

pub fn doc_type_from_aggregate(aggregate: &str) -> Option<&'static str> {
    match aggregate {
        "customer" => Some("customer"),
        "deal" => Some("deal"),
        "invoice" => Some("invoice"),
        "task" => Some("task"),
        "project" => Some("project"),
        _ => None,
    }
}
