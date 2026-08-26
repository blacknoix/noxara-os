use axum::extract::{Query, State};
use axum::Json;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::PublicId;
use companyos_tenancy::{set_session_org_id, OrgId};
use serde::Deserialize;

use crate::auth::AuthCtx;
use crate::state::AppState;
use crate::types::{FactsResponse, InvoiceIssuedFact};

#[derive(Debug, Deserialize)]
pub struct FactsQuery {
    pub org_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/analytics/facts/invoice-issued",
    params(("org_id" = String, Query, description = "tenant org public id")),
    responses((status = 200, body = FactsResponse)),
    tag = "analytics"
)]
pub async fn invoice_issued(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<FactsQuery>,
) -> Result<Json<FactsResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_raw = q.org_id.as_deref().ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "org_id query parameter is required",
        )
    })?;
    let org = OrgId::from_public(
        &org_raw
            .parse::<PublicId>()
            .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid org_id"))?,
    )
    .map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "org_id must be org_…",
        )
    })?;

    if auth.ctx.org_id != org {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            &request_id,
            "org_id does not match authenticated tenant",
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        uuid::Uuid,
        uuid::Uuid,
        String,
        Option<i64>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT event_id, org_id, invoice_id, amount_minor, currency, issued_at
        FROM analytics_fact_invoice_issued
        WHERE org_id = $1
        ORDER BY issued_at DESC
        LIMIT 500
        "#,
    )
    .bind(org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, &request_id, e.to_string()))?;

    let facts = rows
        .into_iter()
        .map(
            |(event_id, org_id, invoice_id, amount_minor, currency, issued_at)| InvoiceIssuedFact {
                event_id: event_id.to_string(),
                org_id: OrgId::new(org_id).to_public().as_str(),
                invoice_id,
                amount_minor,
                currency,
                issued_at: issued_at.to_rfc3339(),
            },
        )
        .collect();

    Ok(Json(FactsResponse { facts }))
}
