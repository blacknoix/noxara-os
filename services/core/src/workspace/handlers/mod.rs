//! Workspace HTTP handlers — `/api/v1/workspace/...`.

#![allow(clippy::type_complexity)]

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_authz::{
    is_allowed, perms, Effect, PermissionId, Principal, Role, Scope, Statement,
    PERMISSION_CATALOGUE, SENSITIVE_ACTIONS,
};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use uuid::Uuid;

use super::last_owner;
use super::provisioning;
use super::types::*;
use super::{audit_mutation, require_perm};
use crate::auth::extract::AuthUser;
use crate::auth::mail::{self, OutboundMail};
use crate::auth::password;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspace/organizations",
            post(create_organization).get(get_organization),
        )
        .route(
            "/api/v1/workspace/organizations/settings",
            put(update_settings),
        )
        .route("/api/v1/workspace/members", get(list_members))
        .route("/api/v1/workspace/members/invite", post(invite_member))
        .route(
            "/api/v1/workspace/invitations/accept",
            post(accept_invitation),
        )
        .route(
            "/api/v1/workspace/members/{user_id}/role",
            put(change_member_role),
        )
        .route(
            "/api/v1/workspace/members/{user_id}/suspend",
            post(suspend_member),
        )
        .route(
            "/api/v1/workspace/members/{user_id}/revoke",
            post(revoke_member),
        )
        .route("/api/v1/workspace/roles", get(list_roles).post(create_role))
        .route(
            "/api/v1/workspace/roles/{role_id}",
            get(get_role).put(update_role),
        )
        .route(
            "/api/v1/workspace/roles/{role_id}/preview",
            get(preview_role_capabilities),
        )
        .route("/api/v1/workspace/permissions", get(list_permissions))
        .route("/api/v1/workspace/me/capabilities", get(my_capabilities))
        .route("/api/v1/workspace/teams", get(list_teams).post(create_team))
        .route(
            "/api/v1/workspace/departments",
            get(list_departments).post(create_department),
        )
}

fn parse_user_ref(s: &str, request_id: &str) -> Result<Uuid, AppError> {
    if let Ok(u) = Uuid::parse_str(s) {
        return Ok(u);
    }
    let pub_id: PublicId = s
        .parse()
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, request_id, "bad user id"))?;
    Ok(pub_id.uuid())
}

async fn resolve_role(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    role_ref: &str,
    request_id: &str,
) -> Result<(Uuid, String, String), AppError> {
    // Returns (role_id, system_key_or_member, public_id)
    if let Ok(pub_id) = role_ref.parse::<PublicId>() {
        if pub_id.kind() == IdKind::Role {
            let row: Option<(Uuid, Option<String>, String)> = sqlx::query_as(
                "SELECT id, system_key, public_id FROM org_role WHERE org_id = $1 AND id = $2",
            )
            .bind(org_id)
            .bind(pub_id.uuid())
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
            let Some((id, system_key, public_id)) = row else {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    request_id,
                    "role not found",
                ));
            };
            let key = system_key.unwrap_or_else(|| "member".into());
            return Ok((id, key, public_id));
        }
    }
    if Role::parse(role_ref).is_some() {
        let row: Option<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, system_key, public_id FROM org_role WHERE org_id = $1 AND system_key = $2",
        )
        .bind(org_id)
        .bind(role_ref)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        let Some((id, system_key, public_id)) = row else {
            return Err(AppError::new(
                ErrorCode::NotFound,
                request_id,
                format!("system role '{role_ref}' not provisioned"),
            ));
        };
        return Ok((id, system_key, public_id));
    }
    Err(AppError::new(
        ErrorCode::ValidationFailed,
        request_id,
        "role must be rol_… or a system key",
    ))
}

async fn bump_all_policy_versions(
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

async fn bump_role_memberships(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    role_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE membership
        SET policy_version = policy_version + 1, updated_at = now()
        WHERE org_id = $1 AND role_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id)
    .bind(role_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/workspace/organizations", tag = "workspace",
    request_body = CreateOrgRequest, responses((status = 201, body = OrgResponse)))]
