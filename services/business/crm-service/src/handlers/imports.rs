//! `/api/v1/sales/imports/customers/{preview,confirm}` — CSV customer import.
//!
//! `preview` never writes to the database — it only parses + runs duplicate
//! detection so the UI can show the user what will happen. `confirm` runs the
//! actual inserts inside **one transaction**: either every row that isn't
//! explicitly skipped is committed, or (on any unexpected DB error) nothing
//! is. For very large files (~10k rows) a future iteration should chunk rows
//! into per-batch `SAVEPOINT`s so a mid-file failure can report a precise
//! row without re-running already-validated batches — but it must still only
//! ever *commit* atomically at the end; partial commits are not acceptable
//! for a bulk import a user might retry.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use sqlx::Postgres;
use uuid::Uuid;

use super::{internal, validation};
use crate::auth::AuthCtx;
use crate::dupes::find_customer_duplicates;
use crate::principal::{enforce, load_membership_scope};
use crate::state::AppState;
use crate::types::{
    ImportConfirmRequest, ImportConfirmResponse, ImportPreviewRequest, ImportPreviewResponse,
    ImportRowInput, ImportRowPreview,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sales/imports/customers/preview", post(preview_customers))
        .route("/api/v1/sales/imports/customers/confirm", post(confirm_customers))
}

fn field_synonyms(field: &str) -> &'static [&'static str] {
    match field {
        "name" => &["name", "full name", "contact name", "customer name"],
        "company" => &["company", "company name", "organization", "org"],
        "email" => &["email", "email address", "e-mail"],
        "phone" => &["phone", "phone number", "telephone", "mobile"],
        _ => &[],
    }
}

fn resolve_column(headers: &[String], mapping: &HashMap<String, String>, field: &str) -> Option<usize> {
    for (i, h) in headers.iter().enumerate() {
        if let Some(mapped_field) = mapping.get(h) {
            if mapped_field.eq_ignore_ascii_case(field) {
                return Some(i);
            }
        }
    }
    let syns = field_synonyms(field);
    headers
        .iter()
        .position(|h| syns.iter().any(|s| *s == h.trim().to_lowercase()))
}

/// Parse CSV text into `(name, email, phone, company)` rows using
/// header-alias resolution (explicit `mapping` overrides, then built-in
/// synonyms).
fn parse_csv_rows(csv_text: &str, mapping: &HashMap<String, String>) -> Result<Vec<ImportRowInput>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_text.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let name_idx = resolve_column(&headers, mapping, "name");
    let company_idx = resolve_column(&headers, mapping, "company");
    let email_idx = resolve_column(&headers, mapping, "email");
    let phone_idx = resolve_column(&headers, mapping, "phone");

    let get = |record: &csv::StringRecord, idx: Option<usize>| -> Option<String> {
        idx.and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let mut rows = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| e.to_string())?;
        rows.push(ImportRowInput {
            name: get(&record, name_idx),
            email: get(&record, email_idx),
            phone: get(&record, phone_idx),
            company: get(&record, company_idx),
        });
    }
    Ok(rows)
}

fn row_display_name(row: &ImportRowInput) -> Option<String> {
    row.company
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| row.name.clone().filter(|s| !s.trim().is_empty()))
}

/// POST /api/v1/sales/imports/customers/preview — no writes.
#[utoipa::path(post, path = "/api/v1/sales/imports/customers/preview", tag = "sales-imports",
    request_body = ImportPreviewRequest,
    responses((status = 200, body = ImportPreviewResponse)))]
