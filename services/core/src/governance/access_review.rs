//! Access review: who-could-see / who-did-see queries, and kickoff runs that
//! snapshot both into exportable findings.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_outbox::insert_event;
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::entitlement;
use super::types::{AccessReviewRunView, AuditReadRow, EntitlementRow};
use super::{internal, not_found, outbox_internal, tenancy_internal, validation};

/// Actions considered equivalent to `permission_id` for "who did" audit
/// queries. `hr.payroll.read` also covers payslip reads and the sensitive
/// employee-field read that payroll screens depend on.
fn related_actions(permission_id: &str) -> Vec<String> {
    if permission_id == "hr.payroll.read" {
        vec![
            "hr.payroll.read".to_string(),
            "hr.payslip.read".to_string(),
            "hr.employee.read_sensitive".to_string(),
        ]
    } else {
        vec![permission_id.to_string()]
    }
}

type AuditReadDbRow = (Uuid, String, String, String, String, DateTime<Utc>, Value);

async fn fetch_did_see<'e, E>(
    executor: E,
    org_id: OrgId,
    permission_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<AuditReadRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let actions = related_actions(permission_id);
    let rows: Vec<AuditReadDbRow> = sqlx::query_as(
        r#"
        SELECT a.actor_user_id, u.email, a.action, a.resource_type, a.resource_id,
               a.created_at, a.metadata
        FROM audit_entry a
        JOIN user_identity u ON u.id = a.actor_user_id
        WHERE a.org_id = $1
          AND a.action = ANY($2)
          AND a.created_at BETWEEN $3 AND $4
        ORDER BY a.created_at DESC
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(&actions)
    .bind(start)
    .bind(end)
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(user_id, email, action, resource_type, resource_id, created_at, metadata)| {
                AuditReadRow {
                    user_id: PublicId::new(IdKind::User, user_id).as_str(),
                    email,
                    action,
                    resource_type,
                    resource_id,
                    created_at: created_at.to_rfc3339(),
                    metadata,
                }
            },
        )
        .collect())
}

/// Who could see `permission_id` during `[start, end]` (entitlement history).
pub async fn who_could(
    pool: &PgPool,
    org_id: OrgId,
    permission_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    request_id: &str,
) -> Result<Vec<EntitlementRow>, AppError> {
    entitlement::who_could_see(pool, org_id, permission_id, start, end, request_id).await
}

/// Who did read/act on `permission_id` (or its related sensitive actions)
/// during `[start, end]` (audit log).
pub async fn who_did(
    pool: &PgPool,
    org_id: OrgId,
    permission_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    request_id: &str,
) -> Result<Vec<AuditReadRow>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;
    let rows = fetch_did_see(&mut *tx, org_id, permission_id, start, end)
        .await
        .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(rows)
}

fn user_uuid_from_public(request_id: &str, public_id: &str) -> Result<Uuid, AppError> {
    public_id
        .parse::<PublicId>()
        .map(|p| p.uuid())
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, format!("bad user id: {e}")))
}

