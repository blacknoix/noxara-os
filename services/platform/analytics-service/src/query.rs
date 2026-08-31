//! Analytics query builder — rejects queries without an `org_id` predicate.
//!
//! Execution is permission-filtered: the viewer only sees facts their module
//! permissions allow. Simulate/dry-run never writes facts.

use companyos_authz::{is_allowed, PermissionId, Principal};
use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::OrgId;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use utoipa::ToSchema;

use crate::metrics::{get_metric, FactSource, MeasureKind, MetricDefinition};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueryFilter {
    pub field: String,
    pub op: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportDefinition {
    /// Must be present — query guard rejects missing org_id.
    pub org_id: Option<String>,
    pub metric: String,
    #[serde(default)]
    pub dimensions: Vec<String>,
    #[serde(default)]
    pub filters: Vec<QueryFilter>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default = "default_viz")]
    pub visualization: String,
}

fn default_viz() -> String {
    "table".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueryRow {
    pub dimensions: serde_json::Map<String, serde_json::Value>,
    pub value: i64,
    pub record_ids: Vec<String>,
    pub drill_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueryResult {
    pub metric: String,
    pub rows: Vec<QueryRow>,
    pub filtered_by_permission: bool,
    pub permission_denied_empty: bool,
    pub dry_run: bool,
    pub elapsed_ms: u64,
    pub freshness_as_of: Option<String>,
    pub eventually_consistent: bool,
}

#[derive(Debug, Clone)]
pub struct ValidatedQuery {
    pub org: OrgId,
    pub metric: MetricDefinition,
    pub dimensions: Vec<String>,
    pub filters: Vec<QueryFilter>,
    pub group_by: Vec<String>,
    pub visualization: String,
}

/// Reject queries without an org_id predicate (DoD).
pub fn validate_query(
    def: &ReportDefinition,
    request_id: &str,
) -> Result<ValidatedQuery, AppError> {
    let org_raw = def
        .org_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                "analytics query rejected: org_id predicate is required",
            )
        })?;
    let pub_id: companyos_ids::PublicId = org_raw
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, request_id, "invalid org_id"))?;
    let org = OrgId::from_public(&pub_id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "org_id must be org_…",
        )
    })?;
    let metric = get_metric(&def.metric).ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            format!("unknown governed metric '{}'", def.metric),
        )
    })?;
    for d in &def.dimensions {
        if !metric.dimensions.contains(d) && d != "occurred_at" {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("dimension '{d}' is not defined for metric {}", metric.name),
            ));
        }
    }
    Ok(ValidatedQuery {
        org,
        metric,
        dimensions: def.dimensions.clone(),
        filters: def.filters.clone(),
        group_by: def.group_by.clone(),
        visualization: def.visualization.clone(),
    })
}

pub fn viewer_may_see_fact(principal: &Principal, fact: FactSource) -> bool {
    is_allowed(principal, &fact.required_read_permission())
}

pub fn viewer_may_see_metric(principal: &Principal, metric: &MetricDefinition) -> bool {
    is_allowed(
        principal,
        &PermissionId::from(metric.required_permission.as_str()),
    )
}

fn record_id_column(fact: FactSource) -> &'static str {
    match fact {
        FactSource::DealStageChange => "deal_id",
        FactSource::InvoiceLifecycle | FactSource::InvoiceIssued => "invoice_id",
        FactSource::Payment => "payment_id",
        FactSource::Expense => "expense_id",
        FactSource::TaskLifecycle => "task_id",
        FactSource::AiUsage => "usage_kind",
        FactSource::ApiRequest => "route",
    }
}

fn amount_expr(metric: &MetricDefinition) -> String {
    match metric.measure {
        MeasureKind::Sum => format!("COALESCE(SUM({}), 0)", metric.measure_field),
        MeasureKind::Count => "COUNT(*)::bigint".into(),
        MeasureKind::Avg => format!("COALESCE(AVG({}), 0)::bigint", metric.measure_field),
    }
}

