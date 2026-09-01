//! Inherited team grants + membership permission delegation (via authz PDP statements).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::set_session_org_id;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::extract::AuthUser;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspace/grants/inherit",
            get(list_inherit).post(create_inherit),
        )
        .route(
            "/api/v1/workspace/grants/delegate",
            get(list_delegate).post(create_delegate),
        )
        .route(
            "/api/v1/workspace/grants/delegate/{id}/revoke",
            post(revoke_delegate),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateInheritRequest {
    pub team_id: String,
    pub permission_id: String,
    #[serde(default = "default_allow")]
    pub effect: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_allow() -> String {
    "allow".into()
}
fn default_scope() -> String {
    "organization".into()
}

#[derive(Debug, Serialize)]
pub struct InheritGrantDto {
    pub id: String,
    pub team_id: String,
    pub permission_id: String,
    pub effect: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDelegateRequest {
    pub to_membership_id: String,
    pub permission_id: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DelegationDto {
    pub id: String,
    pub from_membership_id: String,
    pub to_membership_id: String,
    pub permission_id: String,
    pub scope: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

async fn bump_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE membership
        SET policy_version = policy_version + 1, updated_at = now()
        WHERE org_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn create_inherit(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateInheritRequest>,
) -> Result<Json<InheritGrantDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::workspace_grant_inherit()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    if companyos_authz::validate_permission_id(&body.permission_id).is_err() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid permission_id",
        ));
    }
    let effect = body.effect.to_lowercase();
    if effect != "allow" && effect != "deny" {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "effect must be allow|deny",
        ));
    }

    let team_pid: PublicId = body
        .team_id
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid team_id"))?;
    if team_pid.kind() != IdKind::Team {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "team_id must be tem_…",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let team_ok: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, public_id FROM team WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(team_pid.uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal(&request_id))?;
    let Some((_, team_public)) = team_ok else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "team not found",
        ));
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::PermissionInheritGrant, id);
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO permission_inherit_grant (
            id, org_id, public_id, team_id, permission_id, effect, scope, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING created_at
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(team_pid.uuid())
    .bind(&body.permission_id)
    .bind(&effect)
    .bind(&body.scope)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    bump_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "grant.inherit.create",
        "permission_inherit_grant",
        &public_id.to_string(),
        serde_json::json!({
            "team_id": team_public,
            "permission_id": body.permission_id,
            "effect": effect,
        }),
    )
    .await;

    Ok(Json(InheritGrantDto {
        id: public_id.to_string(),
        team_id: team_public,
        permission_id: body.permission_id,
        effect,
        scope: body.scope,
        created_at,
    }))
}

async fn list_inherit(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<InheritGrantDto>>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::workspace_grant_inherit()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(String, String, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        r#"
        SELECT g.public_id, t.public_id, g.permission_id, g.effect, g.scope, g.created_at
        FROM permission_inherit_grant g
        JOIN team t ON t.id = g.team_id
        WHERE g.org_id = $1
        ORDER BY g.created_at
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, team_id, permission_id, effect, scope, created_at)| InheritGrantDto {
                    id,
                    team_id,
                    permission_id,
                    effect,
                    scope,
                    created_at,
                },
            )
            .collect(),
    ))
}

async fn create_delegate(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateDelegateRequest>,
) -> Result<Json<DelegationDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::workspace_grant_delegate()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    if companyos_authz::validate_permission_id(&body.permission_id).is_err() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid permission_id",
        ));
    }
    if body.expires_at <= Utc::now() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "expires_at must be in the future",
        ));
    }

    let to_pid: PublicId = body.to_membership_id.parse().map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid to_membership_id",
        )
    })?;
    if to_pid.kind() != IdKind::Membership {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "to_membership_id must be mem_…",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let from_membership = user.membership_id;
    let to_row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, public_id FROM membership WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL",
    )
    .bind(org_id)
    .bind(to_pid.uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    let Some((_, to_public)) = to_row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "target membership not found",
        ));
    };

    let from_public: String =
        sqlx::query_scalar("SELECT public_id FROM membership WHERE id = $1 AND org_id = $2")
            .bind(from_membership)
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal(&request_id))?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::PermissionDelegation, id);
    sqlx::query(
        r#"
        INSERT INTO permission_delegation (
            id, org_id, public_id, from_membership_id, to_membership_id,
            permission_id, scope, expires_at, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(from_membership)
    .bind(to_pid.uuid())
    .bind(&body.permission_id)
    .bind(&body.scope)
    .bind(body.expires_at)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    bump_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "grant.delegate.create",
        "permission_delegation",
        &public_id.to_string(),
        serde_json::json!({
            "to_membership_id": to_public,
            "permission_id": body.permission_id,
            "expires_at": body.expires_at,
        }),
    )
    .await;

    Ok(Json(DelegationDto {
        id: public_id.to_string(),
        from_membership_id: from_public,
        to_membership_id: to_public,
        permission_id: body.permission_id,
        scope: body.scope,
        expires_at: body.expires_at,
        revoked_at: None,
    }))
}

async fn list_delegate(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<DelegationDto>>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::workspace_grant_delegate()).await?;
    let org_id = user.ctx.org_id.as_uuid();

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT d.public_id, f.public_id, t.public_id, d.permission_id, d.scope,
               d.expires_at, d.revoked_at
        FROM permission_delegation d
        JOIN membership f ON f.id = d.from_membership_id
        JOIN membership t ON t.id = d.to_membership_id
        WHERE d.org_id = $1
        ORDER BY d.created_at DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    from_membership_id,
                    to_membership_id,
                    permission_id,
                    scope,
                    expires_at,
                    revoked_at,
                )| DelegationDto {
                    id,
                    from_membership_id,
                    to_membership_id,
                    permission_id,
                    scope,
                    expires_at,
                    revoked_at,
                },
            )
            .collect(),
    ))
}

async fn revoke_delegate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::workspace_grant_delegate()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let pid: PublicId = id
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, &request_id, "invalid id"))?;
    if pid.kind() != IdKind::PermissionDelegation {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "id must be pdg_…",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let n = sqlx::query(
        "UPDATE permission_delegation SET revoked_at = now()
         WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL",
    )
    .bind(org_id)
    .bind(pid.uuid())
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?
    .rows_affected();
    if n == 0 {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "delegation not found",
        ));
    }
    bump_policy(&mut tx, org_id)
        .await
        .map_err(internal(&request_id))?;
    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "grant.delegate.revoke",
        "permission_delegation",
        &pid.to_string(),
        serde_json::json!({}),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "revoked": true, "id": pid.to_string() }),
    ))
}
