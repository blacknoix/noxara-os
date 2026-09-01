use axum::extract::{Path, State};
use axum::Json;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::json;

use crate::auth::AuthCtx;
use crate::export::{to_csv, to_xlsx_tsv};
use crate::query::{execute_query, validate_query_for_tenant, ReportDefinition};
use crate::state::AppState;
use crate::types::{ExportRequest, ExportResponse};

use super::{authorize, internal, not_found, parse_id, set_org};

#[utoipa::path(post, path = "/api/v1/analytics/reports/{id}/export",
    tag = "analytics-reports", params(("id" = String, Path)),
    request_body = ExportRequest, responses((status = 200, body = ExportResponse)))]
pub async fn export_report(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<ExportRequest>,
) -> Result<Json<ExportResponse>, AppError> {
    let request_id = auth.ctx.request_id.as_str();
    let principal = authorize(&state, &auth, perms::analytics_report_export()).await?;
    let report_id = parse_id(IdKind::AnalyticsReport, &id, request_id)?;
    let format = body.format.to_ascii_lowercase();
    if format != "csv" && format != "xlsx" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "export format must be csv or xlsx",
        ));
    }
    let mut tx = state.pool.begin().await.map_err(internal(request_id))?;
    set_org(&mut tx, auth.ctx.org_id, request_id).await?;
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT definition FROM analytics_report WHERE org_id = $1 AND id = $2")
            .bind(auth.ctx.org_id.as_uuid())
            .bind(report_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(request_id))?;
    let (definition_json,) = row.ok_or_else(|| not_found(request_id, "report"))?;
    let mut definition: ReportDefinition = serde_json::from_value(definition_json)
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    let home = auth.ctx.region.unwrap_or(companyos_tenancy::RegionCode::Us);
    if definition.region.is_none() {
        definition.region = Some(home.as_str().into());
    }
    let validated = validate_query_for_tenant(&definition, home, request_id)?;
    if validated.org != auth.ctx.org_id {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "report org_id does not match authenticated tenant",
        ));
    }
    let result = execute_query(&mut tx, &validated, principal.as_ref(), false, request_id).await?;
    let (content, content_type, file_id) = if format == "xlsx" {
        (
            to_xlsx_tsv(&result),
            "text/tab-separated-values".to_string(),
            "inline:xlsx-tsv".to_string(),
        )
    } else {
        (
            to_csv(&result),
            "text/csv".to_string(),
            "inline:csv".to_string(),
        )
    };
    let run_id = new_uuid_v7();
    let run_public = PublicId::new(IdKind::AnalyticsRun, run_id).as_str();
    sqlx::query(
        "INSERT INTO analytics_run \
         (id, public_id, org_id, report_id, kind, status, started_by, finished_at, row_count, file_id) \
         VALUES ($1,$2,$3,$4,'export','completed',$5,now(),$6,$7)",
    )
    .bind(run_id)
    .bind(&run_public)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(report_id)
    .bind(auth.ctx.actor.on_behalf_of)
    .bind(i32::try_from(result.rows.len()).unwrap_or(i32::MAX))
    .bind(&file_id)
    .execute(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    let event = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Analytics,
        "export",
        "ready",
        1,
        auth.ctx.actor.clone(),
        json!({
            "report_id": id,
            "run_id": run_public,
            "format": format,
            "file_id": file_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &event)
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, request_id, error.to_string()))?;
    tx.commit().await.map_err(internal(request_id))?;
    Ok(Json(ExportResponse {
        run_id: run_public,
        report_id: id,
        format,
        content_type,
        file_id,
        content,
        row_count: result.rows.len(),
    }))
}