/// Kick off a synchronous access-review run: snapshot could-see + did-see
/// findings plus a per-role summary, and emit `admin.access_review.completed.v1`.
///
/// Must run inside a transaction with `app.org_id` already bound (callers
/// set the session org id before invoking this, same as other mutations).
pub async fn kickoff_run(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    actor: Actor,
    permission_id: &str,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    request_id: &str,
) -> Result<AccessReviewRunView, AppError> {
    companyos_authz::validate_permission_id(permission_id)
        .map_err(|e| validation(request_id, e.to_string()))?;
    if period_end < period_start {
        return Err(validation(request_id, "period_end must be >= period_start"));
    }

    let run_uuid = new_uuid_v7();
    let public_id = PublicId::new(IdKind::AccessReview, run_uuid);

    let (created_at,): (DateTime<Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO access_review_run (
            id, org_id, public_id, status, permission_id, period_start, period_end,
            created_by, completed_at
        ) VALUES ($1,$2,$3,'completed',$4,$5,$6,$7, now())
        RETURNING created_at
        "#,
    )
    .bind(run_uuid)
    .bind(org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(permission_id)
    .bind(period_start)
    .bind(period_end)
    .bind(actor.user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let could_see =
        entitlement::fetch_could_see(&mut **tx, org_id, permission_id, period_start, period_end)
            .await
            .map_err(internal(request_id))?;
    let did_see = fetch_did_see(&mut **tx, org_id, permission_id, period_start, period_end)
        .await
        .map_err(internal(request_id))?;

    for row in &could_see {
        let user_uuid = user_uuid_from_public(request_id, &row.user_id)?;
        sqlx::query(
            r#"
            INSERT INTO access_review_finding (id, org_id, run_id, kind, user_id, role_key, permission_id, detail)
            VALUES ($1,$2,$3,'could_see',$4,$5,$6,$7)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(run_uuid)
        .bind(user_uuid)
        .bind(&row.role_key)
        .bind(permission_id)
        .bind(serde_json::to_value(row).unwrap_or_default())
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }

    for row in &did_see {
        let user_uuid = user_uuid_from_public(request_id, &row.user_id)?;
        sqlx::query(
            r#"
            INSERT INTO access_review_finding (id, org_id, run_id, kind, user_id, permission_id, detail)
            VALUES ($1,$2,$3,'did_see',$4,$5,$6)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(run_uuid)
        .bind(user_uuid)
        .bind(permission_id)
        .bind(serde_json::to_value(row).unwrap_or_default())
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }

    let mut role_summary: BTreeMap<String, i64> = BTreeMap::new();
    for row in &could_see {
        *role_summary.entry(row.role_key.clone()).or_insert(0) += 1;
    }
    for (role_key, count) in &role_summary {
        sqlx::query(
            r#"
            INSERT INTO access_review_finding (id, org_id, run_id, kind, role_key, permission_id, detail)
            VALUES ($1,$2,$3,'role_summary',$4,$5,$6)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(run_uuid)
        .bind(role_key)
        .bind(permission_id)
        .bind(serde_json::json!({ "could_see_count": count }))
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;
    }

    let summary = serde_json::json!({
        "could_see_count": could_see.len(),
        "did_see_count": did_see.len(),
        "role_summary": role_summary,
    });

    sqlx::query("UPDATE access_review_run SET summary = $2 WHERE id = $1")
        .bind(run_uuid)
        .bind(&summary)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;

    let envelope = EventEnvelope::new(
        org_id,
        Context::Admin,
        "access_review",
        "completed",
        1,
        actor,
        serde_json::json!({
            "run_id": public_id.as_str(),
            "permission_id": permission_id,
            "could_see_count": could_see.len(),
            "did_see_count": did_see.len(),
        }),
    );
    insert_event(&mut **tx, &envelope)
        .await
        .map_err(outbox_internal(request_id))?;

    Ok(AccessReviewRunView {
        id: public_id.as_str(),
        status: "completed".into(),
        permission_id: permission_id.to_string(),
        period_start: period_start.to_rfc3339(),
        period_end: period_end.to_rfc3339(),
        summary,
        created_at: created_at.to_rfc3339(),
        completed_at: Some(created_at.to_rfc3339()),
    })
}

type RunDbRow = (
    String,
    String,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
    Value,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub async fn get_run(
    pool: &PgPool,
    org_id: OrgId,
    run_public_id: &str,
    request_id: &str,
) -> Result<AccessReviewRunView, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let row: Option<RunDbRow> = sqlx::query_as(
        r#"
        SELECT public_id, status, permission_id, period_start, period_end, summary,
               created_at, completed_at
        FROM access_review_run
        WHERE org_id = $1 AND public_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(run_public_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    let Some((
        id,
        status,
        permission_id,
        period_start,
        period_end,
        summary,
        created_at,
        completed_at,
    )) = row
    else {
        return Err(not_found(request_id, "access review run"));
    };

    Ok(AccessReviewRunView {
        id,
        status,
        permission_id,
        period_start: period_start.to_rfc3339(),
        period_end: period_end.to_rfc3339(),
        summary,
        created_at: created_at.to_rfc3339(),
        completed_at: completed_at.map(|d| d.to_rfc3339()),
    })
}

type FindingRow = (
    String,
    Option<Uuid>,
    Option<String>,
    String,
    Value,
    DateTime<Utc>,
);

async fn fetch_findings(
    pool: &PgPool,
    org_id: OrgId,
    run_public_id: &str,
    request_id: &str,
) -> Result<Vec<FindingRow>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let run_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM access_review_run WHERE org_id = $1 AND public_id = $2")
            .bind(org_id.as_uuid())
            .bind(run_public_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(request_id))?;
    let Some((run_id,)) = run_id else {
        return Err(not_found(request_id, "access review run"));
    };

    let rows: Vec<FindingRow> = sqlx::query_as(
        r#"
        SELECT kind, user_id, role_key, permission_id, detail, created_at
        FROM access_review_finding
        WHERE org_id = $1 AND run_id = $2
        ORDER BY kind, created_at
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(rows)
}

pub async fn export_json(
    pool: &PgPool,
    org_id: OrgId,
    run_public_id: &str,
    request_id: &str,
) -> Result<Value, AppError> {
    let rows = fetch_findings(pool, org_id, run_public_id, request_id).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(
            |(kind, user_id, role_key, permission_id, detail, created_at)| {
                serde_json::json!({
                    "kind": kind,
                    "user_id": user_id.map(|u| PublicId::new(IdKind::User, u).as_str()),
                    "role_key": role_key,
                    "permission_id": permission_id,
                    "detail": detail,
                    "created_at": created_at.to_rfc3339(),
                })
            },
        )
        .collect();
    Ok(serde_json::json!({ "items": items }))
}

pub async fn export_csv(
    pool: &PgPool,
    org_id: OrgId,
    run_public_id: &str,
    request_id: &str,
) -> Result<String, AppError> {
    let rows = fetch_findings(pool, org_id, run_public_id, request_id).await?;
    // Build with push_str (not format!) so JSON detail braces cannot interact
    // with formatting machinery, and so an empty finding set is obvious.
    let mut csv = String::from("kind,user_id,role_key,permission_id,created_at,detail\n");
    for (kind, user_id, role_key, permission_id, detail, created_at) in rows {
        let user_id_str = user_id
            .map(|u| PublicId::new(IdKind::User, u).as_str())
            .unwrap_or_default();
        let role_key_str = role_key.unwrap_or_default().replace('"', "\"\"");
        let detail_str = detail.to_string().replace('"', "\"\"");
        csv.push_str(&kind);
        csv.push(',');
        csv.push_str(&user_id_str);
        csv.push(',');
        csv.push('"');
        csv.push_str(&role_key_str);
        csv.push('"');
        csv.push(',');
        csv.push_str(&permission_id);
        csv.push(',');
        csv.push_str(&created_at.to_rfc3339());
        csv.push(',');
        csv.push('"');
        csv.push_str(&detail_str);
        csv.push('"');
        csv.push('\n');
    }
    Ok(csv)
}
