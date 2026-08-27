//! `/api/v1/people/employees/{id}/documents` — HR document metadata (opaque fil_ ids).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use uuid::Uuid;

use super::employees::{enforce_employee_scope, fetch_employee_row, parse_optional_date};
use super::{internal, not_found, parse_public_id, validation};
use crate::audit::insert_audit;
use crate::auth::AuthCtx;
use crate::idempotency;
use crate::principal::{enforce_any_scope, load_membership_scope};
use crate::state::AppState;
use crate::types::{CreateDocumentRequest, DocumentDto, DocumentListResponse};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/people/employees/{id}/documents",
        get(list_documents).post(create_document),
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DocumentRow {
    #[allow(dead_code)]
    id: Uuid,
    public_id: String,
    #[allow(dead_code)]
    employee_id: Uuid,
    title: String,
    doc_type: String,
    file_id: Option<String>,
    expires_at: Option<NaiveDate>,
    collected: bool,
    created_at: DateTime<Utc>,
    version: i32,
}

impl DocumentRow {
    fn into_dto(self, employee_public: &str) -> DocumentDto {
        DocumentDto {
            id: self.public_id,
            employee_id: employee_public.to_string(),
            title: self.title,
            doc_type: self.doc_type,
            file_id: self.file_id,
            expires_at: self.expires_at.map(|d| d.to_string()),
            collected: self.collected,
            created_at: self.created_at.to_rfc3339(),
            version: self.version,
        }
    }
}

fn validate_file_id(raw: Option<&str>, request_id: &str) -> Result<Option<String>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            let s = s.trim();
            if !s.starts_with("fil_") {
                return Err(validation(
                    request_id,
                    "file_id must be an opaque fil_… identifier",
                ));
            }
            Ok(Some(s.to_string()))
        }
    }
}

/// GET /api/v1/people/employees/{id}/documents
#[utoipa::path(
    get,
    path = "/api/v1/people/employees/{id}/documents",
    tag = "people-documents",
    params(("id" = String, Path)),
    responses((status = 200, body = DocumentListResponse), (status = 404))
)]
pub async fn list_documents(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<DocumentListResponse>, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_document_read(),
        &request_id,
    )?;

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    // Scope check against employee owner using document read (org-wide for role defaults).
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_document_read(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    let rows: Vec<DocumentRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, employee_id, title, doc_type, file_id,
               expires_at, collected, created_at, version
        FROM people_document
        WHERE org_id = $1 AND employee_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(org_id)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;
    Ok(Json(DocumentListResponse {
        items: rows
            .into_iter()
            .map(|r| r.into_dto(&emp.public_id))
            .collect(),
    }))
}

/// POST /api/v1/people/employees/{id}/documents
#[utoipa::path(
    post,
    path = "/api/v1/people/employees/{id}/documents",
    tag = "people-documents",
    request_body = CreateDocumentRequest,
    params(("id" = String, Path)),
    responses((status = 201, body = DocumentDto))
)]
pub async fn create_document(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreateDocumentRequest>,
) -> Result<Response, AppError> {
    let request_id = auth.ctx.request_id.clone();
    let org_id = auth.ctx.org_id.as_uuid();
    let employee_id = parse_public_id(IdKind::Employee, &id, &request_id)?;
    let idem_key = idempotency::header_key(&headers);

    let membership = load_membership_scope(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.user_id,
        &request_id,
    )
    .await?;
    enforce_any_scope(
        &membership.principal,
        perms::hr_document_write(),
        &request_id,
    )?;

    if body.title.trim().is_empty() {
        return Err(validation(&request_id, "title must not be empty"));
    }
    let doc_type = body.doc_type.as_deref().unwrap_or("other").to_string();
    let file_id = validate_file_id(body.file_id.as_deref(), &request_id)?;
    let expires_at = parse_optional_date(body.expires_at.as_deref(), "expires_at", &request_id)?;
    let collected = file_id.is_some();

    let public_id = PublicId::generate(IdKind::EmployeeDocument);
    let id_uuid = public_id.uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, auth.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if let Some(key) = idem_key.as_deref() {
        if let Some((status_code, stored)) =
            idempotency::get(&mut *tx, org_id, "document.create", key)
                .await
                .map_err(internal(&request_id))?
        {
            tx.commit().await.map_err(internal(&request_id))?;
            let code = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::CREATED);
            return Ok((code, Json(stored)).into_response());
        }
    }

    let emp = fetch_employee_row(&mut tx, org_id, employee_id)
        .await
        .map_err(internal(&request_id))?
        .ok_or_else(|| not_found(&request_id, "employee"))?;
    enforce_employee_scope(
        &mut tx,
        org_id,
        &auth,
        &membership,
        perms::hr_document_write(),
        emp.owner_user_id,
        &request_id,
    )
    .await?;

    sqlx::query(
        r#"
        INSERT INTO people_document (
            id, org_id, public_id, employee_id, title, doc_type, file_id, expires_at, collected
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id_uuid)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(employee_id)
    .bind(body.title.trim())
    .bind(&doc_type)
    .bind(&file_id)
    .bind(expires_at)
    .bind(collected)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    insert_audit(
        &mut *tx,
        org_id,
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "hr.document.create",
        "document",
        &public_id.as_str(),
        serde_json::json!({ "employee_id": emp.public_id, "doc_type": doc_type }),
    )
    .await
    .map_err(internal(&request_id))?;

    let dto = DocumentDto {
        id: public_id.as_str(),
        employee_id: emp.public_id,
        title: body.title.trim().to_string(),
        doc_type,
        file_id,
        expires_at: expires_at.map(|d| d.to_string()),
        collected,
        created_at: Utc::now().to_rfc3339(),
        version: 1,
    };

    if let Some(key) = idem_key.as_deref() {
        idempotency::put(
            &mut *tx,
            org_id,
            "document.create",
            key,
            201,
            serde_json::to_value(&dto).unwrap_or_default(),
        )
        .await
        .map_err(internal(&request_id))?;
    }

    tx.commit().await.map_err(internal(&request_id))?;
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}