pub async fn create_organization(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<OrgResponse>), AppError> {
    let request_id = user.ctx.request_id.clone();
    if body.name.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "name required",
        ));
    }

    let org = OrgId::generate();
    let membership_id = new_uuid_v7();
    let mem_public = PublicId::new(IdKind::Membership, membership_id);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO organization (
            id, public_id, name, currency, timezone, business_type, plan
        ) VALUES ($1,$2,$3,$4,$5,$6,'starter')
        "#,
    )
    .bind(org.as_uuid())
    .bind(org.to_public().as_str())
    .bind(body.name.trim())
    .bind(body.currency.trim())
    .bind(body.timezone.trim())
    .bind(body.business_type.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO membership (
            id, org_id, user_id, public_id, role, policy_version, status
        ) VALUES ($1,$2,$3,$4,'owner',1,'active')
        "#,
    )
    .bind(membership_id)
    .bind(org.as_uuid())
    .bind(user.ctx.actor.user_id)
    .bind(mem_public.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let created = EventEnvelope::new(
        org,
        Context::Workspace,
        "organization",
        "created",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({
            "org_id": org.to_public().as_str(),
            "name": body.name.trim(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &created)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mem_evt = EventEnvelope::new(
        org,
        Context::Workspace,
        "membership",
        "created",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({
            "membership_id": mem_public.as_str(),
            "user_id": PublicId::new(IdKind::User, user.ctx.actor.user_id).as_str(),
            "role": "owner",
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &mem_evt)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    provisioning::enqueue_and_run(
        &state.pool,
        org,
        user.ctx.actor.user_id,
        body.business_type.trim(),
        &request_id,
    )
    .await?;

    audit_mutation(
        &state.pool,
        org.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.organization.created",
        "organization",
        &org.to_public().as_str(),
        serde_json::json!({ "name": body.name.trim() }),
    )
    .await;

    let org_resp = fetch_org(&state.pool, org, &request_id).await?;
    Ok((StatusCode::CREATED, Json(org_resp)))
}

async fn fetch_org(
    pool: &sqlx::PgPool,
    org: OrgId,
    request_id: &str,
) -> Result<OrgResponse, AppError> {
    let row: Option<(
        String,
        String,
        String,
        String,
        i32,
        String,
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, name, currency, timezone, fiscal_year_start_month,
               business_type, plan, numbering_series, branding, feature_flags
        FROM organization WHERE id = $1
        "#,
    )
    .bind(org.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let Some(r) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "organization not found",
        ));
    };
    Ok(OrgResponse {
        org_id: r.0,
        name: r.1,
        currency: r.2,
        timezone: r.3,
        fiscal_year_start_month: r.4,
        business_type: r.5,
        plan: r.6,
        numbering_series: r.7,
        branding: r.8,
        feature_flags: r.9,
    })
}

#[utoipa::path(get, path = "/api/v1/workspace/organizations", tag = "workspace",
    responses((status = 200, body = OrgResponse)))]
pub async fn get_organization(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<OrgResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_org_read())?;
    let org = fetch_org(&state.pool, user.ctx.org_id, &request_id).await?;
    Ok(Json(org))
}

#[utoipa::path(put, path = "/api/v1/workspace/organizations/settings", tag = "workspace",
    request_body = UpdateOrgSettingsRequest, responses((status = 200, body = OrgResponse)))]
