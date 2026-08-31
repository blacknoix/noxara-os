//! Audit hash-chain verification. Fails closed: any content or chain-link
//! mismatch (or DB error) reports `ok = false` rather than a false positive.

use chrono::{DateTime, Utc};
use companyos_errors::AppError;
use companyos_tenancy::{set_session_org_id, OrgId};
use sqlx::PgPool;
use uuid::Uuid;

use super::types::AuditVerifyResponse;
use super::{internal, tenancy_internal};

/// Recomputes `content_hash` from the same fields the `audit_entry_hash_chain_trg`
/// trigger hashes, and compares each row's stored `prev_hash` against the
/// previous row's actual `content_hash` (via `LAG`) to catch broken links.
const VERIFY_SQL: &str = r#"
WITH ordered AS (
    SELECT
        id, org_id, actor_user_id, actor_on_behalf_of, actor_is_ai, action,
        resource_type, resource_id, metadata, created_at, prev_hash, content_hash,
        LAG(content_hash) OVER (ORDER BY created_at, id) AS expected_prev_hash
    FROM audit_entry
    WHERE org_id = $1 AND partition_key = $2
)
SELECT
    id,
    created_at,
    content_hash = encode(digest(
        COALESCE(prev_hash, '')
        || '|' || id::text
        || '|' || org_id::text
        || '|' || actor_user_id::text
        || '|' || actor_on_behalf_of::text
        || '|' || (CASE WHEN actor_is_ai THEN '1' ELSE '0' END)
        || '|' || action
        || '|' || resource_type
        || '|' || resource_id
        || '|' || COALESCE(metadata::text, '{}')
        || '|' || COALESCE(created_at, now())::text,
    'sha256'), 'hex') AS content_ok,
    prev_hash = COALESCE(expected_prev_hash, '') AS chain_ok
FROM ordered
ORDER BY created_at, id
"#;

struct PartitionCheck {
    rows_checked: i64,
    ok: bool,
    first_break: Option<String>,
}

async fn check_partition(
    pool: &PgPool,
    org_id: OrgId,
    partition_key: &str,
    request_id: &str,
) -> Result<PartitionCheck, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let rows: Vec<(Uuid, DateTime<Utc>, bool, bool)> = sqlx::query_as(VERIFY_SQL)
        .bind(org_id.as_uuid())
        .bind(partition_key)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    let rows_checked = rows.len() as i64;
    for (id, created_at, content_ok, chain_ok) in &rows {
        if !content_ok || !chain_ok {
            let reason = if !content_ok {
                "content_hash mismatch"
            } else {
                "prev_hash chain break"
            };
            return Ok(PartitionCheck {
                rows_checked,
                ok: false,
                first_break: Some(format!("{reason} at entry {id} ({created_at})")),
            });
        }
    }
    Ok(PartitionCheck {
        rows_checked,
        ok: true,
        first_break: None,
    })
}

pub async fn verify_partition(
    pool: &PgPool,
    org_id: OrgId,
    partition_key: &str,
    request_id: &str,
) -> Result<AuditVerifyResponse, AppError> {
    let check = check_partition(pool, org_id, partition_key, request_id).await?;
    Ok(AuditVerifyResponse {
        ok: check.ok,
        partitions_checked: 1,
        rows_checked: check.rows_checked,
        first_break: check.first_break,
    })
}

pub async fn verify_all(
    pool: &PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<AuditVerifyResponse, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;
    let partitions: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT partition_key FROM audit_entry
        WHERE org_id = $1 AND partition_key IS NOT NULL
        ORDER BY partition_key
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    let mut rows_checked = 0i64;
    let mut ok = true;
    let mut first_break = None;
    for (partition_key,) in &partitions {
        let check = check_partition(pool, org_id, partition_key, request_id).await?;
        rows_checked += check.rows_checked;
        if !check.ok && ok {
            ok = false;
            first_break = check.first_break.map(|m| format!("[{partition_key}] {m}"));
        }
    }

    Ok(AuditVerifyResponse {
        ok,
        partitions_checked: partitions.len() as i64,
        rows_checked,
        first_break,
    })
}