pub async fn preview_customers(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ImportPreviewRequest>,
) -> Result<Json<ImportPreviewResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce(&membership.principal, perms::sales_import_create(), &request_id)?;

    let rows = parse_csv_rows(&body.csv, &body.mapping)
        .map_err(|e| validation(&request_id, format!("invalid csv: {e}")))?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut previews = Vec::with_capacity(rows.len());
    let mut exact = 0usize;
    let mut near = 0usize;
    for (i, row) in rows.iter().enumerate() {
        let mut errors = Vec::new();
        let display_name = row_display_name(row);
        if display_name.is_none() && row.email.is_none() {
            errors.push("row has neither a name/company nor an email".to_string());
        }
        let duplicates = match &display_name {
            Some(name) => find_customer_duplicates(&mut tx, org_id, name, row.email.as_deref())
                .await
                .map_err(internal(&request_id))?,
            None => Vec::new(),
        };
        for d in &duplicates {
            if d.reason == "exact_email" {
                exact += 1;
            } else {
                near += 1;
            }
        }
        previews.push(ImportRowPreview {
            row_number: (i + 1) as i32,
            name: row.name.clone(),
            email: row.email.clone(),
            phone: row.phone.clone(),
            company: row.company.clone(),
            duplicates,
            errors,
        });
    }
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(ImportPreviewResponse {
        rows: previews,
        exact_duplicate_count: exact,
        near_duplicate_count: near,
    }))
}

/// POST /api/v1/sales/imports/customers/confirm — single all-or-nothing
/// transaction (see module docs for the 10k-row scaling note).
#[utoipa::path(post, path = "/api/v1/sales/imports/customers/confirm", tag = "sales-imports",
    request_body = ImportConfirmRequest,
    responses((status = 201, body = ImportConfirmResponse)))]
pub async fn confirm_customers(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ImportConfirmRequest>,
) -> Result<(StatusCode, Json<ImportConfirmResponse>), AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();

    let membership =
        load_membership_scope(&state.pool, auth.ctx.org_id, auth.ctx.actor.user_id, &request_id)
            .await?;
    enforce(&membership.principal, perms::sales_import_create(), &request_id)?;

    let rows: Vec<ImportRowInput> = if let Some(rows) = body.rows.clone() {
        rows
    } else if let Some(csv_text) = body.csv.as_deref() {
        parse_csv_rows(csv_text, &body.mapping)
            .map_err(|e| validation(&request_id, format!("invalid csv: {e}")))?
    } else {
        return Err(validation(&request_id, "one of `csv` or `rows` is required"));
    };
    if rows.is_empty() {
        return Err(validation(&request_id, "no rows to import"));
    }

    let job_id = new_uuid_v7();
    let job_public = PublicId::new(IdKind::Import, job_id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(companyos_errors::ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO sales_import_job (id, org_id, public_id, status, filename, mapping, created_by)
        VALUES ($1,$2,$3,'confirmed',$4,$5,$6)
        "#,
    )
    .bind(job_id)
    .bind(org_id)
    .bind(job_public.as_str())
    .bind("inline")
    .bind(serde_json::to_value(&body.mapping).unwrap_or_default())
    .bind(auth.ctx.actor.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    let mut imported: i64 = 0;
    let mut skipped: i64 = 0;
    for row in &rows {
        let Some(name) = row_display_name(row) else {
            skipped += 1;
            continue;
        };
        if body.skip_exact_duplicates {
            let dupes = find_customer_duplicates(&mut tx, org_id, &name, row.email.as_deref())
                .await
                .map_err(internal(&request_id))?;
            if dupes.iter().any(|d| d.reason == "exact_email") {
                skipped += 1;
                continue;
            }
        }
        insert_customer(&mut tx, org_id, &name, row, auth.ctx.actor.user_id)
            .await
            .map_err(internal(&request_id))?;
        imported += 1;
    }

    tx.commit().await.map_err(internal(&request_id))?;

    Ok((
        StatusCode::CREATED,
        Json(ImportConfirmResponse {
            job_id: job_public.as_str(),
            imported,
            skipped,
        }),
    ))
}

async fn insert_customer(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    org_id: Uuid,
    name: &str,
    row: &ImportRowInput,
    owner_user_id: Uuid,
) -> Result<(), sqlx::Error> {
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Customer, id);
    sqlx::query(
        r#"
        INSERT INTO sales_customer (id, org_id, public_id, name, email, phone, owner_user_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(name)
    .bind(&row.email)
    .bind(&row.phone)
    .bind(owner_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