pub async fn update_settings(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateOrgSettingsRequest>,
) -> Result<Json<OrgResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_org_update_settings())?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE organization SET
            name = COALESCE($2, name),
            currency = COALESCE($3, currency),
            timezone = COALESCE($4, timezone),
            fiscal_year_start_month = COALESCE($5, fiscal_year_start_month),
            numbering_series = COALESCE($6, numbering_series),
            branding = COALESCE($7, branding),
            business_type = COALESCE($8, business_type),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(user.ctx.org_id.as_uuid())
    .bind(body.name.as_deref())
    .bind(body.currency.as_deref())
    .bind(body.timezone.as_deref())
    .bind(body.fiscal_year_start_month)
    .bind(body.numbering_series.as_ref())
    .bind(body.branding.as_ref())
    .bind(body.business_type.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    bump_all_policy_versions(&mut tx, user.ctx.org_id.as_uuid())
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "organization",
        "settings_updated",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "org_id": user.ctx.org_id.to_public().as_str() }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    state.perm_cache.clear();

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.org.settings_updated",
        "organization",
        &user.ctx.org_id.to_public().as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok(Json(
        fetch_org(&state.pool, user.ctx.org_id, &request_id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workspace/members", tag = "workspace",
    responses((status = 200, body = MemberListResponse)))]
pub async fn list_members(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MemberListResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_member_read())?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let rows: Vec<(
        String,
        Uuid,
        String,
        String,
        String,
        String,
        Option<Uuid>,
        Option<String>,
        String,
        i64,
        Option<Uuid>,
        Option<Uuid>,
    )> = sqlx::query_as(
        r#"
        SELECT m.public_id, u.id, u.public_id, u.email, u.display_name,
               m.role, m.role_id, r.public_id, m.status, m.policy_version,
               m.team_id, m.department_id
        FROM membership m
        JOIN user_identity u ON u.id = m.user_id
        LEFT JOIN org_role r ON r.id = m.role_id
        WHERE m.org_id = $1
        ORDER BY m.created_at ASC
        "#,
    )
    .bind(user.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|r| MemberView {
            membership_id: r.0,
            user_id: r.2,
            email: r.3,
            display_name: r.4,
            role: r.5,
            role_id: r.7,
            role_name: None,
            status: r.8,
            policy_version: r.9,
            team_id: r.10.map(|id| PublicId::new(IdKind::Team, id).as_str()),
            department_id: r
                .11
                .map(|id| PublicId::new(IdKind::Department, id).as_str()),
        })
        .collect();
    Ok(Json(MemberListResponse { items }))
}

#[utoipa::path(post, path = "/api/v1/workspace/members/invite", tag = "workspace",
    request_body = InviteMemberRequest, responses((status = 201, body = InviteResponse)))]
pub async fn invite_member(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<InviteMemberRequest>,
) -> Result<(StatusCode, Json<InviteResponse>), AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_member_invite())?;

    let email_norm = body.email.trim().to_ascii_lowercase();
    if email_norm.is_empty() || !email_norm.contains('@') {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid email",
        ));
    }

    let invite_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Invitation, invite_id);
    let raw_token = generate_opaque_token();
    let token_hash = hash_token(&raw_token);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let (role_id, _key, _) =
        resolve_role(&mut tx, user.ctx.org_id.as_uuid(), &body.role, &request_id).await?;

    // Cannot invite into a different org than the token — org comes from AuthUser.
    let expires = chrono::Utc::now() + chrono::Duration::days(7);
    sqlx::query(
        r#"
        INSERT INTO invitation (
            id, org_id, public_id, email, email_normalized, role_id,
            invited_by, token_hash, status, expires_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'pending',$9)
        "#,
    )
    .bind(invite_id)
    .bind(user.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(body.email.trim())
    .bind(&email_norm)
    .bind(role_id)
    .bind(user.ctx.actor.user_id)
    .bind(&token_hash)
    .bind(expires)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "invitation",
        "created",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({
            "invitation_id": public_id.as_str(),
            "email": email_norm,
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let link = format!(
        "{}/invite/accept?token={}",
        mail::public_app_base(),
        urlencoding::encode(&raw_token)
    );
    let _ = mail::send_mail(OutboundMail {
        to: email_norm.clone(),
        subject: "You're invited to CompanyOS".into(),
        body: format!("Accept your invitation: {link}"),
    })
    .await;

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.member.invited",
        "invitation",
        &public_id.as_str(),
        serde_json::json!({ "email": email_norm }),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(InviteResponse {
            invitation_id: public_id.as_str(),
            email: email_norm,
            status: "pending".into(),
            expires_at: expires.to_rfc3339(),
        }),
    ))
}

#[utoipa::path(post, path = "/api/v1/workspace/invitations/accept", tag = "workspace",
    request_body = AcceptInviteRequest, responses((status = 200, body = MessageResponse)))]
