//! Governed semantic metric layer — one definition per metric name.
//!
//! The UI and API both consume [`METRIC_CATALOGUE`] / [`list_metrics`] so every
//! surface that shows a metric agrees on formula, unit, and source fact.

use companyos_authz::PermissionId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How a measure is aggregated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    Sum,
    Count,
    Avg,
}

/// Unit of a metric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricUnit {
    MoneyMinor,
    Count,
    Tokens,
}

/// Fact table backing a metric (maps to Postgres `analytics_fact_*` / ClickHouse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactSource {
    DealStageChange,
    InvoiceLifecycle,
    Payment,
    Expense,
    TaskLifecycle,
    AiUsage,
    ApiRequest,
    /// Legacy Phase 1.8 invoice-issued mirror.
    InvoiceIssued,
}

impl FactSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DealStageChange => "fact_deal_stage_change",
            Self::InvoiceLifecycle => "fact_invoice_lifecycle",
            Self::Payment => "fact_payment",
            Self::Expense => "fact_expense",
            Self::TaskLifecycle => "fact_task_lifecycle",
            Self::AiUsage => "fact_ai_usage",
            Self::ApiRequest => "fact_api_request",
            Self::InvoiceIssued => "fact_invoice_issued",
        }
    }

    pub fn postgres_table(self) -> &'static str {
        match self {
            Self::DealStageChange => "analytics_fact_deal_stage_change",
            Self::InvoiceLifecycle => "analytics_fact_invoice_lifecycle",
            Self::Payment => "analytics_fact_payment",
            Self::Expense => "analytics_fact_expense",
            Self::TaskLifecycle => "analytics_fact_task_lifecycle",
            Self::AiUsage => "analytics_fact_ai_usage",
            Self::ApiRequest => "analytics_fact_api_request",
            Self::InvoiceIssued => "analytics_fact_invoice_issued",
        }
    }

    /// Module permission required to see rows from this fact (row-level filter).
    pub fn required_read_permission(self) -> PermissionId {
        match self {
            Self::DealStageChange => PermissionId::from("sales.deal.read"),
            Self::InvoiceLifecycle | Self::InvoiceIssued | Self::Payment => {
                PermissionId::from("finance.invoice.read")
            }
            Self::Expense => PermissionId::from("finance.expense.read"),
            Self::TaskLifecycle => PermissionId::from("operations.task.read"),
            Self::AiUsage => PermissionId::from("ai.insights.read"),
            Self::ApiRequest => PermissionId::from("platform.analytics.read"),
        }
    }

    /// Consumer durable name: `{service}--{purpose}`.
    pub fn consumer_name(self) -> &'static str {
        match self {
            Self::DealStageChange => "analytics--fact-deal-stage",
            Self::InvoiceLifecycle => "analytics--fact-invoice",
            Self::Payment => "analytics--fact-payment",
            Self::Expense => "analytics--fact-expense",
            Self::TaskLifecycle => "analytics--fact-task",
            Self::AiUsage => "analytics--fact-ai-usage",
            Self::ApiRequest => "analytics--fact-api-request",
            Self::InvoiceIssued => "analytics--fact-invoice-issued",
        }
    }

    pub fn drill_route_template(self) -> &'static str {
        match self {
            Self::DealStageChange => "/sales/deals/{record_id}",
            Self::InvoiceLifecycle | Self::InvoiceIssued => "/finance/invoices/{record_id}",
            Self::Payment => "/finance/invoices/{record_id}",
            Self::Expense => "/finance/expenses/{record_id}",
            Self::TaskLifecycle => "/ops/tasks/{record_id}",
            Self::AiUsage => "/settings/ai",
            Self::ApiRequest => "/insights",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct MetricDefinition {
    /// Stable machine name — unique across the catalogue.
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub fact: FactSource,
    pub measure: MeasureKind,
    /// Column used for sum/avg (ignored for count).
    pub measure_field: String,
    pub dimensions: Vec<String>,
    pub unit: MetricUnit,
    /// Permission required to see this metric's values (same as fact source).
    pub required_permission: String,
    pub drill_route: String,
    /// Flagship metrics appear in benchmark/trend views.
    pub flagship: bool,
}

/// Single source of truth for governed metrics.
pub static METRIC_CATALOGUE: &[MetricDefinition] = &[];

fn defs() -> Vec<MetricDefinition> {
    vec![
        MetricDefinition {
            name: "pipeline_amount".into(),
            display_name: "Pipeline amount".into(),
            description: "Sum of deal amounts on stage-change facts (amount_minor).".into(),
            fact: FactSource::DealStageChange,
            measure: MeasureKind::Sum,
            measure_field: "amount_minor".into(),
            dimensions: vec!["to_stage".into(), "currency".into()],
            unit: MetricUnit::MoneyMinor,
            required_permission: "sales.deal.read".into(),
            drill_route: FactSource::DealStageChange.drill_route_template().into(),
            flagship: true,
        },
        MetricDefinition {
            name: "revenue_issued".into(),
            display_name: "Revenue issued".into(),
            description: "Sum of invoice lifecycle 'issued' amounts (amount_minor).".into(),
            fact: FactSource::InvoiceLifecycle,
            measure: MeasureKind::Sum,
            measure_field: "amount_minor".into(),
            dimensions: vec!["currency".into(), "lifecycle_event".into()],
            unit: MetricUnit::MoneyMinor,
            required_permission: "finance.invoice.read".into(),
            drill_route: FactSource::InvoiceLifecycle.drill_route_template().into(),
            flagship: true,
        },
        MetricDefinition {
            name: "cash_collected".into(),
            display_name: "Cash collected".into(),
            description: "Sum of payment fact amounts (amount_minor).".into(),
            fact: FactSource::Payment,
            measure: MeasureKind::Sum,
            measure_field: "amount_minor".into(),
            dimensions: vec!["currency".into()],
            unit: MetricUnit::MoneyMinor,
            required_permission: "finance.invoice.read".into(),
            drill_route: FactSource::Payment.drill_route_template().into(),
            flagship: true,
        },
        MetricDefinition {
            name: "expenses_total".into(),
            display_name: "Expenses".into(),
            description: "Sum of expense fact amounts (amount_minor).".into(),
            fact: FactSource::Expense,
            measure: MeasureKind::Sum,
            measure_field: "amount_minor".into(),
            dimensions: vec!["category".into(), "currency".into()],
            unit: MetricUnit::MoneyMinor,
            required_permission: "finance.expense.read".into(),
            drill_route: FactSource::Expense.drill_route_template().into(),
            flagship: true,
        },
        MetricDefinition {
            name: "task_completions".into(),
            display_name: "Task completions".into(),
            description: "Count of task lifecycle 'completed' events.".into(),
            fact: FactSource::TaskLifecycle,
            measure: MeasureKind::Count,
            measure_field: "event_id".into(),
            dimensions: vec!["status".into(), "project_id".into()],
            unit: MetricUnit::Count,
            required_permission: "operations.task.read".into(),
            drill_route: FactSource::TaskLifecycle.drill_route_template().into(),
            flagship: false,
        },
        MetricDefinition {
            name: "ai_tokens_used".into(),
            display_name: "AI tokens used".into(),
            description: "Sum of AI usage tokens.".into(),
            fact: FactSource::AiUsage,
            measure: MeasureKind::Sum,
            measure_field: "tokens".into(),
            dimensions: vec!["usage_kind".into(), "model".into()],
            unit: MetricUnit::Tokens,
            required_permission: "ai.insights.read".into(),
            drill_route: FactSource::AiUsage.drill_route_template().into(),
            flagship: false,
        },
        MetricDefinition {
            name: "headcount_proxy".into(),
            display_name: "Headcount (proxy)".into(),
            description:
                "Count of distinct task assignees is not available; uses task lifecycle event count as ops activity proxy. HR headcount facts land when people events are ingested."
                    .into(),
            fact: FactSource::TaskLifecycle,
            measure: MeasureKind::Count,
            measure_field: "event_id".into(),
            dimensions: vec!["project_id".into()],
            unit: MetricUnit::Count,
            required_permission: "operations.task.read".into(),
            drill_route: FactSource::TaskLifecycle.drill_route_template().into(),
            flagship: true,
        },
    ]
}

/// Catalogue with uniqueness enforced at construction / test time.
pub fn list_metrics() -> Vec<MetricDefinition> {
    defs()
}

pub fn get_metric(name: &str) -> Option<MetricDefinition> {
    defs().into_iter().find(|m| m.name == name)
}

pub fn flagship_metrics() -> Vec<MetricDefinition> {
    defs().into_iter().filter(|m| m.flagship).collect()
}

/// Golden JSON for UI/API agreement tests.
pub fn catalogue_golden_json() -> String {
    serde_json::to_string_pretty(&list_metrics()).expect("metric catalogue serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn one_definition_per_metric_name() {
        let mut seen = HashSet::new();
        for m in list_metrics() {
            assert!(
                seen.insert(m.name.clone()),
                "duplicate metric name {}",
                m.name
            );
            assert!(!m.description.is_empty());
            assert_eq!(
                m.required_permission,
                m.fact.required_read_permission().as_str()
            );
        }
    }

    #[test]
    fn golden_json_round_trips() {
        let json = catalogue_golden_json();
        let back: Vec<MetricDefinition> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, list_metrics());
    }
}
