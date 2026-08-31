//! Start / list / cancel workflow instances.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::Actor;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::definition::WorkflowGraph;
use crate::engine::{enforce_concurrency_cap, run_until_idle, user_workflow_id};
use crate::handlers::{
    internal, not_found, parse_public_id, require_idempotency, user_public, validation,
};
use crate::principal::{enforce, load_principal};
use crate::state::AppState;
use crate::types::{
    MessageResponse, StartWorkflowRequest, WorkflowInstanceDto, WorkflowInstanceListResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workflows/definitions/{id}/start",
            post(start_instance),
        )
        .route("/api/v1/workflows/instances", get(list_instances))
        .route("/api/v1/workflows/instances/{id}", get(get_instance))
        .route(
            "/api/v1/workflows/instances/{id}/cancel",
            post(cancel_instance),
        )
        .route(
            "/api/v1/workflows/internal/triggers/event",
            post(ingest_trigger_event),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub definition_id: Option<String>,
}

#[allow(clippy::type_complexity)]
type InstRow = (
    Uuid,
    Uuid,
    Uuid,
    i32,
    String,
    Uuid,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn map_instance(row: InstRow, def_public: String) -> WorkflowInstanceDto {
    WorkflowInstanceDto {
        id: PublicId::new(IdKind::WorkflowInstance, row.0).as_str(),
        definition_id: def_public,
        version_id: PublicId::new(IdKind::WorkflowVersion, row.2).as_str(),
        version_number: row.3,
        status: row.4,
        actor_user_id: user_public(row.5),
        temporal_workflow_id: row.6,
        step_count: row.7,
        current_node_id: row.8,
        error_message: row.9,
        waiting_until: row.10,
        sla_deadline: row.11,
        started_at: row.12,
        updated_at: row.13,
        completed_at: row.14,
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/definitions/{id}/start",
    tag = "workflows-instances",
    request_body = StartWorkflowRequest,
    responses((status = 201, body = WorkflowInstanceDto))
)]
pub async fn start_instance(
    State(state): State<AppState>,
    auth: AuthCtx,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<StartWorkflowRequest>,
) -> Result<(axum::http::StatusCode, Json<WorkflowInstanceDto>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    if body.dry_run {
        return Err(validation(
            rid,
            "dry_run start is not allowed; use POST /api/v1/workflows/simulate (zero side effects)",
        ));
    }
    let idem = require_idempotency(&headers, rid)?;
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_run(), rid)?;
    let def_uuid = parse_public_id(IdKind::WorkflowDefinition, &id, rid)?;
    let actor_user = auth.ctx.actor.on_behalf_of;
    let scope = format!("workflow.start:{id}");

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    if let Some((status, cached)) =
        crate::idempotency::get(&mut *tx, auth.ctx.org_id.as_uuid(), &scope, &idem)
            .await
            .map_err(internal(rid))?
    {
        tx.commit().await.map_err(internal(rid))?;
        if status == 201 {
            let dto: WorkflowInstanceDto = serde_json::from_value(cached)
                .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
            return Ok((axum::http::StatusCode::CREATED, Json(dto)));
        }
        return Err(AppError::new(
            ErrorCode::Conflict,
            rid,
            "Idempotency-Key already used with a different outcome",
        ));
    }

    let bounds = enforce_concurrency_cap(&mut tx, auth.ctx.org_id.as_uuid(), rid).await?;

    let def: Option<(String, Uuid, Option<i32>)> = sqlx::query_as(
        r#"
        SELECT public_id, created_by, current_published_version
        FROM workflow_definition WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some((def_public, created_by, Some(pub_ver))) = def else {
        return Err(validation(rid, "definition must be published before start"));
    };

    let ver: Option<(Uuid, String, serde_json::Value, Vec<String>)> = sqlx::query_as(
        r#"
        SELECT id, public_id, graph, required_permissions
        FROM workflow_definition_version
        WHERE definition_id = $1 AND org_id = $2 AND version = $3 AND published_at IS NOT NULL
        "#,
    )
    .bind(def_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(pub_ver)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    let Some((ver_uuid, _ver_public, graph_json, _reqs)) = ver else {
        return Err(not_found(rid, "published workflow version"));
    };
    let graph: WorkflowGraph = serde_json::from_value(graph_json)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    // Creator permissions bound the run (not the starter's elevated perms).
    let creator_principal = load_principal(&state.pool, auth.ctx.org_id, created_by, rid)
        .await?
        .0;
    crate::permissions::enforce_creator_can_own_graph(&creator_principal, &graph, rid)?;

    let inst_id = new_uuid_v7();
    let inst_public = PublicId::new(IdKind::WorkflowInstance, inst_id);
    let org_public = auth.ctx.org_id.to_public().as_str();
    let temporal_id = user_workflow_id(&org_public, &def_public, &inst_public.as_str());
    let sla = graph
        .sla_seconds
        .map(|s| Utc::now() + Duration::seconds(s as i64));
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO workflow_instance (
            id, org_id, public_id, definition_id, version_id, version_number, status,
            actor_user_id, created_by, trigger_payload, current_node_id, step_count,
            temporal_workflow_id, sla_deadline, started_at, updated_at
        ) VALUES (
            $1,$2,$3,$4,$5,$6,'running',$7,$8,$9,$10,0,$11,$12,$13,$13
        )
        "#,
    )
    .bind(inst_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(inst_public.as_str())
    .bind(def_uuid)
    .bind(ver_uuid)
    .bind(pub_ver)
    .bind(created_by) // on_behalf_of creator — never escalate
    .bind(actor_user)
    .bind(&body.payload)
    .bind(&graph.entry)
    .bind(&temporal_id)
    .bind(sla)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let envelope = EventEnvelope::new(
        auth.ctx.org_id,
        Context::Workflow,
        "instance",
        "started",
        1,
        Actor::human(created_by),
        json!({
            "id": inst_public.as_str(),
            "definition_id": def_public,
            "version_id": PublicId::new(IdKind::WorkflowVersion, ver_uuid).as_str(),
            "version": pub_ver,
            "temporal_workflow_id": temporal_id,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    let dto = WorkflowInstanceDto {
        id: inst_public.as_str(),
        definition_id: def_public.clone(),
        version_id: PublicId::new(IdKind::WorkflowVersion, ver_uuid).as_str(),
        version_number: pub_ver,
        status: "running".into(),
        actor_user_id: user_public(created_by),
        temporal_workflow_id: temporal_id,
        step_count: 0,
        current_node_id: Some(graph.entry.clone()),
        error_message: None,
        waiting_until: None,
        sla_deadline: sla,
        started_at: now,
        updated_at: now,
        completed_at: None,
    };
    let cached = serde_json::to_value(&dto)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
    crate::idempotency::put(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        &scope,
        &idem,
        201,
        cached,
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    // Drive steps with activities noop in tests via env; creator principal + actor.
    let _ = bounds;
    let dry_http = matches!(
        std::env::var("WORKFLOW_ACTIVITIES_NOOP").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    );
    run_until_idle(
        &state.pool,
        auth.ctx.org_id,
        inst_id,
        &graph,
        &creator_principal,
        &Actor::human(created_by),
        rid,
        dry_http,
    )
    .await?;

    // Re-read final status
    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let row: Option<InstRow> = sqlx::query_as(
        r#"
        SELECT id, definition_id, version_id, version_number, status, actor_user_id,
               temporal_workflow_id, step_count, current_node_id, error_message,
               waiting_until, sla_deadline, started_at, updated_at, completed_at
        FROM workflow_instance WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(inst_id)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;
    let row = row.ok_or_else(|| not_found(rid, "workflow instance"))?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(map_instance(row, def_public)),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/instances",
    tag = "workflows-instances",
    responses((status = 200, body = WorkflowInstanceListResponse))
)]
pub async fn list_instances(
    State(state): State<AppState>,
    auth: AuthCtx,
    Query(q): Query<ListQuery>,
) -> Result<Json<WorkflowInstanceListResponse>, AppError> {
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

    let def_filter: Option<Uuid> = match q.definition_id.as_deref() {
        Some(s) => Some(parse_public_id(IdKind::WorkflowDefinition, s, rid)?),
        None => None,
    };

    let rows: Vec<(
        Uuid,
        Uuid,
        Uuid,
        i32,
        String,
        Uuid,
        String,
        i32,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT i.id, i.definition_id, i.version_id, i.version_number, i.status, i.actor_user_id,
               i.temporal_workflow_id, i.step_count, i.current_node_id, i.error_message,
               i.waiting_until, i.sla_deadline, i.started_at, i.updated_at, i.completed_at,
               d.public_id
        FROM workflow_instance i
        JOIN workflow_definition d ON d.id = i.definition_id AND d.org_id = i.org_id
        WHERE i.org_id = $1
          AND ($2::text IS NULL OR i.status = $2)
          AND ($3::uuid IS NULL OR i.definition_id = $3)
        ORDER BY i.started_at DESC
        LIMIT 200
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(q.status.as_deref())
    .bind(def_filter)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(WorkflowInstanceListResponse {
        items: rows
            .into_iter()
            .map(|r| {
                map_instance(
                    (
                        r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13,
                        r.14,
                    ),
                    r.15,
                )
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/workflows/instances/{id}",
    tag = "workflows-instances",
    responses((status = 200, body = WorkflowInstanceDto))
)]
pub async fn get_instance(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<WorkflowInstanceDto>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_read(), rid)?;
    let inst_uuid = parse_public_id(IdKind::WorkflowInstance, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let row: Option<(
        Uuid,
        Uuid,
        Uuid,
        i32,
        String,
        Uuid,
        String,
        i32,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT i.id, i.definition_id, i.version_id, i.version_number, i.status, i.actor_user_id,
               i.temporal_workflow_id, i.step_count, i.current_node_id, i.error_message,
               i.waiting_until, i.sla_deadline, i.started_at, i.updated_at, i.completed_at,
               d.public_id
        FROM workflow_instance i
        JOIN workflow_definition d ON d.id = i.definition_id AND d.org_id = i.org_id
        WHERE i.id = $1 AND i.org_id = $2
        "#,
    )
    .bind(inst_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;
    let Some(r) = row else {
        return Err(not_found(rid, "workflow instance"));
    };
    Ok(Json(map_instance(
        (
            r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14,
        ),
        r.15,
    )))
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/instances/{id}/cancel",
    tag = "workflows-instances",
    responses((status = 200, body = MessageResponse))
)]
pub async fn cancel_instance(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_run(), rid)?;
    let inst_uuid = parse_public_id(IdKind::WorkflowInstance, &id, rid)?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
    let updated = sqlx::query(
        r#"
        UPDATE workflow_instance
        SET status = 'cancelled', completed_at = now(), updated_at = now()
        WHERE id = $1 AND org_id = $2 AND status IN ('running', 'waiting')
        "#,
    )
    .bind(inst_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;
    if updated.rows_affected() == 0 {
        return Err(validation(rid, "instance not cancellable"));
    }

    let meta: Option<(String, Uuid, Uuid, i32)> = sqlx::query_as(
        r#"
        SELECT i.public_id, i.definition_id, i.version_id, i.version_number
        FROM workflow_instance i WHERE i.id = $1 AND i.org_id = $2
        "#,
    )
    .bind(inst_uuid)
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    if let Some((pub_id, def_uuid, ver_uuid, ver_num)) = meta {
        let envelope = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Workflow,
            "instance",
            "cancelled",
            1,
            auth.ctx.actor.clone(),
            json!({
                "id": pub_id,
                "definition_id": PublicId::new(IdKind::WorkflowDefinition, def_uuid).as_str(),
                "version_id": PublicId::new(IdKind::WorkflowVersion, ver_uuid).as_str(),
                "version": ver_num,
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
    }
    tx.commit().await.map_err(internal(rid))?;
    Ok(Json(MessageResponse {
        message: "cancelled".into(),
    }))
}

/// Internal: start matching published workflows for a domain event (service-to-service).
#[derive(Debug, Deserialize)]
pub struct TriggerEventRequest {
    pub event_key: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[utoipa::path(
    post,
    path = "/api/v1/workflows/internal/triggers/event",
    tag = "workflows-internal",
    request_body = StartWorkflowRequest,
    responses((status = 200, body = MessageResponse))
)]
pub async fn ingest_trigger_event(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<TriggerEventRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    let (principal, _, _) = load_principal(
        &state.pool,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;
    enforce(&principal, perms::operations_workflow_run(), rid)?;

    if !crate::catalogue::is_known_trigger(&body.event_key) {
        return Err(validation(
            rid,
            format!("unknown trigger {}", body.event_key),
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;

    // Find published definitions whose published graph trigger matches.
    let defs: Vec<(Uuid, String, Uuid, i32, serde_json::Value, Uuid)> = sqlx::query_as(
        r#"
        SELECT d.id, d.public_id, v.id, v.version, v.graph, d.created_by
        FROM workflow_definition d
        JOIN workflow_definition_version v
          ON v.definition_id = d.id AND v.org_id = d.org_id
         AND v.version = d.current_published_version AND v.published_at IS NOT NULL
        WHERE d.org_id = $1 AND d.status = 'published'
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;
    tx.commit().await.map_err(internal(rid))?;

    let mut started = 0u32;
    for (def_uuid, def_public, ver_uuid, ver_num, graph_json, created_by) in defs {
        let graph: WorkflowGraph = match serde_json::from_value(graph_json) {
            Ok(g) => g,
            Err(_) => continue,
        };
        let matches = match &graph.trigger {
            crate::definition::WorkflowTrigger::DomainEvent { event_key } => {
                event_key == &body.event_key
            }
            crate::definition::WorkflowTrigger::Manual => false,
        };
        if !matches {
            continue;
        }

        // Reuse start path via inline insert (simplified).
        let mut tx = state.pool.begin().await.map_err(internal(rid))?;
        crate::handlers::set_org(&mut tx, auth.ctx.org_id, rid).await?;
        if enforce_concurrency_cap(&mut tx, auth.ctx.org_id.as_uuid(), rid)
            .await
            .is_err()
        {
            tx.rollback().await.ok();
            continue;
        }
        let creator_principal = load_principal(&state.pool, auth.ctx.org_id, created_by, rid)
            .await?
            .0;
        if crate::permissions::enforce_creator_can_own_graph(&creator_principal, &graph, rid)
            .is_err()
        {
            tx.rollback().await.ok();
            continue;
        }

        let inst_id = new_uuid_v7();
        let inst_public = PublicId::new(IdKind::WorkflowInstance, inst_id);
        let org_public = auth.ctx.org_id.to_public().as_str();
        let temporal_id = user_workflow_id(&org_public, &def_public, &inst_public.as_str());
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO workflow_instance (
                id, org_id, public_id, definition_id, version_id, version_number, status,
                actor_user_id, created_by, trigger_event, trigger_payload, current_node_id,
                step_count, temporal_workflow_id, started_at, updated_at
            ) VALUES (
                $1,$2,$3,$4,$5,$6,'running',$7,$7,$8,$9,$10,0,$11,$12,$12
            )
            "#,
        )
        .bind(inst_id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(inst_public.as_str())
        .bind(def_uuid)
        .bind(ver_uuid)
        .bind(ver_num)
        .bind(created_by)
        .bind(json!({"event_key": body.event_key}))
        .bind(&body.payload)
        .bind(&graph.entry)
        .bind(&temporal_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;

        let envelope = EventEnvelope::new(
            auth.ctx.org_id,
            Context::Workflow,
            "instance",
            "started",
            1,
            Actor::human(created_by),
            json!({
                "id": inst_public.as_str(),
                "definition_id": def_public,
                "version_id": PublicId::new(IdKind::WorkflowVersion, ver_uuid).as_str(),
            }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
        tx.commit().await.map_err(internal(rid))?;

        let dry_http = matches!(
            std::env::var("WORKFLOW_ACTIVITIES_NOOP").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        );
        let _ = run_until_idle(
            &state.pool,
            auth.ctx.org_id,
            inst_id,
            &graph,
            &creator_principal,
            &Actor::human(created_by),
            rid,
            dry_http,
        )
        .await;
        started += 1;
    }

    Ok(Json(MessageResponse {
        message: format!("started {started} instance(s)"),
    }))
}
