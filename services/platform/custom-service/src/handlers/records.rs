//! `/api/v1/custom/records/{slug}` — custom record CRUD with formulas + scripts.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    conflict, enforce_opt, internal, load_authz_principal, not_found, parse_public_id, set_org,
    validation,
};
use crate::auth::AuthCtx;
use crate::formula::apply_formulas;
use crate::sandbox::{execute, parse_program, Limits, RecordHost};
use crate::search_doc::{build_search_text, search_document};
use crate::state::AppState;
use crate::types::{CustomRecordDto, FieldDef, FieldType, UpsertRecordRequest};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/custom/records/{slug}",
            get(list_records).post(create_record),
        )
        .route(
            "/api/v1/custom/records/{slug}/{id}",
            get(get_record).patch(update_record).delete(delete_record),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecordListResponse {
    pub items: Vec<CustomRecordDto>,
}

type RecordRow = (String, String, Value, i32, DateTime<Utc>, DateTime<Utc>);

fn require_if_match(headers: &HeaderMap, request_id: &str) -> Result<i32, AppError> {
    headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().trim_matches('"').parse::<i32>().ok())
        .ok_or_else(|| {
            validation(
                request_id,
                "If-Match header with integer version is required",
            )
        })
}

async fn load_published_entity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    slug: &str,
    rid: &str,
) -> Result<(Uuid, Vec<FieldDef>), AppError> {
    let row: Option<(Uuid, Value)> = sqlx::query_as(
        r#"
        SELECT id, fields
        FROM custom_entity_definition
        WHERE org_id = $1 AND slug = $2 AND status = 'published' AND deleted_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(slug)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(rid))?;
    let (entity_id, fields_json) = row.ok_or_else(|| not_found(rid, "published entity"))?;
    let fields: Vec<FieldDef> = serde_json::from_value(fields_json)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, format!("fields json: {e}")))?;
    Ok((entity_id, fields))
}

fn validate_values(
    fields: &[FieldDef],
    values: &Map<String, Value>,
    rid: &str,
) -> Result<(), AppError> {
    let known: std::collections::HashSet<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    for key in values.keys() {
        if !known.contains(key.as_str()) {
            return Err(validation(rid, format!("unknown field '{key}'")));
        }
    }
    for f in fields {
        if f.field_type == FieldType::Formula {
            continue;
        }
        let present = values.get(&f.name).filter(|v| !v.is_null());
        if f.required && present.is_none() {
            return Err(validation(rid, format!("field '{}' is required", f.name)));
        }
        let Some(v) = present else {
            continue;
        };
        match f.field_type {
            FieldType::Text | FieldType::Date | FieldType::Ref => {
                if !v.is_string() {
                    return Err(validation(
                        rid,
                        format!("field '{}' must be a string", f.name),
                    ));
                }
            }
            FieldType::Number => {
                if v.as_i64().is_none() {
                    return Err(validation(
                        rid,
                        format!("field '{}' must be an integer", f.name),
                    ));
                }
            }
            FieldType::Bool => {
                if !v.is_boolean() {
                    return Err(validation(
                        rid,
                        format!("field '{}' must be a boolean", f.name),
                    ));
                }
            }
            FieldType::Select => {
                let s = v.as_str().ok_or_else(|| {
                    validation(rid, format!("field '{}' must be a string option", f.name))
                })?;
                if let Some(opts) = &f.options {
                    if !opts.iter().any(|o| o == s) {
                        return Err(validation(
                            rid,
                            format!("field '{}' value not in options", f.name),
                        ));
                    }
                }
            }
            FieldType::Money => {
                validate_money(v, &f.name, rid)?;
            }
            FieldType::Formula => {}
        }
    }
    Ok(())
}

