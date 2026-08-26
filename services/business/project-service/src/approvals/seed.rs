//! Default policy seeding for existing orgs (expense limits + quote discounts).

use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::PgPool;
use uuid::Uuid;

use super::types::{ApprovalMode, PolicyDefinition, PolicyMatch, PolicyStepDef};

fn expense_default_definition() -> PolicyDefinition {
    PolicyDefinition {
        mode: ApprovalMode::Any,
        match_criteria: PolicyMatch {
            // Finance decides WHETHER to request; this policy routes all expense approvals.
            amount_minor_gte: Some(1),
            ..Default::default()
        },
        steps: vec![PolicyStepDef {
            order: 1,
            approver_role: Some("finance".into()),
            approver_user_ids: vec![],
            sla_seconds: Some(86_400),
            escalate_to_role: Some("admin".into()),
        }],
    }
}

fn quote_discount_default_definition() -> PolicyDefinition {
    PolicyDefinition {
        mode: ApprovalMode::Any,
        match_criteria: PolicyMatch {
            // 10% document discount threshold (1000 bps).
            discount_bps_gte: Some(1_000),
            ..Default::default()
        },
        steps: vec![PolicyStepDef {
            order: 1,
            approver_role: Some("manager".into()),
            approver_user_ids: vec![],
            sla_seconds: Some(86_400),
            escalate_to_role: Some("admin".into()),
        }],
    }
}

/// Ensure default expense + quote_discount policies exist for an org.
pub async fn ensure_default_policies(pool: &PgPool, org_id: Uuid) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let org = companyos_tenancy::OrgId::new(org_id);
    set_session_org_id(&mut tx, org).await?;

    seed_one(
        &mut tx,
        org_id,
        "Default expense approval",
        "expense",
        &expense_default_definition(),
    )
    .await?;
    seed_one(
        &mut tx,
        org_id,
        "Default quote discount approval",
        "quote_discount",
        &quote_discount_default_definition(),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn seed_one(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    name: &str,
    subject_type: &str,
    def: &PolicyDefinition,
) -> anyhow::Result<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM operations_approval_policy
        WHERE org_id = $1 AND subject_type = $2 AND name = $3
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(subject_type)
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?;
    if exists.is_some() {
        return Ok(());
    }

    let public_id = PublicId::generate(IdKind::ApprovalPolicy);
    let policy_id = public_id.uuid();
    let version_id = new_uuid_v7();
    let def_json = serde_json::to_value(def)?;

    sqlx::query(
        r#"
        INSERT INTO operations_approval_policy (
            id, org_id, public_id, name, subject_type, is_active, current_version
        ) VALUES ($1,$2,$3,$4,$5,true,1)
        "#,
    )
    .bind(policy_id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(name)
    .bind(subject_type)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO operations_approval_policy_version (
            id, org_id, policy_id, version, definition_json, published_by
        ) VALUES ($1,$2,$3,1,$4,NULL)
        "#,
    )
    .bind(version_id)
    .bind(org_id)
    .bind(policy_id)
    .bind(def_json)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