pub async fn accept_invitation(
    State(state): State<AppState>,
    Json(body): Json<AcceptInviteRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = "invite-accept".to_string();
    let token_hash = hash_token(&body.token);

    let mut lookup = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    companyos_tenancy::set_invite_token_hash(&mut lookup, &token_hash)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let inv: Option<(
        Uuid,
        Uuid,
        String,
        Uuid,
        String,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        r#"
        SELECT id, org_id, email_normalized, role_id, status, expires_at
        FROM invitation WHERE token_hash = $1
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(&mut *lookup)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    lookup
        .commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((inv_id, org_uuid, email_norm, role_id, status, expires_at)) = inv else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "invitation not found",
        ));
    };
    if status != "pending" {
        return Err(AppError::new(
            ErrorCode::Conflict,
            request_id,
            "invitation not pending",
        ));
    }
    if expires_at < chrono::Utc::now() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invitation expired",
        ));
    }

    let org = OrgId::new(org_uuid);
    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(&email_norm)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let user_id = if let Some((id,)) = existing {
        id
    } else {
        let password = body.password.ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                "password required for new users",
            )
        })?;
        let (hash, salt) = password::hash_password(&password).map_err(|e| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                e.to_string(),
            )
        })?;
        let uid = new_uuid_v7();
        let display = body
            .display_name
            .clone()
            .unwrap_or_else(|| email_norm.clone());
        sqlx::query(
            r#"
            INSERT INTO user_identity (
                id, public_id, email, email_normalized, password_hash, password_salt,
                display_name, email_verified_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,now())
            "#,
        )
        .bind(uid)
        .bind(PublicId::new(IdKind::User, uid).as_str())
        .bind(&email_norm)
        .bind(&email_norm)
        .bind(&hash)
        .bind(&salt)
        .bind(display)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        uid
    };

    let role_key: String =
        sqlx::query_as("SELECT COALESCE(system_key, 'member') FROM org_role WHERE id = $1")
            .bind(role_id)
            .fetch_one(&state.pool)
            .await
            .map(|(k,): (String,)| k)
            .unwrap_or_else(|_| "member".into());

    let membership_id = new_uuid_v7();
    let mem_public = PublicId::new(IdKind::Membership, membership_id);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO membership (
            id, org_id, user_id, public_id, role, role_id, policy_version, status
        ) VALUES ($1,$2,$3,$4,$5,$6,1,'active')
        ON CONFLICT (org_id, user_id) DO UPDATE SET
            role = EXCLUDED.role,
            role_id = EXCLUDED.role_id,
            status = 'active',
            revoked_at = NULL,
            policy_version = membership.policy_version + 1,
            updated_at = now()
        "#,
    )
    .bind(membership_id)
    .bind(org_uuid)
    .bind(user_id)
    .bind(mem_public.as_str())
    .bind(&role_key)
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        UPDATE invitation SET status = 'accepted', accepted_at = now(),
            accepted_user_id = $2, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(inv_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let envelope = EventEnvelope::new(
        org,
        Context::Workspace,
        "membership",
        "created",
        1,
        Actor::human(user_id),
        serde_json::json!({
            "membership_id": mem_public.as_str(),
            "user_id": PublicId::new(IdKind::User, user_id).as_str(),
            "role": role_key,
            "via": "invitation",
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let role_evt = EventEnvelope::new(
        org,
        Context::Workspace,
        "role",
        "assigned",
        1,
        Actor::human(user_id),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, user_id).as_str(), "role": role_key }),
    );
    companyos_outbox::insert_event(&mut *tx, &role_evt)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "invitation accepted".into(),
    }))
}

#[utoipa::path(put, path = "/api/v1/workspace/members/{user_id}/role", tag = "workspace",
    request_body = ChangeRoleRequest, responses((status = 200, body = MessageResponse)))]
pub async fn change_member_role(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user): Path<String>,
    Json(body): Json<ChangeRoleRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_assign())?;
    let target = parse_user_ref(&target_user, &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let (role_id, role_key, _) =
        resolve_role(&mut tx, user.ctx.org_id.as_uuid(), &body.role, &request_id).await?;

    // Demoting an owner?
    let current: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT role, id FROM membership WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(user.ctx.org_id.as_uuid())
    .bind(target)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((old_role, membership_id)) = current else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "membership not found",
        ));
    };

    if old_role == "owner" && role_key != "owner" {
        last_owner::ensure_not_last_owner(&mut tx, user.ctx.org_id.as_uuid(), target, &request_id)
            .await?;
    }

    sqlx::query(
        r#"
        UPDATE membership
        SET role = $3, role_id = $4, policy_version = policy_version + 1, updated_at = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(membership_id)
    .bind(user.ctx.org_id.as_uuid())
    .bind(&role_key)
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let revoked = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "role",
        "revoked",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, target).as_str(), "role": old_role }),
    );
    companyos_outbox::insert_event(&mut *tx, &revoked)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let assigned = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "role",
        "assigned",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, target).as_str(), "role": role_key }),
    );
    companyos_outbox::insert_event(&mut *tx, &assigned)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    state
        .perm_cache
        .invalidate_membership(&membership_id.to_string());

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.role.assigned",
        "membership",
        &membership_id.to_string(),
        serde_json::json!({ "from": old_role, "to": role_key }),
    )
    .await;

    Ok(Json(MessageResponse {
        message: "role updated".into(),
    }))
}

