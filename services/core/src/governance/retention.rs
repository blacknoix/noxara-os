//! Per-org data retention configuration and audit-partition dry-run.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use companyos_errors::AppError;
use companyos_tenancy::{set_session_org_id, OrgId};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::types::{RetentionConfigView, RetentionDryRunResponse};
use super::{internal, tenancy_internal};

const DEFAULT_RETENTION_DAYS: i32 = 2555; // ~7 years

/// Bounded lookback so `select_cutoff_partitions` stays pure/testable without
/// a DB round trip. `dry_run` intersects this candidate list against actual
/// `audit_entry` partitions — partitions that don't exist simply contribute
/// zero rows to the estimate, so a 15-year window comfortably covers any
/// realistic org history under the max 10-year (3650-day) retention setting.
const LOOKBACK_MONTHS: i32 = 180;

pub async fn get(
    pool: &PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<RetentionConfigView, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let row: Option<(i32, Value, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT default_retention_days, overrides, updated_at, version FROM org_retention_config WHERE org_id = $1",
    )
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(match row {
        Some((default_retention_days, overrides, updated_at, version)) => RetentionConfigView {
            default_retention_days,
            overrides,
            updated_at: updated_at.to_rfc3339(),
            version,
        },
        None => RetentionConfigView {
            default_retention_days: DEFAULT_RETENTION_DAYS,
            overrides: serde_json::json!({}),
            updated_at: Utc::now().to_rfc3339(),
            version: 0,
        },
    })
}

pub async fn upsert(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    updated_by: Uuid,
    default_retention_days: Option<i32>,
    overrides: Option<Value>,
    request_id: &str,
) -> Result<RetentionConfigView, AppError> {
    let row: (i32, Value, DateTime<Utc>, i32) = sqlx::query_as(
        r#"
        INSERT INTO org_retention_config (org_id, default_retention_days, overrides, updated_by)
        VALUES ($1, COALESCE($2, 2555), COALESCE($3, '{}'::jsonb), $4)
        ON CONFLICT (org_id) DO UPDATE SET
            default_retention_days = COALESCE($2, org_retention_config.default_retention_days),
            overrides = COALESCE($3, org_retention_config.overrides),
            updated_by = $4,
            updated_at = now(),
            version = org_retention_config.version + 1
        RETURNING default_retention_days, overrides, updated_at, version
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(default_retention_days)
    .bind(overrides)
    .bind(updated_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok(RetentionConfigView {
        default_retention_days: row.0,
        overrides: row.1,
        updated_at: row.2.to_rfc3339(),
        version: row.3,
    })
}

/// Pure: computes the retention cutoff date and the `YYYY-MM` partition keys
/// strictly before the cutoff's month, bounded to [`LOOKBACK_MONTHS`].
/// `overrides` is accepted for API symmetry (per-resource day overrides can
/// feed dry-run estimate metadata later) but does not change which calendar
/// partitions are candidates — the audit hash chain partitions by month only.
pub fn select_cutoff_partitions(
    default_days: i32,
    _overrides: &Value,
    as_of: NaiveDate,
) -> (NaiveDate, Vec<String>) {
    let cutoff = as_of - Duration::days(default_days.max(0) as i64);
    let mut cursor =
        NaiveDate::from_ymd_opt(cutoff.year(), cutoff.month(), 1).expect("valid cutoff month");

    let mut partitions = Vec::with_capacity(LOOKBACK_MONTHS as usize);
    for _ in 0..LOOKBACK_MONTHS {
        cursor = prev_month(cursor);
        partitions.push(format!("{:04}-{:02}", cursor.year(), cursor.month()));
    }
    partitions.reverse();
    (cutoff, partitions)
}

fn prev_month(d: NaiveDate) -> NaiveDate {
    if d.month() == 1 {
        NaiveDate::from_ymd_opt(d.year() - 1, 12, 1).expect("valid date")
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() - 1, 1).expect("valid date")
    }
}

pub async fn dry_run(
    pool: &PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<RetentionDryRunResponse, AppError> {
    let config = get(pool, org_id, request_id).await?;
    let as_of = Utc::now().date_naive();
    let (cutoff, partitions) =
        select_cutoff_partitions(config.default_retention_days, &config.overrides, as_of);

    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_entry WHERE org_id = $1 AND partition_key = ANY($2)",
    )
    .bind(org_id.as_uuid())
    .bind(&partitions)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(RetentionDryRunResponse {
        cutoff_date: cutoff.to_string(),
        partitions,
        would_affect_estimate: count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_is_default_days_before_as_of() {
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 28).unwrap();
        let (cutoff, partitions) = select_cutoff_partitions(2555, &serde_json::json!({}), as_of);
        assert_eq!(cutoff, as_of - Duration::days(2555));
        assert_eq!(partitions.len(), 180);
    }

    #[test]
    fn partitions_exclude_cutoff_month_and_are_ordered() {
        let as_of = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let (cutoff, partitions) = select_cutoff_partitions(365, &serde_json::json!({}), as_of);
        let cutoff_month = format!("{:04}-{:02}", cutoff.year(), cutoff.month());
        assert!(!partitions.contains(&cutoff_month));
        assert!(partitions
            .iter()
            .all(|p| p.as_str() < cutoff_month.as_str()));

        let mut sorted = partitions.clone();
        sorted.sort();
        assert_eq!(
            partitions, sorted,
            "partitions must already be chronologically ordered"
        );
    }

    #[test]
    fn partitions_are_contiguous_months() {
        // as_of - 30 days lands in January 2030, so the cutoff month is
        // 2030-01 and the 180-month lookback runs 2015-01 .. 2029-12.
        let as_of = NaiveDate::from_ymd_opt(2030, 3, 1).unwrap();
        let (_cutoff, partitions) = select_cutoff_partitions(30, &serde_json::json!({}), as_of);
        assert_eq!(partitions.first().unwrap(), "2015-01");
        assert_eq!(partitions.last().unwrap(), "2029-12");
    }
}
