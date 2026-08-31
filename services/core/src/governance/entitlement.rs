//! Permission entitlement history — answers "who could see X in period Y".
//!
//! `record_entitlements_for_membership` is the write-side hook: call it
//! whenever a membership's role/permission set changes (create, role
//! change, revoke) so the history stays gap-free.

use chrono::{DateTime, Utc};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::types::EntitlementRow;
use super::{internal, tenancy_internal};

/// Close prior open entitlement rows for `user_id` and record one new allow
/// row per permission in `permissions` (`(permission_id, effect)` pairs —
/// effect is normally `"allow"`; deny grants are not entitlements to see
/// anything and are skipped by callers).
pub async fn record_entitlements_for_membership(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    user_id: Uuid,
    membership_id: Uuid,
    role_key: &str,
    permissions: &[(String, String)],
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE permission_entitlement_history
        SET effective_to = $3
        WHERE org_id = $1 AND user_id = $2 AND effective_to IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(user_id)
    .bind(at)
    .execute(&mut **tx)
    .await?;

    for (permission_id, effect) in permissions {
        sqlx::query(
            r#"
            INSERT INTO permission_entitlement_history (
                id, org_id, user_id, membership_id, role_key, permission_id, effect, effective_from
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(user_id)
        .bind(membership_id)
        .bind(role_key)
        .bind(permission_id)
        .bind(effect)
        .bind(at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

type EntitlementDbRow = (
    Uuid,
    String,
    String,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub(crate) async fn fetch_could_see<'e, E>(
    executor: E,
    org_id: OrgId,
    permission_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<EntitlementRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let rows: Vec<EntitlementDbRow> = sqlx::query_as(
        r#"
        SELECT h.user_id, u.email, h.role_key, h.permission_id, h.effective_from, h.effective_to
        FROM permission_entitlement_history h
        JOIN user_identity u ON u.id = h.user_id
        WHERE h.org_id = $1
          AND h.permission_id = $2
          AND h.effect = 'allow'
          AND h.effective_from <= $4
          AND (h.effective_to IS NULL OR h.effective_to >= $3)
        ORDER BY h.effective_from ASC
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(permission_id)
    .bind(start)
    .bind(end)
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(user_id, email, role_key, permission_id, effective_from, effective_to)| {
                EntitlementRow {
                    user_id: PublicId::new(IdKind::User, user_id).as_str(),
                    email,
                    role_key,
                    permission_id,
                    effective_from: effective_from.to_rfc3339(),
                    effective_to: effective_to.map(|d| d.to_rfc3339()),
                }
            },
        )
        .collect())
}

/// Who could see `permission_id` (an active `allow` grant) at any point
/// during `[start, end]`.
pub async fn who_could_see(
    pool: &PgPool,
    org_id: OrgId,
    permission_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    request_id: &str,
) -> Result<Vec<EntitlementRow>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;
    let rows = fetch_could_see(&mut *tx, org_id, permission_id, start, end)
        .await
        .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(rows)
}
