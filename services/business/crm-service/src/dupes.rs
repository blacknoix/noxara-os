//! Duplicate detection for customers and leads.
//!
//! Two signals, both cheap and index-backed:
//! - **Exact email** match (case-insensitive) → score `1.0`.
//! - **Near name** match via `pg_trgm` `similarity() > 0.4` (also catches an
//!   `ILIKE '%name%'` substring match so short/partial names still recall).

use uuid::Uuid;

use crate::types::DuplicateMatch;

const NAME_SIMILARITY_THRESHOLD: f64 = 0.4;

/// Find customers that look like duplicates of `(name, email)`.
pub async fn find_customer_duplicates(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    name: &str,
    email: Option<&str>,
) -> Result<Vec<DuplicateMatch>, sqlx::Error> {
    let mut out: Vec<DuplicateMatch> = Vec::new();

    if let Some(email) = email.map(str::trim).filter(|e| !e.is_empty()) {
        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT public_id, name, email
            FROM sales_customer
            WHERE org_id = $1 AND deleted_at IS NULL AND lower(email) = lower($2)
            "#,
        )
        .bind(org_id)
        .bind(email)
        .fetch_all(&mut *conn)
        .await?;
        for (public_id, cname, cemail) in rows {
            out.push(DuplicateMatch {
                customer_id: Some(public_id),
                lead_id: None,
                name: cname,
                email: cemail,
                score: 1.0,
                reason: "exact_email".into(),
            });
        }
    }

    let trimmed_name = name.trim();
    if !trimmed_name.is_empty() {
        // `similarity()` returns `real` (FLOAT4) in Postgres; cast to `double
        // precision` so sqlx's `f64` decode doesn't choke on the type mismatch.
        let rows: Vec<(String, String, Option<String>, f64)> = sqlx::query_as(
            r#"
            SELECT public_id, name, email, similarity(name, $2)::float8 AS score
            FROM sales_customer
            WHERE org_id = $1 AND deleted_at IS NULL
              AND (similarity(name, $2) > $3 OR name ILIKE '%' || $2 || '%')
            ORDER BY score DESC
            LIMIT 10
            "#,
        )
        .bind(org_id)
        .bind(trimmed_name)
        .bind(NAME_SIMILARITY_THRESHOLD)
        .fetch_all(&mut *conn)
        .await?;
        for (public_id, cname, cemail, score) in rows {
            if out.iter().any(|m| m.customer_id.as_deref() == Some(public_id.as_str())) {
                continue;
            }
            out.push(DuplicateMatch {
                customer_id: Some(public_id),
                lead_id: None,
                name: cname,
                email: cemail,
                score,
                reason: "near_name".into(),
            });
        }
    }

    Ok(out)
}

/// Find leads that look like duplicates of `(name, email)` (excludes
/// `converted`/`disqualified` leads from the near-name scan so stale leads
/// don't generate noisy warnings, but always checks exact email).
pub async fn find_lead_duplicates(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    name: &str,
    email: Option<&str>,
) -> Result<Vec<DuplicateMatch>, sqlx::Error> {
    let mut out: Vec<DuplicateMatch> = Vec::new();

    if let Some(email) = email.map(str::trim).filter(|e| !e.is_empty()) {
        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT public_id, name, email
            FROM sales_lead
            WHERE org_id = $1 AND deleted_at IS NULL AND lower(email) = lower($2)
            "#,
        )
        .bind(org_id)
        .bind(email)
        .fetch_all(&mut *conn)
        .await?;
        for (public_id, lname, lemail) in rows {
            out.push(DuplicateMatch {
                customer_id: None,
                lead_id: Some(public_id),
                name: lname,
                email: lemail,
                score: 1.0,
                reason: "exact_email".into(),
            });
        }
    }

    let trimmed_name = name.trim();
    if !trimmed_name.is_empty() {
        let rows: Vec<(String, String, Option<String>, f64)> = sqlx::query_as(
            r#"
            SELECT public_id, name, email, similarity(name, $2)::float8 AS score
            FROM sales_lead
            WHERE org_id = $1 AND deleted_at IS NULL AND status NOT IN ('converted', 'disqualified')
              AND (similarity(name, $2) > $3 OR name ILIKE '%' || $2 || '%')
            ORDER BY score DESC
            LIMIT 10
            "#,
        )
        .bind(org_id)
        .bind(trimmed_name)
        .bind(NAME_SIMILARITY_THRESHOLD)
        .fetch_all(&mut *conn)
        .await?;
        for (public_id, lname, lemail, score) in rows {
            if out.iter().any(|m| m.lead_id.as_deref() == Some(public_id.as_str())) {
                continue;
            }
            out.push(DuplicateMatch {
                customer_id: None,
                lead_id: Some(public_id),
                name: lname,
                email: lemail,
                score,
                reason: "near_name".into(),
            });
        }
    }

    Ok(out)
}
