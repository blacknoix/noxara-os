//! Gapless per-org invoice numbering (transactional).

use chrono::Datelike;
use uuid::Uuid;

/// Allocate the next invoice number for `org_id` inside an open transaction.
/// Format: `INV-{year}-{NNNNNN}` (zero-padded). Unique via
/// `finance_invoice (org_id, invoice_number)` plus this sequence row lock.
pub async fn next_invoice_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<String, sqlx::Error> {
    let year = chrono::Utc::now().year();
    sqlx::query(
        r#"
        INSERT INTO finance_invoice_seq (org_id, year, next_number)
        VALUES ($1, $2, 1)
        ON CONFLICT (org_id, year) DO NOTHING
        "#,
    )
    .bind(org_id)
    .bind(year)
    .execute(&mut **tx)
    .await?;

    let (n,): (i64,) = sqlx::query_as(
        r#"
        UPDATE finance_invoice_seq
        SET next_number = next_number + 1
        WHERE org_id = $1 AND year = $2
        RETURNING next_number - 1
        "#,
    )
    .bind(org_id)
    .bind(year)
    .fetch_one(&mut **tx)
    .await?;

    Ok(format!("INV-{year}-{n:06}"))
}

/// Allocate the next credit note number (reuses invoice seq with CN prefix
/// via a separate year key offset — stored as negative year marker is awkward,
/// so we use a dedicated convention: credit numbers `CN-{year}-{NNNNNN}`
/// from the same table with year = -year for credits… Prefer a second row
/// keyed by year+10000 for credits to avoid schema churn.
pub async fn next_credit_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<String, sqlx::Error> {
    let year = chrono::Utc::now().year();
    // Use year + 100_000 as a distinct sequence namespace for credit notes.
    let seq_year = year + 100_000;
    sqlx::query(
        r#"
        INSERT INTO finance_invoice_seq (org_id, year, next_number)
        VALUES ($1, $2, 1)
        ON CONFLICT (org_id, year) DO NOTHING
        "#,
    )
    .bind(org_id)
    .bind(seq_year)
    .execute(&mut **tx)
    .await?;

    let (n,): (i64,) = sqlx::query_as(
        r#"
        UPDATE finance_invoice_seq
        SET next_number = next_number + 1
        WHERE org_id = $1 AND year = $2
        RETURNING next_number - 1
        "#,
    )
    .bind(org_id)
    .bind(seq_year)
    .fetch_one(&mut **tx)
    .await?;

    Ok(format!("CN-{year}-{n:06}"))
}
