//! Access revocation helpers for EmployeeOffboarding (shared DB, Workspace tables).
//!
//! HR does not own membership/sessions; this module mutates them only as the
//! explicit offboarding access-path revoke step (same pattern as core revoke).

use companyos_errors::{AppError, ErrorCode};
use uuid::Uuid;

use crate::types::AccessChecklistItem;

async fn active_owner_count(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM membership
        WHERE org_id = $1
          AND role = 'owner'
          AND status = 'active'
          AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(count)
}

/// Fail if offboarding would remove the last active Owner.
pub async fn ensure_not_last_owner(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    target_user_id: Uuid,
    request_id: &str,
) -> Result<(), AppError> {
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT role, status FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(target_user_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some((role, status)) = row else {
        return Ok(());
    };
    if role != "owner" || status != "active" {
        return Ok(());
    }
    let count = active_owner_count(conn, org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    if count <= 1 {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "cannot offboard the last active Owner — transfer ownership first",
        ));
    }
    Ok(())
}

/// Revoke membership + all org sessions for the linked user. Returns checklist.
pub async fn revoke_all_access_paths(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    user_id: Uuid,
    request_id: &str,
) -> Result<Vec<AccessChecklistItem>, AppError> {
    ensure_not_last_owner(conn, org_id, user_id, request_id).await?;

    let mut checklist = Vec::new();

    let mem: Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>, String)> = sqlx::query_as(
        r#"
        SELECT id, revoked_at, status FROM membership
        WHERE org_id = $1 AND user_id = $2
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    match mem {
        Some((_id, Some(_), _)) => {
            checklist.push(AccessChecklistItem {
                path: "membership".into(),
                cleared: true,
                detail: "membership already revoked".into(),
            });
        }
        Some((id, None, _)) => {
            sqlx::query(
                r#"
                UPDATE membership
                SET status = 'revoked', revoked_at = now(), updated_at = now(),
                    policy_version = policy_version + 1
                WHERE id = $1 AND org_id = $2
                "#,
            )
            .bind(id)
            .bind(org_id)
            .execute(&mut *conn)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            checklist.push(AccessChecklistItem {
                path: "membership".into(),
                cleared: true,
                detail: "membership revoked".into(),
            });
        }
        None => {
            checklist.push(AccessChecklistItem {
                path: "membership".into(),
                cleared: true,
                detail: "no membership row".into(),
            });
        }
    }

    let sessions: Result<u64, _> = async {
        let res = sqlx::query(
            r#"
            UPDATE auth_session
            SET revoked_at = COALESCE(revoked_at, now()),
                revoke_reason = COALESCE(revoke_reason, 'employee_offboarding')
            WHERE user_id = $1 AND org_id = $2 AND revoked_at IS NULL
            "#,
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *conn)
        .await?;
        Ok(res.rows_affected())
    }
    .await;
    let n = sessions
        .map_err(|e: sqlx::Error| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    checklist.push(AccessChecklistItem {
        path: "sessions".into(),
        cleared: true,
        detail: format!("revoked {n} active sessions"),
    });

    // Product API keys do not exist in Phase 2.1; record explicit N/A.
    checklist.push(AccessChecklistItem {
        path: "api_keys".into(),
        cleared: true,
        detail: "no product API-key entity in this system".into(),
    });
    checklist.push(AccessChecklistItem {
        path: "integration_tokens".into(),
        cleared: true,
        detail: "no integration tokens issued yet — reserved checklist path".into(),
    });

    Ok(checklist)
}

/// Prove no active membership/session remains for the user in this org.
pub async fn audit_access_cleared(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<AccessChecklistItem>, sqlx::Error> {
    let mem_active: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT FROM membership
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL AND status = 'active'
        "#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;

    let sess_active: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT FROM auth_session
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;

    Ok(vec![
        AccessChecklistItem {
            path: "membership".into(),
            cleared: mem_active.0 == 0,
            detail: format!("active memberships: {}", mem_active.0),
        },
        AccessChecklistItem {
            path: "sessions".into(),
            cleared: sess_active.0 == 0,
            detail: format!("active sessions: {}", sess_active.0),
        },
        AccessChecklistItem {
            path: "api_keys".into(),
            cleared: true,
            detail: "no product API-key entity".into(),
        },
        AccessChecklistItem {
            path: "integration_tokens".into(),
            cleared: true,
            detail: "no integration tokens".into(),
        },
    ])
}