fn validate_money(v: &Value, name: &str, rid: &str) -> Result<(), AppError> {
    let obj = v.as_object().ok_or_else(|| {
        validation(
            rid,
            format!("field '{name}' must be {{amount_minor: i64, currency: String}}"),
        )
    })?;
    let amount = obj
        .get("amount_minor")
        .ok_or_else(|| validation(rid, format!("field '{name}' missing amount_minor")))?;
    if amount.as_i64().is_none() {
        return Err(validation(
            rid,
            format!("field '{name}' amount_minor must be an integer (no floats)"),
        ));
    }
    let currency = obj
        .get("currency")
        .and_then(|c| c.as_str())
        .ok_or_else(|| validation(rid, format!("field '{name}' currency must be a string")))?;
    if currency.len() != 3 {
        return Err(validation(
            rid,
            format!("field '{name}' currency must be a 3-letter code"),
        ));
    }
    if obj.keys().any(|k| k != "amount_minor" && k != "currency") {
        return Err(validation(
            rid,
            format!("field '{name}' money object may only contain amount_minor and currency"),
        ));
    }
    Ok(())
}

async fn load_script(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    slug: &str,
    hook: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT source FROM custom_script
        WHERE org_id = $1 AND entity_slug = $2 AND hook = $3 AND enabled = true
        "#,
    )
    .bind(org_id)
    .bind(slug)
    .bind(hook)
    .fetch_optional(&mut **tx)
    .await
}

fn run_script(source: &str, values: &mut Map<String, Value>, rid: &str) -> Result<(), AppError> {
    let program = parse_program(source).map_err(|e| {
        AppError::new(
            ErrorCode::ValidationFailed,
            rid,
            format!("script parse failed: {e}"),
        )
    })?;
    let limits = Limits::default();
    let mut host = RecordHost::new(values.clone(), limits.max_bytes);
    execute(&program, &mut host, limits).map_err(|e| {
        // Fail-closed — caller must not persist. Status maps via ValidationFailed (400);
        // detail makes the script failure explicit for clients/tests expecting reject-on-script-error.
        AppError::new(
            ErrorCode::ValidationFailed,
            rid,
            format!("script failed (fail-closed): {e}"),
        )
    })?;
    *values = host.values;
    Ok(())
}