#[utoipa::path(post, path = "/api/v1/workspace/members/{user_id}/suspend", tag = "workspace",
    responses((status = 200, body = MessageResponse)))]
pub async fn suspend_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_member_suspend())?;
    let target = parse_user_ref(&target_user, &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    last_owner::ensure_not_last_owner(&mut tx, user.ctx.org_id.as_uuid(), target, &request_id)
        .await?;

    let updated: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE membership
        SET status = 'suspended', suspended_at = now(),
            policy_version = policy_version + 1, updated_at = now()
        WHERE org_id = $1 AND user_id = $2 AND status = 'active' AND revoked_at IS NULL
        RETURNING id
        "#,
    )
    .bind(user.ctx.org_id.as_uuid())
    .bind(target)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if updated.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "active membership not found",
        ));
    }

    let _ =
        crate::auth::sessions::revoke_org_user_sessions(&mut tx, user.ctx.org_id.as_uuid(), target)
            .await;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "membership",
        "suspended",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, target).as_str() }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "membership suspended".into(),
    }))
}

#[utoipa::path(post, path = "/api/v1/workspace/members/{user_id}/revoke", tag = "workspace",
    responses((status = 200, body = MessageResponse)))]
pub async fn revoke_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_member_revoke())?;
    let target = parse_user_ref(&target_user, &request_id)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    last_owner::ensure_not_last_owner(&mut tx, user.ctx.org_id.as_uuid(), target, &request_id)
        .await?;

    let updated: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE membership
        SET status = 'revoked', revoked_at = now(),
            policy_version = policy_version + 1, updated_at = now()
        WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
        RETURNING id
        "#,
    )
    .bind(user.ctx.org_id.as_uuid())
    .bind(target)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if updated.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "membership not found",
        ));
    }

    let _ =
        crate::auth::sessions::revoke_org_user_sessions(&mut tx, user.ctx.org_id.as_uuid(), target)
            .await;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "membership",
        "revoked",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, target).as_str() }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let deactivated = EventEnvelope::new(
        user.ctx.org_id,
        Context::Workspace,
        "user",
        "deactivated",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({ "user_id": PublicId::new(IdKind::User, target).as_str() }),
    );
    companyos_outbox::insert_event(&mut *tx, &deactivated)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.membership.revoked",
        "membership",
        &target_user,
        serde_json::json!({}),
    )
    .await;

    Ok(Json(MessageResponse {
        message: "membership revoked".into(),
    }))
}

async fn load_role_view(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    role_id: Uuid,
    request_id: &str,
) -> Result<RoleView, AppError> {
    let row: Option<(
        String,
        String,
        String,
        Option<String>,
        bool,
        Option<i64>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, name, description, system_key, is_system,
               approval_limit_amount_minor, approval_limit_currency
        FROM org_role WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id)
    .bind(role_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let Some(r) = row else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "role not found",
        ));
    };

    let perms_rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT permission_id, effect, scope FROM role_permission WHERE role_id = $1",
    )
    .bind(role_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    Ok(RoleView {
        role_id: r.0,
        name: r.1,
        description: r.2,
        system_key: r.3,
        is_system: r.4,
        approval_limit_amount_minor: r.5,
        approval_limit_currency: r.6,
        permissions: perms_rows
            .into_iter()
            .map(|p| RolePermissionView {
                permission_id: p.0,
                effect: p.1,
                scope: p.2,
            })
            .collect(),
    })
}

#[utoipa::path(get, path = "/api/v1/workspace/roles", tag = "workspace",
    responses((status = 200, body = RoleListResponse)))]
pub async fn list_roles(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<RoleListResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_read())?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let ids: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 ORDER BY is_system DESC, name")
            .bind(user.ctx.org_id.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut items = Vec::new();
    for (id,) in ids {
        items.push(load_role_view(&mut tx, user.ctx.org_id.as_uuid(), id, &request_id).await?);
    }
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    Ok(Json(RoleListResponse { items }))
}

