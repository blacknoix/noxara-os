//! Account lockout persistence helpers.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

const MAX_FAILURES: i32 = 8;
const LOCK_MINUTES: i64 = 15;

#[allow(dead_code)]
pub async fn is_locked(pool: &PgPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<(Option<chrono::DateTime<Utc>>,)> =
        sqlx::query_as("SELECT locked_until FROM user_identity WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row
        .and_then(|(u,)| u)
        .is_some_and(|until| until > Utc::now()))
}

pub async fn record_failure(pool: &PgPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let (count,): (i32,) = sqlx::query_as(
        r#"
        UPDATE user_identity
        SET failed_login_count = failed_login_count + 1,
            locked_until = CASE
                WHEN failed_login_count + 1 >= $2 THEN now() + ($3::text || ' minutes')::interval
                ELSE locked_until
            END,
            updated_at = now()
        WHERE id = $1
        RETURNING failed_login_count
        "#,
    )
    .bind(user_id)
    .bind(MAX_FAILURES)
    .bind(LOCK_MINUTES.to_string())
    .fetch_one(pool)
    .await?;
    Ok(count >= MAX_FAILURES)
}

pub async fn clear_failures(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE user_identity
        SET failed_login_count = 0, locked_until = NULL, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub fn lock_duration() -> Duration {
    Duration::minutes(LOCK_MINUTES)
}

#[allow(dead_code)]
pub fn max_failures() -> i32 {
    MAX_FAILURES
}