fn to_dto(row: RecordRow) -> CustomRecordDto {
    CustomRecordDto {
        id: row.0,
        entity_slug: row.1,
        values: row.2,
        version: row.3,
        created_at: row.4,
        updated_at: row.5,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/records/{slug}",
    tag = "custom-records",
    params(("slug" = String, Path, description = "Published entity slug")),
    responses((status = 200, body = RecordListResponse))
)]
pub async fn list_records(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
) -> Result<Json<RecordListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_entity_read(&slug), rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let _ = load_published_entity(&mut tx, auth.ctx.org_id.as_uuid(), &slug, rid).await?;

    let rows: Vec<RecordRow> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, "values", version, created_at, updated_at
        FROM custom_record
        WHERE org_id = $1 AND entity_slug = $2 AND deleted_at IS NULL
        ORDER BY updated_at DESC
        LIMIT 200
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&slug)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(RecordListResponse {
        items: rows.into_iter().map(to_dto).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/records/{slug}",
    tag = "custom-records",
    request_body = UpsertRecordRequest,
    responses((status = 201, body = CustomRecordDto))
)]
pub async fn create_record(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(slug): Path<String>,
    Json(body): Json<UpsertRecordRequest>,
) -> Result<(StatusCode, Json<CustomRecordDto>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_entity_write(&slug), rid)?;

    let mut values = body.values;
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::CustomRecord, id);
    let actor = auth.ctx.actor.on_behalf_of;
    let org_public = auth.ctx.org_id.to_public().as_str();

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let (entity_id, fields) =
        load_published_entity(&mut tx, auth.ctx.org_id.as_uuid(), &slug, rid).await?;
    validate_values(&fields, &values, rid)?;
    apply_formulas(&fields, &mut values)
        .map_err(|e| validation(rid, format!("formula error: {e}")))?;

    if let Some(src) = load_script(&mut tx, auth.ctx.org_id.as_uuid(), &slug, "before_save")
        .await
        .map_err(internal(rid))?
    {
        run_script(&src, &mut values, rid)?;
    }

    let search_text = build_search_text(&values);
    let values_json = Value::Object(values.clone());

    sqlx::query(
        r#"
        INSERT INTO custom_record (
            id, org_id, public_id, entity_id, entity_slug, "values", search_text,
            created_by, updated_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8)
        "#,
    )
    .bind(id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(entity_id)
    .bind(&slug)
    .bind(&values_json)
    .bind(&search_text)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    if let Some(src) = load_script(&mut tx, auth.ctx.org_id.as_uuid(), &slug, "after_save")
        .await
        .map_err(internal(rid))?
    {
        run_script(&src, &mut values, rid)?;
        let search_text = build_search_text(&values);
        let values_json = Value::Object(values.clone());
        sqlx::query(
            r#"
            UPDATE custom_record SET "values" = $3, search_text = $4, updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(id)
        .bind(&values_json)
        .bind(&search_text)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
    }

    let values_json = Value::Object(values.clone());
    let search_text = build_search_text(&values);
    let search_doc = search_document(
        &org_public,
        &slug,
        &public_id.as_str(),
        &values_json,
        &search_text,
    );

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Custom,
        slug.clone(),
        "created",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id.as_str(),
            "record_id": public_id.as_str(),
            "entity_slug": slug,
            "search_text": search_text,
            "search_doc": search_doc,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.record.create",
        "custom_record",
        &public_id.as_str(),
        serde_json::json!({ "entity_slug": slug }),
    )
    .await
    .map_err(internal(rid))?;

    let now = Utc::now();
    tx.commit().await.map_err(internal(rid))?;

    Ok((
        StatusCode::CREATED,
        Json(CustomRecordDto {
            id: public_id.as_str(),
            entity_slug: slug,
            values: values_json,
            version: 1,
            created_at: now,
            updated_at: now,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/records/{slug}/{id}",
    tag = "custom-records",
    responses((status = 200, body = CustomRecordDto), (status = 404))
)]
pub async fn get_record(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((slug, id)): Path<(String, String)>,
) -> Result<Json<CustomRecordDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_entity_read(&slug), rid)?;
    let record_id = parse_public_id(IdKind::CustomRecord, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let _ = load_published_entity(&mut tx, auth.ctx.org_id.as_uuid(), &slug, rid).await?;

    let row: Option<RecordRow> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, "values", version, created_at, updated_at
        FROM custom_record
        WHERE org_id = $1 AND id = $2 AND entity_slug = $3 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(to_dto(row.ok_or_else(|| not_found(rid, "record"))?)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/custom/records/{slug}/{id}",
    tag = "custom-records",
    request_body = UpsertRecordRequest,
    responses((status = 200, body = CustomRecordDto), (status = 409))
)]
pub async fn update_record(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<UpsertRecordRequest>,
) -> Result<Json<CustomRecordDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_entity_write(&slug), rid)?;
    let record_id = parse_public_id(IdKind::CustomRecord, &id, rid)?;
    let expected_version = require_if_match(&headers, rid)?;
    let org_public = auth.ctx.org_id.to_public().as_str();

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let (_entity_id, fields) =
        load_published_entity(&mut tx, auth.ctx.org_id.as_uuid(), &slug, rid).await?;

    let existing: Option<(String, Value, i32)> = sqlx::query_as(
        r#"
        SELECT public_id, "values", version FROM custom_record
        WHERE org_id = $1 AND id = $2 AND entity_slug = $3 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let (public_id, existing_values, current_version) =
        existing.ok_or_else(|| not_found(rid, "record"))?;

    if expected_version != current_version {
        return Err(conflict(
            rid,
            format!("version mismatch: expected {expected_version}, current {current_version}"),
        ));
    }

    let mut values = existing_values.as_object().cloned().unwrap_or_default();
    for (k, v) in body.values {
        values.insert(k, v);
    }

    validate_values(&fields, &values, rid)?;
    apply_formulas(&fields, &mut values)
        .map_err(|e| validation(rid, format!("formula error: {e}")))?;

    if let Some(src) = load_script(&mut tx, auth.ctx.org_id.as_uuid(), &slug, "before_save")
        .await
        .map_err(internal(rid))?
    {
        run_script(&src, &mut values, rid)?;
    }

    let search_text = build_search_text(&values);
    let values_json = Value::Object(values.clone());

    let updated = sqlx::query(
        r#"
        UPDATE custom_record
        SET "values" = $4, search_text = $5, updated_by = $6,
            version = version + 1, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND entity_slug = $3
          AND deleted_at IS NULL AND version = $7
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .bind(&slug)
    .bind(&values_json)
    .bind(&search_text)
    .bind(auth.ctx.actor.on_behalf_of)
    .bind(expected_version)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    if updated.rows_affected() == 0 {
        return Err(conflict(
            rid,
            format!("version mismatch: expected {expected_version}, current changed concurrently"),
        ));
    }

    if let Some(src) = load_script(&mut tx, auth.ctx.org_id.as_uuid(), &slug, "after_save")
        .await
        .map_err(internal(rid))?
    {
        run_script(&src, &mut values, rid)?;
        let search_text = build_search_text(&values);
        let values_json = Value::Object(values.clone());
        sqlx::query(
            r#"
            UPDATE custom_record SET "values" = $3, search_text = $4, updated_at = now()
            WHERE org_id = $1 AND id = $2
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(record_id)
        .bind(&values_json)
        .bind(&search_text)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
    }

    let values_json = Value::Object(values.clone());
    let search_text = build_search_text(&values);
    let search_doc = search_document(&org_public, &slug, &public_id, &values_json, &search_text);

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Custom,
        slug.clone(),
        "updated",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": public_id,
            "record_id": public_id,
            "entity_slug": slug,
            "search_text": search_text,
            "search_doc": search_doc,
            "version": expected_version + 1,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.record.update",
        "custom_record",
        &public_id,
        serde_json::json!({ "entity_slug": slug, "version": expected_version + 1 }),
    )
    .await
    .map_err(internal(rid))?;

    let row: RecordRow = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, "values", version, created_at, updated_at
        FROM custom_record WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(to_dto(row)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/custom/records/{slug}/{id}",
    tag = "custom-records",
    responses((status = 200, body = CustomRecordDto))
)]
pub async fn delete_record(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((slug, id)): Path<(String, String)>,
) -> Result<Json<CustomRecordDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let principal = load_authz_principal(&state, &auth).await?;
    enforce_opt(&principal, perms::custom_entity_write(&slug), rid)?;
    let record_id = parse_public_id(IdKind::CustomRecord, &id, rid)?;
    let org_public = auth.ctx.org_id.to_public().as_str();

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let _ = load_published_entity(&mut tx, auth.ctx.org_id.as_uuid(), &slug, rid).await?;

    let row: Option<RecordRow> = sqlx::query_as(
        r#"
        SELECT public_id, entity_slug, "values", version, created_at, updated_at
        FROM custom_record
        WHERE org_id = $1 AND id = $2 AND entity_slug = $3 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .bind(&slug)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let row = row.ok_or_else(|| not_found(rid, "record"))?;

    sqlx::query(
        r#"
        UPDATE custom_record
        SET deleted_at = now(), updated_by = $4, updated_at = now()
        WHERE org_id = $1 AND id = $2 AND entity_slug = $3 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(record_id)
    .bind(&slug)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let empty = Map::new();
    let search_text = build_search_text(row.2.as_object().unwrap_or(&empty));
    let search_doc = search_document(&org_public, &slug, &row.0, &row.2, &search_text);

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Custom,
        slug.clone(),
        "deleted",
        1,
        auth.ctx.actor.clone(),
        serde_json::json!({
            "id": row.0,
            "entity_slug": slug,
            "search_doc": search_doc,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.record.delete",
        "custom_record",
        &row.0,
        serde_json::json!({ "entity_slug": slug }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(to_dto(row)))
}