#[utoipa::path(get, path = "/api/v1/workspace/roles/{role_id}", tag = "workspace",
    responses((status = 200, body = RoleView)))]
pub async fn get_role(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role_id): Path<String>,
) -> Result<Json<RoleView>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_read())?;
    let rid = role_id
        .parse::<PublicId>()
        .map_err(|_| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                "bad role id",
            )
        })?
        .uuid();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let view = load_role_view(&mut tx, user.ctx.org_id.as_uuid(), rid, &request_id).await?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    Ok(Json(view))
}

async fn write_role_permissions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    role_id: Uuid,
    permissions: &[RolePermissionInput],
    request_id: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM role_permission WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    for p in permissions {
        if companyos_authz::validate_permission_id(&p.permission_id).is_err() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                format!("invalid permission_id {}", p.permission_id),
            ));
        }
        if p.effect != "allow" && p.effect != "deny" {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                request_id,
                "effect must be allow or deny",
            ));
        }
        let scope = if Scope::parse(&p.scope).is_some() {
            p.scope.as_str()
        } else {
            "organization"
        };
        sqlx::query(
            r#"
            INSERT INTO role_permission (id, org_id, role_id, permission_id, effect, scope)
            VALUES ($1,$2,$3,$4,$5,$6)
            ON CONFLICT (role_id, permission_id, effect) DO UPDATE SET scope = EXCLUDED.scope
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(role_id)
        .bind(&p.permission_id)
        .bind(&p.effect)
        .bind(scope)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/workspace/roles", tag = "workspace",
    request_body = UpsertRoleRequest, responses((status = 201, body = RoleView)))]
pub async fn create_role(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpsertRoleRequest>,
) -> Result<(StatusCode, Json<RoleView>), AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_manage())?;
    if body.name.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "name required",
        ));
    }

    let role_uuid = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Role, role_uuid);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO org_role (
            id, org_id, public_id, name, description, is_system,
            approval_limit_amount_minor, approval_limit_currency
        ) VALUES ($1,$2,$3,$4,$5,false,$6,$7)
        "#,
    )
    .bind(role_uuid)
    .bind(user.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(body.description.as_deref().unwrap_or(""))
    .bind(body.approval_limit_amount_minor)
    .bind(body.approval_limit_currency.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if e.to_string().contains("org_role") {
            AppError::new(ErrorCode::Conflict, request_id.clone(), "role name exists")
        } else {
            AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
        }
    })?;

    write_role_permissions(
        &mut tx,
        user.ctx.org_id.as_uuid(),
        role_uuid,
        &body.permissions,
        &request_id,
    )
    .await?;

    let view = load_role_view(&mut tx, user.ctx.org_id.as_uuid(), role_uuid, &request_id).await?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.role.created",
        "role",
        &public_id.as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok((StatusCode::CREATED, Json(view)))
}

#[utoipa::path(put, path = "/api/v1/workspace/roles/{role_id}", tag = "workspace",
    request_body = UpsertRoleRequest, responses((status = 200, body = RoleView)))]
pub async fn update_role(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role_id): Path<String>,
    Json(body): Json<UpsertRoleRequest>,
) -> Result<Json<RoleView>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_manage())?;
    let rid = role_id
        .parse::<PublicId>()
        .map_err(|_| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                "bad role id",
            )
        })?
        .uuid();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let meta: Option<(bool, Option<String>)> =
        sqlx::query_as("SELECT is_system, system_key FROM org_role WHERE org_id = $1 AND id = $2")
            .bind(user.ctx.org_id.as_uuid())
            .bind(rid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((is_system, system_key)) = meta else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            request_id,
            "role not found",
        ));
    };
    // Allow editing permissions on system roles except Owner template name/key.
    if is_system && system_key.as_deref() == Some("owner") {
        // Still allow permission matrix / approval limits updates on Owner? Safer to allow
        // approval limits only; permissions for Owner stay full. We still update limits.
    }

    sqlx::query(
        r#"
        UPDATE org_role SET
            name = CASE WHEN is_system THEN name ELSE $3 END,
            description = COALESCE($4, description),
            approval_limit_amount_minor = $5,
            approval_limit_currency = $6,
            updated_at = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(rid)
    .bind(user.ctx.org_id.as_uuid())
    .bind(body.name.trim())
    .bind(body.description.as_deref())
    .bind(body.approval_limit_amount_minor)
    .bind(body.approval_limit_currency.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    if system_key.as_deref() != Some("owner") {
        write_role_permissions(
            &mut tx,
            user.ctx.org_id.as_uuid(),
            rid,
            &body.permissions,
            &request_id,
        )
        .await?;
    }

    bump_role_memberships(&mut tx, user.ctx.org_id.as_uuid(), rid)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let view = load_role_view(&mut tx, user.ctx.org_id.as_uuid(), rid, &request_id).await?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    state.perm_cache.clear();

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.role.updated",
        "role",
        &role_id,
        serde_json::json!({}),
    )
    .await;

    Ok(Json(view))
}

