//! Last-Owner invariant: every org has ≥1 active Owner at all times.

use companyos_errors::{AppError, ErrorCode};
use uuid::Uuid;

pub async fn active_owner_count(
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

/// Returns Ok if the target membership can be revoked/suspended/demoted without
/// leaving the org with zero active owners.
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
            "cannot remove, suspend, or demote the last active Owner",
        ));
    }
    Ok(())
}
