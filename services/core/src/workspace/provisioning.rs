//! Durable OrgProvisioning command.
//!
//! Idempotent key: `{org_public_id}:OrgProvisioning:{org_public_id}`
//!
//! Temporal workers are not wired in this repo yet — see ADR 017. This module
//! implements the same durable command semantics so provisioning is not a
//! status-column + cron fake.

use companyos_authz::{Effect, Role, PERMISSION_CATALOGUE};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use uuid::Uuid;

pub fn provisioning_idempotency_key(org_public_id: &str) -> String {
    format!("{org_public_id}:OrgProvisioning:{org_public_id}")
}

/// Enqueue (or no-op if exists) and immediately process OrgProvisioning.
pub async fn enqueue_and_run(
    pool: &sqlx::PgPool,
    org_id: OrgId,
    owner_user_id: Uuid,
    business_type: &str,
    request_id: &str,
) -> Result<(), AppError> {
    let org_public = org_id.to_public().as_str();
    let key = provisioning_idempotency_key(&org_public);
    let cmd_id = new_uuid_v7();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO workspace_command (
            id, org_id, command_type, idempotency_key, payload, status
        ) VALUES ($1, $2, 'OrgProvisioning', $3, $4, 'pending')
        ON CONFLICT (idempotency_key) DO NOTHING
        "#,
    )
    .bind(cmd_id)
    .bind(org_id.as_uuid())
    .bind(&key)
    .bind(serde_json::json!({
        "owner_user_id": owner_user_id,
        "business_type": business_type,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    process_pending(pool, org_id, request_id).await
}

/// Process pending/failed OrgProvisioning for an org (idempotent).
pub async fn process_pending(
    pool: &sqlx::PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let cmd: Option<(Uuid, serde_json::Value, String)> = sqlx::query_as(
        r#"
        SELECT id, payload, status FROM workspace_command
        WHERE org_id = $1 AND command_type = 'OrgProvisioning'
          AND status IN ('pending', 'failed', 'running')
        ORDER BY created_at ASC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((cmd_id, payload, status)) = cmd else {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return Ok(());
    };
    if status == "completed" {
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return Ok(());
    }

    sqlx::query(
        r#"
        UPDATE workspace_command
        SET status = 'running', attempts = attempts + 1, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(cmd_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let owner_user_id = payload.get("owner_user_id").and_then(|v| {
        v.as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .or_else(|| serde_json::from_value::<Uuid>(v.clone()).ok())
    });
    let Some(owner_user_id) = owner_user_id else {
        sqlx::query(
            r#"
            UPDATE workspace_command
            SET status = 'failed', last_error = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(cmd_id)
        .bind("missing owner_user_id")
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            "OrgProvisioning failed: missing owner_user_id",
        ));
    };
    let business_type = payload
        .get("business_type")
        .and_then(|v| v.as_str())
        .unwrap_or("general");

    if let Err(e) = seed_workspace(&mut tx, org_id, owner_user_id, business_type).await {
        sqlx::query(
            r#"
            UPDATE workspace_command
            SET status = 'failed', last_error = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(cmd_id)
        .bind(e.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            format!("OrgProvisioning failed: {e}"),
        ));
    }

    sqlx::query(
        r#"
        UPDATE workspace_command
        SET status = 'completed', completed_at = now(), updated_at = now(), last_error = NULL
        WHERE id = $1
        "#,
    )
    .bind(cmd_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(())
}

async fn seed_workspace(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: OrgId,
    owner_user_id: Uuid,
    business_type: &str,
) -> anyhow::Result<()> {
    // Seed system roles + default role_permission matrix.
    for role in Role::all_system() {
        let role_uuid = new_uuid_v7();
        let public_id = PublicId::new(IdKind::Role, role_uuid);
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            r#"
            INSERT INTO org_role (
                id, org_id, public_id, name, description, system_key, is_system
            ) VALUES ($1,$2,$3,$4,$5,$6,true)
            ON CONFLICT (org_id, system_key) DO UPDATE SET name = EXCLUDED.name
            RETURNING id
            "#,
        )
        .bind(role_uuid)
        .bind(org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(role.display_name())
        .bind(format!("System role: {}", role.display_name()))
        .bind(role.as_str())
        .fetch_optional(&mut **tx)
        .await?;

        let rid = if let Some((id,)) = inserted {
            id
        } else {
            let (id,): (Uuid,) =
                sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 AND system_key = $2")
                    .bind(org_id.as_uuid())
                    .bind(role.as_str())
                    .fetch_one(&mut **tx)
                    .await?;
            id
        };

        // Clear and re-seed permissions for system role (idempotent).
        sqlx::query("DELETE FROM role_permission WHERE role_id = $1")
            .bind(rid)
            .execute(&mut **tx)
            .await?;

        let allows = role.default_allows();
        for p in PERMISSION_CATALOGUE {
            let effect = if allows.contains(&p.permission_id()) {
                Effect::Allow
            } else {
                continue; // deny-by-default; no row needed
            };
            let _ = effect;
            sqlx::query(
                r#"
                INSERT INTO role_permission (id, org_id, role_id, permission_id, effect, scope)
                VALUES ($1,$2,$3,$4,'allow','organization')
                ON CONFLICT (role_id, permission_id, effect) DO NOTHING
                "#,
            )
            .bind(new_uuid_v7())
            .bind(org_id.as_uuid())
            .bind(rid)
            .bind(p.id)
            .execute(&mut **tx)
            .await?;
        }

        if *role == Role::Owner {
            // Bind owner membership to owner role if present.
            sqlx::query(
                r#"
                UPDATE membership
                SET role_id = $1, role = 'owner', status = 'active', updated_at = now()
                WHERE org_id = $2 AND user_id = $3
                "#,
            )
            .bind(rid)
            .bind(org_id.as_uuid())
            .bind(owner_user_id)
            .execute(&mut **tx)
            .await?;

            // Seed entitlement history so access-review who-could queries work.
            let mem: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM membership WHERE org_id = $1 AND user_id = $2")
                    .bind(org_id.as_uuid())
                    .bind(owner_user_id)
                    .fetch_optional(&mut **tx)
                    .await?;
            if let Some((membership_id,)) = mem {
                let perms: Vec<(String, String)> = allows
                    .iter()
                    .map(|p| (p.as_str().to_string(), "allow".to_string()))
                    .collect();
                crate::governance::entitlement::record_entitlements_for_membership(
                    &mut *tx,
                    org_id,
                    owner_user_id,
                    membership_id,
                    "owner",
                    &perms,
                    chrono::Utc::now(),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            }
        }
    }

    // Default numbering series / seed_defaults JSON
    let (stages, expenses): (Vec<&str>, Vec<&str>) = match business_type {
        "agency" => (
            vec!["Lead", "Discovery", "Proposal", "Won", "Lost"],
            vec!["Travel", "Software", "Contractors", "Office"],
        ),
        "retail" => (
            vec!["Inquiry", "Quote", "Order", "Fulfilled", "Closed"],
            vec!["Inventory", "Shipping", "Marketing", "Utilities"],
        ),
        _ => (
            vec!["New", "Qualified", "Proposal", "Negotiation", "Won", "Lost"],
            vec!["General", "Travel", "Meals", "Software", "Other"],
        ),
    };

    sqlx::query(
        r#"
        UPDATE organization SET
            business_type = $2,
            numbering_series = COALESCE(NULLIF(numbering_series, '{}'::jsonb),
                '{"invoice":"INV-","quote":"Q-","deal":"D-"}'::jsonb),
            seed_defaults = $3,
            feature_flags = COALESCE(NULLIF(feature_flags, '{}'::jsonb),
                '{"sso":false,"ai_copilot":false}'::jsonb),
            plan = COALESCE(NULLIF(plan, ''), 'starter'),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(business_type)
    .bind(serde_json::json!({
        "pipeline_stages": stages,
        "expense_categories": expenses,
    }))
    .execute(&mut **tx)
    .await?;

    for (i, name) in stages.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO workspace_seed_pipeline_stage (id, org_id, name, position)
            VALUES ($1,$2,$3,$4)
            ON CONFLICT (org_id, name) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(*name)
        .bind(i as i32)
        .execute(&mut **tx)
        .await?;
    }
    for name in expenses {
        sqlx::query(
            r#"
            INSERT INTO workspace_seed_expense_category (id, org_id, name)
            VALUES ($1,$2,$3)
            ON CONFLICT (org_id, name) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id.as_uuid())
        .bind(name)
        .execute(&mut **tx)
        .await?;
    }

    // Ensure at least one owner membership exists.
    let owners = super::last_owner::active_owner_count(tx, org_id.as_uuid()).await?;
    if owners < 1 {
        anyhow::bail!("OrgProvisioning left org without an active Owner");
    }

    let envelope = EventEnvelope::new(
        org_id,
        Context::Workspace,
        "organization",
        "provisioned",
        1,
        Actor::human(owner_user_id),
        serde_json::json!({
            "business_type": business_type,
            "roles_seeded": Role::all_system().len(),
        }),
    );
    companyos_outbox::insert_event(&mut **tx, &envelope).await?;

    Ok(())
}
