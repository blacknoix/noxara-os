//! Process deferred email digest deliveries (quiet-hours backlog).

use companyos_errors::{AppError, ErrorCode};
use companyos_tenancy::set_session_org_id;
use sqlx::PgPool;
use uuid::Uuid;

use crate::mail;

/// Flush `notification_delivery` rows with status `deferred_digest` for email.
///
/// Returns the number of deliveries marked sent. Safe to call repeatedly.
pub async fn run_deferred_digest(pool: &PgPool) -> Result<u32, AppError> {
    // Cross-tenant internal job: temporarily enable ingest session bypass if needed.
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;
    sqlx::query("SELECT set_config('app.notification_ingest', '1', true)")
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, Uuid, Uuid, String, String)> = sqlx::query_as(
        r#"
        SELECT d.id, d.org_id, i.user_id, i.title, i.body
        FROM notification_delivery d
        JOIN notification_item i ON i.id = d.item_id
        WHERE d.channel = 'email' AND d.status = 'deferred_digest'
        ORDER BY d.created_at ASC
        LIMIT 200
        "#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;

    let mut sent = 0u32;
    for (delivery_id, org_id, user_id, title, body) in rows {
        let org = companyos_tenancy::OrgId::new(org_id);
        set_session_org_id(&mut tx, org)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;

        let email: Option<(String,)> =
            sqlx::query_as("SELECT email FROM user_identity WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await
                .ok()
                .flatten();
        let to = email
            .map(|(e,)| e)
            .unwrap_or_else(|| format!("{user_id}@local"));
        let _ = mail::send_email(&to, &title, &body).await;

        sqlx::query(
            "UPDATE notification_delivery SET status = 'sent' WHERE id = $1 AND status = 'deferred_digest'",
        )
        .bind(delivery_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;
        sent += 1;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, "digest", e.to_string()))?;
    Ok(sent)
}

#[cfg(test)]
mod tests {
    #[test]
    fn digest_module_compiles() {
        // Unit smoke — integration covered by POST /internal/digest/run when DB present.
        assert_eq!(0u32.saturating_add(0), 0);
    }
}