#[utoipa::path(get, path = "/api/v1/workspace/roles/{role_id}/preview", tag = "workspace",
    responses((status = 200, body = CapabilityPreviewResponse)))]
pub async fn preview_role_capabilities(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role_id): Path<String>,
) -> Result<Json<CapabilityPreviewResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_role_read())?;
    let rid = role_id
        .parse::<PublicId>()
        .map_err(|_| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                "bad role id",
            )
        })?
        .uuid();

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let view = load_role_view(&mut tx, user.ctx.org_id.as_uuid(), rid, &request_id).await?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut roles = Vec::new();
    if let Some(ref key) = view.system_key {
        if let Some(r) = Role::parse(key) {
            roles.push(r);
        }
    }
    let mut statements = Vec::new();
    for p in &view.permissions {
        let effect = if p.effect == "deny" {
            Effect::Deny
        } else {
            Effect::Allow
        };
        statements.push(Statement {
            effect,
            permission: PermissionId::from(p.permission_id.as_str()),
            scope: Scope::parse(&p.scope).unwrap_or(Scope::Organization),
            conditions: vec![],
        });
    }
    let principal = Principal { roles, statements };
    let allowed: Vec<String> = principal
        .effective_allows()
        .into_iter()
        .map(|p| p.0)
        .collect();
    let denied_sensitive: Vec<String> = SENSITIVE_ACTIONS
        .iter()
        .filter(|p| !is_allowed(&principal, &PermissionId::from(**p)))
        .map(|s| (*s).to_string())
        .collect();

    Ok(Json(CapabilityPreviewResponse {
        role_id: view.role_id,
        allowed,
        denied_sensitive,
    }))
}

#[utoipa::path(get, path = "/api/v1/workspace/permissions", tag = "workspace",
    responses((status = 200, body = PermissionCatalogueResponse)))]
pub async fn list_permissions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<PermissionCatalogueResponse>, AppError> {
    let _ = require_perm(&user, &state, &perms::workspace_role_read())?;
    let items = PERMISSION_CATALOGUE
        .iter()
        .map(|p| PermissionCatalogueItem {
            id: p.id.into(),
            context: p.context.into(),
            resource: p.resource.into(),
            action: p.action.into(),
            description: p.description.into(),
            sensitive: p.sensitive,
        })
        .collect();
    Ok(Json(PermissionCatalogueResponse { items }))
}

#[utoipa::path(get, path = "/api/v1/workspace/me/capabilities", tag = "workspace",
    responses((status = 200, body = MyCapabilitiesResponse)))]
pub async fn my_capabilities(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MyCapabilitiesResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    let cache_key = user.membership_id.to_string();
    if let Some(cached) = state.perm_cache.get(&cache_key, user.policy_version) {
        let role = user.roles.first().cloned().unwrap_or_default();
        return Ok(Json(MyCapabilitiesResponse {
            org_id: user.ctx.org_id.to_public().as_str(),
            role,
            policy_version: user.policy_version,
            allowed: cached.into_iter().collect(),
        }));
    }

    let (principal, policy_version, _) = super::load_principal(
        &state.pool,
        user.ctx.org_id,
        user.ctx.actor.user_id,
        &request_id,
    )
    .await
    .unwrap_or_else(|_| {
        (
            Principal::with_roles(user.roles.iter().filter_map(|r| Role::parse(r)).collect()),
            user.policy_version,
            user.membership_id,
        )
    });

    let allowed_set = principal.effective_allows();
    let allowed: Vec<String> = allowed_set.iter().map(|p| p.0.clone()).collect();
    state.perm_cache.put(
        &cache_key,
        policy_version,
        allowed.iter().cloned().collect(),
    );

    Ok(Json(MyCapabilitiesResponse {
        org_id: user.ctx.org_id.to_public().as_str(),
        role: user.roles.first().cloned().unwrap_or_default(),
        policy_version,
        allowed,
    }))
}

