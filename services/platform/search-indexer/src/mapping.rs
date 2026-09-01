//! Map search doc types → authz permissions.

use companyos_authz::PermissionId;

pub fn permission_for_doc_type(doc_type: &str) -> Option<PermissionId> {
    if let Some(slug) = doc_type.strip_prefix("custom:") {
        return Some(PermissionId(format!("custom.{slug}.read")));
    }
    let perm = match doc_type {
        "customer" => "sales.customer.read",
        "deal" => "sales.deal.read",
        "invoice" => "finance.invoice.read",
        "task" => "operations.task.read",
        "project" => "operations.project.read",
        "employee" => "hr.employee.read",
        "leave_request" => "hr.leave.read",
        "attendance" => "hr.attendance.read",
        "payroll_run" | "payslip" => "hr.payroll.read",
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
        "employee" => Some("employee"),
        "leave" => Some("leave_request"),
        "attendance" => Some("attendance"),
        "payroll_run" => Some("payroll_run"),
        "payslip" => Some("payslip"),
        _ => None,
    }
}

/// Phase 4.4: custom entity aggregates use the entity slug; doc type is `custom:{slug}`.
pub fn doc_type_from_custom_aggregate(entity_slug: &str) -> String {
    format!("custom:{entity_slug}")
}
