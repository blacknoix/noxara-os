//! Lightweight event schema contract tests (Phase 1.8).
//!
//! Loads each `docs/events/schemas/*.json`, builds a sample [`EventEnvelope`],
//! and checks required envelope + payload keys without a full JSON Schema crate.

use std::fs;
use std::path::PathBuf;

use companyos_events::{Context, EventEnvelope};
use companyos_tenancy::Actor;
use serde_json::Value;

fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/events/schemas")
}

fn parse_schema_name(stem: &str) -> (Context, &str, &str, u32) {
    // e.g. finance.invoice.issued.v1
    let parts: Vec<&str> = stem.split('.').collect();
    assert!(
        parts.len() >= 4,
        "schema stem {stem} expected context.aggregate.event.vN"
    );
    let version = parts
        .last()
        .and_then(|v| v.strip_prefix('v'))
        .and_then(|v| v.parse().ok())
        .expect("version");
    let context = match parts[0] {
        "core" => Context::Core,
        "sales" => Context::Sales,
        "finance" => Context::Finance,
        "operations" => Context::Operations,
        "people" => Context::People,
        other => panic!("unknown context {other}"),
    };
    (context, parts[1], parts[2], version)
}

fn sample_payload(aggregate: &str, event_type: &str) -> Value {
    match (aggregate, event_type) {
        ("hello", "created") => serde_json::json!({
            "hello_id": "hel_test",
            "message": "hi"
        }),
        ("customer", "created") => serde_json::json!({ "customer_id": "cus_test" }),
        ("deal", "created" | "won") => serde_json::json!({ "deal_id": "dea_test" }),
        ("invoice", "issued" | "paid") => serde_json::json!({ "invoice_id": "inv_test" }),
        ("expense", "submitted") => serde_json::json!({ "expense_id": "exp_test" }),
        ("project", "created") => serde_json::json!({ "project_id": "prj_test" }),
        ("task", "created" | "completed") => serde_json::json!({ "task_id": "tsk_test" }),
        ("approval", "requested" | "decided") => serde_json::json!({ "approval_id": "apr_test" }),
        ("employee", "created" | "updated" | "onboarded" | "offboarded") => {
            serde_json::json!({ "id": "emp_test", "display_name": "Test Employee" })
        }
        ("attendance", "recorded") => serde_json::json!({
            "id": "att_test",
            "employee_id": "emp_test",
            "entry_kind": "check_in"
        }),
        ("leave", "requested" | "approved" | "rejected" | "cancelled" | "ledger_posted") => {
            serde_json::json!({ "id": "lvr_test", "employee_id": "emp_test" })
        }
        ("leave", "carry_forward") => serde_json::json!({
            "workflow_id": "org_test:LeaveCarryForward:2026",
            "year": 2026
        }),
        ("holiday", "created") => serde_json::json!({
            "id": "hol_test",
            "holiday_date": "2026-12-25"
        }),
        ("payroll_run", "drafted" | "calculated" | "approved" | "paid") => {
            serde_json::json!({ "id": "payrun_test" })
        }
        ("payslip", "issued") => serde_json::json!({ "id": "payslip_test" }),
        ("journal", "posted") => serde_json::json!({ "id": "jrn_test" }),
        ("period", "closed" | "reopened") => {
            serde_json::json!({ "id": "period_test", "code": "2026-08" })
        }
        ("statement", "imported") => serde_json::json!({ "id": "stmt_test" }),
        ("reconciliation", "matched") => serde_json::json!({
            "statement_id": "stmt_test",
            "matched": 9,
            "unmatched": 1
        }),
        ("reimbursement", "batched") => serde_json::json!({ "id": "reimb_test" }),
        _ => panic!("add sample_payload for {aggregate}.{event_type}"),
    }
}