#[utoipa::path(get, path = "/api/v1/workspace/teams", tag = "workspace",
    responses((status = 200, body = TeamListResponse)))]
pub async fn list_teams(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<TeamListResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_team_read())?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<(String, String, Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT public_id, name, department_id, parent_team_id, lead_user_id FROM team WHERE org_id = $1 ORDER BY name",
    )
    .bind(user.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    Ok(Json(TeamListResponse {
        items: rows
            .into_iter()
            .map(|r| TeamView {
                team_id: r.0,
                name: r.1,
                department_id: r.2.map(|id| PublicId::new(IdKind::Department, id).as_str()),
                parent_team_id: r.3.map(|id| PublicId::new(IdKind::Team, id).as_str()),
                lead_user_id: r.4.map(|id| PublicId::new(IdKind::User, id).as_str()),
            })
            .collect(),
    }))
}

#[utoipa::path(post, path = "/api/v1/workspace/teams", tag = "workspace",
    request_body = CreateTeamRequest, responses((status = 201, body = TeamView)))]
pub async fn create_team(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamView>), AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_team_manage())?;
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Team, id);
    let dept = body
        .department_id
        .as_ref()
        .and_then(|s| s.parse::<PublicId>().ok())
        .map(|p| p.uuid());
    let parent = body
        .parent_team_id
        .as_ref()
        .and_then(|s| s.parse::<PublicId>().ok())
        .map(|p| p.uuid());
    let lead = body
        .lead_user_id
        .as_ref()
        .and_then(|s| parse_user_ref(s, &request_id).ok());

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO team (id, org_id, public_id, name, department_id, parent_team_id, lead_user_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(id)
    .bind(user.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(dept)
    .bind(parent)
    .bind(lead)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.team.created",
        "team",
        &public_id.as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(TeamView {
            team_id: public_id.as_str(),
            name: body.name.trim().into(),
            department_id: body.department_id,
            parent_team_id: body.parent_team_id,
            lead_user_id: body.lead_user_id,
        }),
    ))
}

#[utoipa::path(get, path = "/api/v1/workspace/departments", tag = "workspace",
    responses((status = 200, body = DepartmentListResponse)))]
pub async fn list_departments(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<DepartmentListResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_department_read())?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let rows: Vec<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT public_id, name, parent_id FROM department WHERE org_id = $1 ORDER BY name",
    )
    .bind(user.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    Ok(Json(DepartmentListResponse {
        items: rows
            .into_iter()
            .map(|r| DepartmentView {
                department_id: r.0,
                name: r.1,
                parent_id: r.2.map(|id| PublicId::new(IdKind::Department, id).as_str()),
            })
            .collect(),
    }))
}

#[utoipa::path(post, path = "/api/v1/workspace/departments", tag = "workspace",
    request_body = CreateDepartmentRequest, responses((status = 201, body = DepartmentView)))]
pub async fn create_department(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateDepartmentRequest>,
) -> Result<(StatusCode, Json<DepartmentView>), AppError> {
    let request_id = user.ctx.request_id.clone();
    require_perm(&user, &state, &perms::workspace_department_manage())?;
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::Department, id);
    let parent = body
        .parent_id
        .as_ref()
        .and_then(|s| s.parse::<PublicId>().ok())
        .map(|p| p.uuid());

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query(
        "INSERT INTO department (id, org_id, public_id, name, parent_id) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(user.ctx.org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(body.name.trim())
    .bind(parent)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit_mutation(
        &state.pool,
        user.ctx.org_id.as_uuid(),
        user.ctx.actor.user_id,
        "workspace.department.created",
        "department",
        &public_id.as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(DepartmentView {
            department_id: public_id.as_str(),
            name: body.name.trim().into(),
            parent_id: body.parent_id,
        }),
    ))
}
