//! Workflow definition CRUD + versioned publish.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::definition::WorkflowGraph;
use crate::handlers::{
    internal, not_found, parse_public_id, require_idempotency, user_public, validation,
};
use crate::permissions::enforce_creator_can_own_graph;
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    CreateWorkflowDefinitionRequest, MessageResponse, MigrateInstanceRequest,
    PublishWorkflowRequest, UpdateWorkflowDefinitionRequest, WorkflowDefinitionDto,
    WorkflowDefinitionListResponse, WorkflowVersionDto, WorkflowVersionListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workflows/definitions",
            get(list_definitions).post(create_definition),
        )
        .route(
            "/api/v1/workflows/definitions/{id}",
            get(get_definition).patch(update_definition),
        )
        .route(
            "/api/v1/workflows/definitions/{id}/publish",
            post(publish_definition),
        )
        .route(
            "/api/v1/workflows/definitions/{id}/versions",
            get(list_versions),
        )
        .route(
            "/api/v1/workflows/definitions/{id}/versions/{version}",
            get(get_version),
        )
        .route(
            "/api/v1/workflows/instances/{id}/migrate",
            post(migrate_stub),
        )
}

#[allow(clippy::type_complexity)]
type DefRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Uuid,
    Option<i32>,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn map_def(
    row: DefRow,
    graph: Option<WorkflowGraph>,
    latest_version_id: Option<String>,
) -> WorkflowDefinitionDto {
    WorkflowDefinitionDto {
        id: PublicId::new(IdKind::WorkflowDefinition, row.0).as_str(),
        name: row.2,
        description: row.3,
        status: row.4,
        created_by: user_public(row.5),
        current_published_version: row.6,
        created_at: row.7,
        updated_at: row.8,
        graph,
        latest_version_id,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/definitions",
    tag = "workflows-definitions",
    responses((status = 200, body = WorkflowDefinitionListResponse))
)]
pub async fn list_definitions(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<WorkflowDefinitionListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let rows: Vec<DefRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, description, status, created_by,
               current_published_version, created_at, updated_at
        FROM workflow_definition
        WHERE org_id = $1
        ORDER BY updated_at DESC
        LIMIT 200
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(WorkflowDefinitionListResponse {
        items: rows.into_iter().map(|r| map_def(r, None, None)).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/definitions",
    tag = "workflows-definitions",
    request_body = CreateWorkflowDefinitionRequest,
    responses((status = 201, body = WorkflowDefinitionDto))
)]
pub async fn create_definition(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateWorkflowDefinitionRequest>,
) -> Result<(axum::http::StatusCode, Json<WorkflowDefinitionDto>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_write(), rid)?;

    if body.name.trim().is_empty() {
        return Err(validation(rid, "name is required"));
    }
    body.graph.validate().map_err(|e| validation(rid, e))?;
    let required = enforce_creator_can_own_graph(&principal, &body.graph, rid)?;

    let def_id = new_uuid_v7();
    let def_public = PublicId::new(IdKind::WorkflowDefinition, def_id);
    let ver_id = new_uuid_v7();
    let ver_public = PublicId::new(IdKind::WorkflowVersion, ver_id);
    let actor = auth.ctx.actor.on_behalf_of;
    let graph_json = serde_json::to_value(&body.graph)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    sqlx::query(
        r#"
        INSERT INTO workflow_definition (
            id, org_id, public_id, name, description, status, created_by, updated_by
        ) VALUES ($1,$2,$3,$4,$5,'draft',$6,$6)
        "#,
    )
    .bind(def_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(def_public.as_str())
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    sqlx::query(
        r#"
        INSERT INTO workflow_definition_version (
            id, org_id, public_id, definition_id, version, graph, required_permissions
        ) VALUES ($1,$2,$3,$4,0,$5,$6)
        "#,
    )
    .bind(ver_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(ver_public.as_str())
    .bind(def_id)
    .bind(&graph_json)
    .bind(&required)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let now = Utc::now();
    tx.commit().await.map_err(internal(rid))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(WorkflowDefinitionDto {
            id: def_public.as_str(),
            name: body.name.trim().to_string(),
            description: body.description,
            status: "draft".into(),
            created_by: user_public(actor),
            current_published_version: None,
            created_at: now,
            updated_at: now,
            graph: Some(body.graph),
            latest_version_id: Some(ver_public.as_str()),
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/definitions/{id}",
    tag = "workflows-definitions",
    responses((status = 200, body = WorkflowDefinitionDto))
)]
pub async fn get_definition(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<WorkflowDefinitionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let row: Option<DefRow> = sqlx::query_as(
        r#"
        SELECT id, public_id, name, description, status, created_by,
               current_published_version, created_at, updated_at
        FROM workflow_definition WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some(row) = row else {
        return Err(not_found(rid, "workflow definition"));
    };

    let ver: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT id, graph FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2
        ORDER BY version DESC LIMIT 1
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let (graph, ver_id) = match ver {
        Some((vid, g)) => (
            Some(
                serde_json::from_value(g)
                    .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?,
            ),
            Some(PublicId::new(IdKind::WorkflowVersion, vid).as_str()),
        ),
        None => (None, None),
    };
    Ok(Json(map_def(row, graph, ver_id)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/workflows/definitions/{id}",
    tag = "workflows-definitions",
    request_body = UpdateWorkflowDefinitionRequest,
    responses((status = 200, body = WorkflowDefinitionDto))
)]
pub async fn update_definition(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
    Json(body): Json<UpdateWorkflowDefinitionRequest>,
) -> Result<Json<WorkflowDefinitionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_write(), rid)?;
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;
    let actor = auth.ctx.actor.on_behalf_of;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let row: Option<(
        Uuid,
        String,
        String,
        String,
        Uuid,
        Option<i32>,
        DateTime<Utc>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
            SELECT id, name, description, status, created_by,
                   current_published_version, created_at, updated_at
            FROM workflow_definition WHERE id = $1 AND org_id = $2 FOR UPDATE
            "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some((_, mut name, mut description, status, created_by, current_pub, created_at, _)) = row
    else {
        return Err(not_found(rid, "workflow definition"));
    };
    if status == "archived" {
        return Err(validation(rid, "cannot update archived definition"));
    }
    if let Some(n) = body.name {
        name = n;
    }
    if let Some(d) = body.description {
        description = d;
    }

    let mut graph_out = None;
    let mut ver_public = None;
    if let Some(graph) = body.graph {
        graph.validate().map_err(|e| validation(rid, e))?;
        let required = enforce_creator_can_own_graph(&principal, &graph, rid)?;
        let graph_json = serde_json::to_value(&graph)
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

        // Update draft version 0 if unpublished changes; else insert new draft snapshot as version 0 upsert.
        let existing: Option<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM workflow_definition_version
            WHERE definition_id = $1 AND org_id = $2 AND version = 0 AND published_at IS NULL
            "#,
        )
        .bind(def_uuid)
        .bind(auth.ctx.org_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(rid))?;

        if let Some((vid,)) = existing {
            sqlx::query(
                r#"
                UPDATE workflow_definition_version
                SET graph = $1, required_permissions = $2
                WHERE id = $3 AND org_id = $4
                "#,
            )
            .bind(&graph_json)
            .bind(&required)
            .bind(vid)
            .bind(auth.ctx.org_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(internal(rid))?;
            ver_public = Some(PublicId::new(IdKind::WorkflowVersion, vid).as_str());
        } else {
            let ver_id = new_uuid_v7();
            let vp = PublicId::new(IdKind::WorkflowVersion, ver_id);
            // If version 0 was published, bump a working draft as negative? Prefer: keep draft as
            // max(version)+0 draft by updating a dedicated draft row with version=0 only when unpublished.
            // When published, create a new unpublished row with version = current+1 but unpublished — simpler:
            // always keep exactly one unpublished draft at version 0 by reusing; if 0 published, insert working copy as version 0 replacement isn't possible.
            // Practical approach: store draft graph on definition via latest unpublished OR version 0.
            // Here: if no unpublished v0, insert new version with version = -1 sentinel... Better:
            // Use version 0 exclusively as draft; on publish, copy to N and clear draft by inserting fresh v0 from published.
            sqlx::query(
                r#"
                INSERT INTO workflow_definition_version (
                    id, org_id, public_id, definition_id, version, graph, required_permissions
                ) VALUES ($1,$2,$3,$4,0,$5,$6)
                ON CONFLICT (org_id, definition_id, version) DO UPDATE
                SET graph = EXCLUDED.graph,
                    required_permissions = EXCLUDED.required_permissions,
                    published_at = NULL,
                    published_by = NULL
                "#,
            )
            .bind(ver_id)
            .bind(auth.ctx.org_id.as_uuid())
            .bind(vp.as_str())
            .bind(def_uuid)
            .bind(&graph_json)
            .bind(&required)
            .execute(&mut *tx)
            .await
            .map_err(internal(rid))?;
            ver_public = Some(vp.as_str());
        }
        graph_out = Some(graph);
    }

    let now = Utc::now();
    sqlx::query(
        r#"
        UPDATE workflow_definition
        SET name = $1, description = $2, updated_by = $3, updated_at = $4
        WHERE id = $5 AND org_id = $6
        "#,
    )
    .bind(&name)
    .bind(&description)
    .bind(actor)
    .bind(now)
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(WorkflowDefinitionDto {
        id: PublicId::new(IdKind::WorkflowDefinition, def_uuid).as_str(),
        name,
        description,
        status,
        created_by: user_public(created_by),
        current_published_version: current_pub,
        created_at,
        updated_at: now,
        graph: graph_out,
        latest_version_id: ver_public,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/definitions/{id}/publish",
    tag = "workflows-definitions",
    request_body = PublishWorkflowRequest,
    responses((status = 200, body = WorkflowVersionDto))
)]
pub async fn publish_definition(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(_body): Json<PublishWorkflowRequest>,
) -> Result<Json<WorkflowVersionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let idem = require_idempotency(&headers, rid)?;
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_publish(), rid)?;
    // AI must never auto-publish — only human commit (actor.is_ai denied).
    if auth.ctx.actor.is_ai {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            rid,
            "AI cannot publish workflow definitions; human must commit publish",
        ));
    }
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;
    let actor = auth.ctx.actor.on_behalf_of;
    let scope = format!("workflow.publish:{id}");

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    if let Some((status, body)) =
        crate::idempotency::get(&mut *tx, auth.ctx.org_id.as_uuid(), &scope, &idem)
            .await
            .map_err(internal(rid))?
    {
        tx.commit().await.map_err(internal(rid))?;
        if status == 200 {
            let dto: WorkflowVersionDto = serde_json::from_value(body)
                .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
            return Ok(Json(dto));
        }
        return Err(AppError::new(
            ErrorCode::Conflict,
            rid,
            "Idempotency-Key already used with a different outcome",
        ));
    }

    let def: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT public_id, created_by FROM workflow_definition WHERE id = $1 AND org_id = $2 FOR UPDATE",
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some((def_public, created_by)) = def else {
        return Err(not_found(rid, "workflow definition"));
    };

    // Creator permissions still bound the published graph.
    let creator_principal = if created_by == actor {
        principal.clone()
    } else {
        load_principal(&state.pool, auth.ctx.org_id, created_by, rid)
            .await?
            .0
    };

    let draft: Option<(Uuid, String, serde_json::Value, Vec<String>)> = sqlx::query_as(
        r#"
        SELECT id, public_id, graph, required_permissions
        FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2 AND version = 0
        FOR UPDATE
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some((_draft_id, _draft_public, graph_json, _reqs)) = draft else {
        return Err(validation(rid, "no draft graph to publish"));
    };
    let graph: WorkflowGraph = serde_json::from_value(graph_json.clone())
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
    graph.validate().map_err(|e| validation(rid, e))?;
    let required = enforce_creator_can_own_graph(&creator_principal, &graph, rid)?;

    let (max_ver,): (i32,) = sqlx::query_as(
        r#"
        SELECT COALESCE(MAX(version), 0)::int FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2 AND version > 0
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let new_ver = max_ver + 1;
    let ver_id = new_uuid_v7();
    let ver_public = PublicId::new(IdKind::WorkflowVersion, ver_id);
    let published_at = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO workflow_definition_version (
            id, org_id, public_id, definition_id, version, graph, required_permissions,
            published_at, published_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(ver_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(ver_public.as_str())
    .bind(def_uuid)
    .bind(new_ver)
    .bind(&graph_json)
    .bind(&required)
    .bind(published_at)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    sqlx::query(
        r#"
        UPDATE workflow_definition
        SET status = 'published', current_published_version = $1, updated_by = $2, updated_at = now()
        WHERE id = $3 AND org_id = $4
        "#,
    )
    .bind(new_ver)
    .bind(actor)
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    // Refresh draft v0 from published snapshot (editing continues on draft).
    sqlx::query(
        r#"
        UPDATE workflow_definition_version
        SET graph = $1, required_permissions = $2, published_at = NULL, published_by = NULL
        WHERE definition_id = $3 AND org_id = $4 AND version = 0
        "#,
    )
    .bind(&graph_json)
    .bind(&required)
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Workflow,
        "definition",
        "published",
        1,
        auth.ctx.actor.clone(),
        json!({
            "id": def_public,
            "version_id": ver_public.as_str(),
            "version": new_ver,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    let dto = WorkflowVersionDto {
        id: ver_public.as_str(),
        definition_id: def_public,
        version: new_ver,
        graph,
        required_permissions: required,
        published_at: Some(published_at),
        published_by: Some(user_public(actor)),
        created_at: published_at,
    };
    let body = serde_json::to_value(&dto)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
    crate::idempotency::put(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        &scope,
        &idem,
        200,
        body,
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(dto))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/definitions/{id}/versions",
    tag = "workflows-definitions",
    responses((status = 200, body = WorkflowVersionListResponse))
)]
pub async fn list_versions(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<WorkflowVersionListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let rows: Vec<(
        Uuid,
        i32,
        serde_json::Value,
        Vec<String>,
        Option<DateTime<Utc>>,
        Option<Uuid>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, version, graph, required_permissions, published_at, published_by, created_at
        FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2 AND version > 0
        ORDER BY version DESC
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let items = rows
        .into_iter()
        .map(|(vid, ver, g, reqs, pub_at, pub_by, created)| {
            Ok(WorkflowVersionDto {
                id: PublicId::new(IdKind::WorkflowVersion, vid).as_str(),
                definition_id: id.clone(),
                version: ver,
                graph: serde_json::from_value(g)
                    .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?,
                required_permissions: reqs,
                published_at: pub_at,
                published_by: pub_by.map(user_public),
                created_at: created,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(Json(WorkflowVersionListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/definitions/{id}/versions/{version}",
    tag = "workflows-definitions",
    responses((status = 200, body = WorkflowVersionDto))
)]
pub async fn get_version(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path((id, version)): Path<(String, i32)>,
) -> Result<Json<WorkflowVersionDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let row: Option<(
        Uuid,
        i32,
        serde_json::Value,
        Vec<String>,
        Option<DateTime<Utc>>,
        Option<Uuid>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, version, graph, required_permissions, published_at, published_by, created_at
        FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2 AND version = $3
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;
    let Some((vid, ver, g, reqs, pub_at, pub_by, created)) = row else {
        return Err(not_found(rid, "workflow version"));
    };
    Ok(Json(WorkflowVersionDto {
        id: PublicId::new(IdKind::WorkflowVersion, vid).as_str(),
        definition_id: id,
        version: ver,
        graph: serde_json::from_value(g)
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?,
        required_permissions: reqs,
        published_at: pub_at,
        published_by: pub_by.map(user_public),
        created_at: created,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/instances/{id}/migrate",
    tag = "workflows-instances",
    request_body = MigrateInstanceRequest,
    responses((status = 501, body = MessageResponse))
)]
pub async fn migrate_stub(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(_id): Path<String>,
    Json(_body): Json<MigrateInstanceRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_manage(), rid)?;
    Err(AppError::new(
        ErrorCode::ValidationFailed,
        rid,
        "explicit in-flight migrate is stubbed; keep-old-version is the safe default",
    ))
}