fn required_payload_keys(schema: &Value) -> Vec<String> {
    schema
        .pointer("/properties/payload/required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn assert_envelope_matches_schema(schema: &Value, envelope: &EventEnvelope) {
    let wire = serde_json::to_value(envelope).expect("serialize envelope");
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("schema.required");
    for key in required {
        let k = key.as_str().expect("required key string");
        assert!(wire.get(k).is_some(), "envelope missing required field {k}");
    }

    if let Some(ctx) = schema
        .pointer("/properties/context/const")
        .and_then(|v| v.as_str())
    {
        assert_eq!(envelope.context.as_str(), ctx);
    }
    if let Some(agg) = schema
        .pointer("/properties/aggregate/const")
        .and_then(|v| v.as_str())
    {
        assert_eq!(envelope.aggregate, agg);
    }
    if let Some(ev) = schema
        .pointer("/properties/event_type/const")
        .and_then(|v| v.as_str())
    {
        assert_eq!(envelope.event_type, ev);
    }
    if let Some(ver) = schema
        .pointer("/properties/version/const")
        .and_then(|v| v.as_u64())
    {
        assert_eq!(envelope.version as u64, ver);
    }

    assert!(!envelope.idempotency_key.is_empty());
    assert!(!envelope.subject.is_empty());
    assert!(wire.get("org_id").is_some());

    let payload = wire.get("payload").expect("payload");
    for key in required_payload_keys(schema) {
        assert!(
            payload.get(&key).is_some(),
            "payload missing required key {key}"
        );
    }
}

fn load_and_check(stem: &str) {
    let path = schemas_dir().join(format!("{stem}.json"));
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let schema: Value = serde_json::from_str(&raw).expect("parse schema json");
    let (context, aggregate, event_type, version) = parse_schema_name(stem);
    let org = companyos_tenancy::OrgId::generate();
    let actor = Actor::human(companyos_ids::new_uuid_v7());
    let envelope = EventEnvelope::new(
        org,
        context,
        aggregate,
        event_type,
        version,
        actor,
        sample_payload(aggregate, event_type),
    );
    assert_envelope_matches_schema(&schema, &envelope);
}

#[test]
fn contract_core_hello_created_v1() {
    load_and_check("core.hello.created.v1");
}

#[test]
fn contract_sales_customer_created_v1() {
    load_and_check("sales.customer.created.v1");
}

#[test]
fn contract_sales_deal_created_v1() {
    load_and_check("sales.deal.created.v1");
}

#[test]
fn contract_finance_invoice_issued_v1() {
    load_and_check("finance.invoice.issued.v1");
}

#[test]
fn contract_finance_expense_submitted_v1() {
    load_and_check("finance.expense.submitted.v1");
}

#[test]
fn contract_operations_project_created_v1() {
    load_and_check("operations.project.created.v1");
}

#[test]
fn contract_operations_task_created_v1() {
    load_and_check("operations.task.created.v1");
}

#[test]
fn contract_operations_approval_requested_v1() {
    load_and_check("operations.approval.requested.v1");
}

#[test]
fn contract_sales_deal_won_v1() {
    load_and_check("sales.deal.won.v1");
}

#[test]
fn contract_finance_invoice_paid_v1() {
    load_and_check("finance.invoice.paid.v1");
}

#[test]
fn contract_operations_approval_decided_v1() {
    load_and_check("operations.approval.decided.v1");
}

#[test]
fn contract_operations_task_completed_v1() {
    load_and_check("operations.task.completed.v1");
}

#[test]
fn every_schema_file_has_a_contract_test() {
    let dir = schemas_dir();
    let mut stems = Vec::new();
    for entry in fs::read_dir(&dir).expect("schemas dir") {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if let Some(stem) = name.strip_suffix(".json") {
            stems.push(stem.to_string());
        }
    }
    assert!(
        !stems.is_empty(),
        "expected schema files in {}",
        dir.display()
    );
    for stem in &stems {
        load_and_check(stem);
    }
}