/// Execute a validated query against the Postgres fact mirror.
/// When `dry_run` is true, only validates and returns empty rows (no writes).
pub async fn execute_query(
    tx: &mut Transaction<'_, Postgres>,
    validated: &ValidatedQuery,
    principal: Option<&Principal>,
    dry_run: bool,
    request_id: &str,
) -> Result<QueryResult, AppError> {
    let start = std::time::Instant::now();
    let eventually_consistent = true;

    if dry_run {
        return Ok(QueryResult {
            metric: validated.metric.name.clone(),
            rows: vec![],
            filtered_by_permission: false,
            permission_denied_empty: false,
            dry_run: true,
            elapsed_ms: start.elapsed().as_millis() as u64,
            freshness_as_of: None,
            eventually_consistent,
        });
    }

    if let Some(p) = principal {
        if !viewer_may_see_metric(p, &validated.metric) {
            return Ok(QueryResult {
                metric: validated.metric.name.clone(),
                rows: vec![],
                filtered_by_permission: true,
                permission_denied_empty: true,
                dry_run: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                freshness_as_of: None,
                eventually_consistent,
            });
        }
    }

    let table = validated.metric.fact.postgres_table();
    let rec_col = record_id_column(validated.metric.fact);
    let measure = amount_expr(&validated.metric);

    // Lifecycle metrics often filter to a specific event type.
    let mut where_extra = String::new();
    if validated.metric.name == "revenue_issued" {
        where_extra.push_str(" AND lifecycle_event = 'issued'");
    }
    if validated.metric.name == "task_completions" {
        where_extra.push_str(" AND lifecycle_event = 'completed'");
    }

    let group_cols: Vec<&str> = if validated.group_by.is_empty() {
        validated.dimensions.iter().map(|s| s.as_str()).collect()
    } else {
        validated.group_by.iter().map(|s| s.as_str()).collect()
    };

    let select_dims = if group_cols.is_empty() {
        "NULL::text AS dim0".to_string()
    } else {
        group_cols
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c}::text AS dim{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let group_sql = if group_cols.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", group_cols.join(", "))
    };

    let sql = format!(
        "SELECT {select_dims}, {measure} AS value, \
         array_agg({rec_col}::text) AS record_ids \
         FROM {table} \
         WHERE org_id = $1 {where_extra} {group_sql} \
         ORDER BY value DESC \
         LIMIT 500"
    );

    let rows_raw: Vec<(Option<String>, i64, Vec<String>)> = if group_cols.is_empty() {
        sqlx::query_as(&sql)
            .bind(validated.org.as_uuid())
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
            .into_iter()
            .map(|(d0, v, ids): (Option<String>, i64, Option<Vec<String>>)| {
                (d0, v, ids.unwrap_or_default())
            })
            .collect()
    } else if group_cols.len() == 1 {
        sqlx::query_as(&sql)
            .bind(validated.org.as_uuid())
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
            .into_iter()
            .map(|(d0, v, ids): (Option<String>, i64, Option<Vec<String>>)| {
                (d0, v, ids.unwrap_or_default())
            })
            .collect()
    } else {
        // Multi-dimension: fold into dim0 = joined label for API simplicity.
        let sql_multi = format!(
            "SELECT concat_ws('|', {}) AS dim0, {measure} AS value, \
             array_agg({rec_col}::text) AS record_ids \
             FROM {table} \
             WHERE org_id = $1 {where_extra} {group_sql} \
             ORDER BY value DESC \
             LIMIT 500",
            group_cols.join(", ")
        );
        sqlx::query_as(&sql_multi)
            .bind(validated.org.as_uuid())
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?
            .into_iter()
            .map(|(d0, v, ids): (Option<String>, i64, Option<Vec<String>>)| {
                (d0, v, ids.unwrap_or_default())
            })
            .collect()
    };

    let drill_tmpl = validated.metric.drill_route.clone();
    let rows: Vec<QueryRow> = rows_raw
        .into_iter()
        .map(|(d0, value, record_ids)| {
            let mut dimensions = serde_json::Map::new();
            if let Some(label) = d0 {
                if group_cols.is_empty() {
                    dimensions.insert("_total".into(), serde_json::Value::String("all".into()));
                } else if group_cols.len() == 1 {
                    dimensions.insert(group_cols[0].to_string(), serde_json::Value::String(label));
                } else {
                    for (i, col) in group_cols.iter().enumerate() {
                        let part = label.split('|').nth(i).unwrap_or("").to_string();
                        dimensions.insert((*col).to_string(), serde_json::Value::String(part));
                    }
                }
            }
            let drill_links: Vec<String> = record_ids
                .iter()
                .take(20)
                .map(|id| drill_tmpl.replace("{record_id}", id))
                .collect();
            QueryRow {
                dimensions,
                value,
                record_ids,
                drill_links,
            }
        })
        .collect();

    let freshness: Option<(chrono::DateTime<chrono::Utc>,)> =
        sqlx::query_as("SELECT last_ingest_at FROM analytics_freshness WHERE org_id = $1")
            .bind(validated.org.as_uuid())
            .fetch_optional(&mut **tx)
            .await
            .ok()
            .flatten();

    Ok(QueryResult {
        metric: validated.metric.name.clone(),
        rows,
        filtered_by_permission: principal.is_some(),
        permission_denied_empty: false,
        dry_run: false,
        elapsed_ms: start.elapsed().as_millis() as u64,
        freshness_as_of: freshness.map(|(t,)| t.to_rfc3339()),
        eventually_consistent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_org_id() {
        let def = ReportDefinition {
            org_id: None,
            metric: "revenue_issued".into(),
            dimensions: vec![],
            filters: vec![],
            group_by: vec![],
            visualization: "table".into(),
        };
        let err = validate_query(&def, "t").unwrap_err();
        assert!(err.message.contains("org_id"));
    }

    #[test]
    fn accepts_org_id_predicate() {
        let org = OrgId::generate();
        let def = ReportDefinition {
            org_id: Some(org.to_public().as_str()),
            metric: "revenue_issued".into(),
            dimensions: vec!["currency".into()],
            filters: vec![],
            group_by: vec!["currency".into()],
            visualization: "bar".into(),
        };
        let v = validate_query(&def, "t").unwrap();
        assert_eq!(v.metric.name, "revenue_issued");
    }
}
