//! SCIM 2.0 Users + Groups provisioning (mock IdP friendly) + admin token management.

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::extract::AuthUser;
use crate::auth::password;
use crate::governance::{authorize, internal};
use crate::state::AppState;
use crate::workspace;

pub fn admin_router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/governance/scim/tokens",
        get(list_tokens).post(create_token),
    )
}

pub fn scim_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/scim/v2/Users", get(list_users).post(create_user))
        .route(
            "/api/v1/scim/v2/Users/{id}",
            get(get_user)
                .put(replace_user)
                .patch(patch_user)
                .delete(delete_user),
        )
        .route(
            "/api/v1/scim/v2/Groups",
            get(list_groups).post(create_group),
        )
        .route(
            "/api/v1/scim/v2/Groups/{id}",
            get(get_group)
                .put(replace_group)
                .patch(patch_group)
                .delete(delete_group),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateScimTokenRequest {
    pub name: String,
    #[serde(default = "default_idp")]
    pub idp_label: String,
}

fn default_idp() -> String {
    "default".into()
}

#[derive(Debug, Serialize)]
pub struct ScimTokenDto {
    pub id: String,
    pub name: String,
    pub idp_label: String,
    pub token_prefix: String,
    pub created_at: chrono::DateTime<Utc>,
    pub revoked_at: Option<chrono::DateTime<Utc>>,
    /// Present only on create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn mint_scim_token() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let raw = format!("scim_{}", hex::encode(bytes));
    let prefix: String = raw.chars().take(12).collect();
    let hash = hash_token(&raw);
    (raw, prefix, hash)
}

async fn create_token(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateScimTokenRequest>,
) -> Result<Json<ScimTokenDto>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_scim_manage()).await?;
    let org_id = user.ctx.org_id.as_uuid();
    let actor = user.ctx.actor.user_id;

    let (raw, prefix, hash) = mint_scim_token();
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::ScimToken, id);

    let mut tx = state.pool.begin().await.map_err(internal(&request_id))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let created_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO scim_token (
            id, org_id, public_id, name, token_prefix, token_hash, idp_label, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING created_at
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(public_id.as_str())
    .bind(&body.name)
    .bind(&prefix)
    .bind(&hash)
    .bind(&body.idp_label)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    sqlx::query(
        r#"
        INSERT INTO scim_token_lookup (token_hash, org_id, token_id, revoked_at)
        VALUES ($1,$2,$3,NULL)
        "#,
    )
    .bind(&hash)
    .bind(org_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal(&request_id))?;

    tx.commit().await.map_err(internal(&request_id))?;

    workspace::audit_mutation(
        &state.pool,
        org_id,
        actor,
        "scim.token.create",
        "scim_token",
        &public_id.to_string(),
        serde_json::json!({ "idp_label": body.idp_label, "name": body.name }),
    )
    .await;

    Ok(Json(ScimTokenDto {
        id: public_id.to_string(),
        name: body.name,
        idp_label: body.idp_label,
        token_prefix: prefix,
        created_at,
        revoked_at: None,
        token: Some(raw),
    }))
}

async fn list_tokens(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ScimTokenDto>>, AppError> {
    let request_id = user.ctx.request_id.clone();
    authorize(&state, &user, &perms::admin_scim_manage()).await?;
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
        chrono::DateTime<Utc>,
        Option<chrono::DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"
        SELECT public_id, name, idp_label, token_prefix, created_at, revoked_at
        FROM scim_token WHERE org_id = $1 ORDER BY created_at DESC
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
                |(id, name, idp_label, token_prefix, created_at, revoked_at)| ScimTokenDto {
                    id,
                    name,
                    idp_label,
                    token_prefix,
                    created_at,
                    revoked_at,
                    token: None,
                },
            )
            .collect(),
    ))
}

/// Authenticated SCIM caller (org-scoped bearer token).
pub struct ScimAuth {
    pub org_id: OrgId,
    pub token_id: Uuid,
    pub request_id: String,
}

impl FromRequestParts<AppState> for ScimAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = parts
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("scim")
            .to_string();

        let auth = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                AppError::new(ErrorCode::Unauthorized, &request_id, "missing bearer token")
            })?;
        let raw = auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Unauthorized,
                    &request_id,
                    "expected Bearer token",
                )
            })?;
        let hash = hash_token(raw.trim());

        let row: Option<(Uuid, Uuid, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
            "SELECT org_id, token_id, revoked_at FROM scim_token_lookup WHERE token_hash = $1",
        )
        .bind(&hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal(&request_id))?;

        let Some((org_uuid, token_id, revoked_at)) = row else {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "invalid SCIM token",
            ));
        };
        if revoked_at.is_some() {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "SCIM token revoked",
            ));
        }

        // Touch last_used (best-effort).
        let _ = sqlx::query("UPDATE scim_token SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(&state.pool)
            .await;

        Ok(ScimAuth {
            org_id: OrgId::new(org_uuid),
            token_id,
            request_id,
        })
    }
}

fn scim_json(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/scim+json")],
        Json(body),
    )
        .into_response()
}

fn scim_error(status: StatusCode, detail: &str) -> Response {
    scim_json(
        status,
        json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:Error"],
            "detail": detail,
            "status": status.as_u16().to_string(),
        }),
    )
}

fn user_resource(
    external_id: &str,
    user_public: &str,
    user_name: &str,
    email: &str,
    display: &str,
    active: bool,
) -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
        "id": external_id,
        "externalId": external_id,
        "userName": user_name,
        "displayName": display,
        "active": active,
        "emails": [{ "value": email, "primary": true }],
        "meta": {
            "resourceType": "User",
            "location": format!("/api/v1/scim/v2/Users/{external_id}"),
        },
        "companyos:userId": user_public,
    })
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ListQuery {
    filter: Option<String>,
    start_index: Option<i64>,
    count: Option<i64>,
}

async fn list_users(
    State(state): State<AppState>,
    auth: ScimAuth,
    Query(_q): Query<ListQuery>,
) -> Response {
    let _ = (&_q, &auth.request_id);
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(e) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let rows: Result<Vec<(String, Option<Uuid>, bool, String)>, _> = sqlx::query_as(
        r#"
        SELECT s.external_id, s.user_id, s.active, COALESCE(u.email, '')
        FROM scim_external_identity s
        LEFT JOIN user_identity u ON u.id = s.user_id
        WHERE s.org_id = $1 AND s.resource_type = 'User'
        ORDER BY s.created_at
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.commit().await;
    let Ok(rows) = rows else {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
    };

    let mut resources = Vec::new();
    for (ext, user_id, active, email) in rows {
        let (user_public, display, user_name) = if let Some(uid) = user_id {
            let r: Option<(String, String, String)> = sqlx::query_as(
                "SELECT public_id, display_name, email FROM user_identity WHERE id = $1",
            )
            .bind(uid)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);
            r.unwrap_or_else(|| (String::new(), email.clone(), email.clone()))
        } else {
            (String::new(), email.clone(), email.clone())
        };
        resources.push(user_resource(
            &ext,
            &user_public,
            &user_name,
            &email,
            &display,
            active,
        ));
    }
    let total = resources.len();
    scim_json(
        StatusCode::OK,
        json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
            "totalResults": total,
            "startIndex": 1,
            "itemsPerPage": total,
            "Resources": resources,
        }),
    )
}

#[derive(Debug, Deserialize)]
struct ScimUserBody {
    #[serde(default)]
    schemas: Vec<String>,
    #[serde(rename = "userName")]
    user_name: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "externalId")]
    external_id: Option<String>,
    active: Option<bool>,
    emails: Option<Vec<ScimEmail>>,
}

#[derive(Debug, Deserialize)]
struct ScimEmail {
    value: String,
    #[serde(default)]
    primary: bool,
}

async fn create_user(
    State(state): State<AppState>,
    auth: ScimAuth,
    Json(body): Json<ScimUserBody>,
) -> Response {
    let request_id = auth.request_id.clone();
    let email = body
        .emails
        .as_ref()
        .and_then(|e| e.iter().find(|x| x.primary).or(e.first()))
        .map(|e| e.value.to_lowercase())
        .or_else(|| body.user_name.as_ref().map(|s| s.to_lowercase()));
    let Some(email) = email else {
        return scim_error(StatusCode::BAD_REQUEST, "userName or emails required");
    };
    let external_id = body
        .external_id
        .clone()
        .unwrap_or_else(|| format!("scim-user-{}", new_uuid_v7()));
    let display = body.display_name.clone().unwrap_or_else(|| email.clone());
    let active = body.active.unwrap_or(true);

    // Unusable password for SCIM-provisioned users (login via SSO/SCIM session only).
    let (hash, salt) = match password::hash_password(&format!("scim-disabled-{}", new_uuid_v7())) {
        Ok(v) => v,
        Err(e) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(e) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }

    let uid = new_uuid_v7();
    let user_public = PublicId::new(IdKind::User, uid);
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,now())
        ON CONFLICT (email_normalized) DO NOTHING
        "#,
    )
    .bind(uid)
    .bind(user_public.as_str())
    .bind(&email)
    .bind(&email)
    .bind(&hash)
    .bind(&salt)
    .bind(&display)
    .execute(&mut *tx)
    .await
    {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    // Resolve user id (existing or new).
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(&email)
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(uid);
    let user_public = PublicId::new(IdKind::User, user_id);

    let member_role_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'member'")
            .bind(auth.org_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .ok()
            .flatten();

    let membership_id = new_uuid_v7();
    let mem_public = PublicId::new(IdKind::Membership, membership_id);
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO membership (
            id, org_id, user_id, public_id, role, role_id, status, policy_version
        ) VALUES ($1,$2,$3,$4,'member',$5,'active',1)
        ON CONFLICT (org_id, user_id) DO UPDATE SET
            revoked_at = NULL, status = 'active', updated_at = now()
        "#,
    )
    .bind(membership_id)
    .bind(auth.org_id.as_uuid())
    .bind(user_id)
    .bind(mem_public.as_str())
    .bind(member_role_id)
    .execute(&mut *tx)
    .await
    {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    let sid = new_uuid_v7();
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO scim_external_identity (
            id, org_id, resource_type, external_id, user_id, active, raw_payload
        ) VALUES ($1,$2,'User',$3,$4,$5,$6)
        ON CONFLICT (org_id, resource_type, external_id) DO UPDATE SET
            user_id = EXCLUDED.user_id,
            active = EXCLUDED.active,
            raw_payload = EXCLUDED.raw_payload,
            updated_at = now()
        "#,
    )
    .bind(sid)
    .bind(auth.org_id.as_uuid())
    .bind(&external_id)
    .bind(user_id)
    .bind(active)
    .bind(json!(&body.schemas))
    .execute(&mut *tx)
    .await
    {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    if tx.commit().await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "commit");
    }

    workspace::audit_mutation(
        &state.pool,
        auth.org_id.as_uuid(),
        user_id,
        "scim.user.create",
        "scim_user",
        &external_id,
        json!({ "email": email, "token_id": auth.token_id }),
    )
    .await;

    let _ = request_id;
    scim_json(
        StatusCode::CREATED,
        user_resource(
            &external_id,
            &user_public.to_string(),
            &email,
            &email,
            &display,
            active,
        ),
    )
}

async fn get_user(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let row: Option<(String, Option<Uuid>, bool)> = sqlx::query_as(
        r#"
        SELECT external_id, user_id, active FROM scim_external_identity
        WHERE org_id = $1 AND resource_type = 'User' AND external_id = $2
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let _ = tx.commit().await;
    let Some((ext, Some(user_id), active)) = row else {
        return scim_error(StatusCode::NOT_FOUND, "User not found");
    };
    let info: Option<(String, String, String)> =
        sqlx::query_as("SELECT public_id, display_name, email FROM user_identity WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((pub_id, display, email)) = info else {
        return scim_error(StatusCode::NOT_FOUND, "User not found");
    };
    scim_json(
        StatusCode::OK,
        user_resource(&ext, &pub_id, &email, &email, &display, active),
    )
}

async fn replace_user(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
    Json(body): Json<ScimUserBody>,
) -> Response {
    patch_user_inner(state, auth, id, body).await
}

async fn patch_user(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    // Minimal PATCH: support active false via Operations or top-level active.
    let active = body.get("active").and_then(|v| v.as_bool()).or_else(|| {
        body.get("Operations").and_then(|ops| {
            ops.as_array().and_then(|arr| {
                arr.iter().find_map(|op| {
                    let path = op.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    if path.eq_ignore_ascii_case("active") {
                        op.get("value").and_then(|v| v.as_bool())
                    } else {
                        None
                    }
                })
            })
        })
    });
    let mut scim_body = ScimUserBody {
        schemas: vec![],
        user_name: None,
        display_name: None,
        external_id: Some(id.clone()),
        active,
        emails: None,
    };
    if let Some(un) = body.get("userName").and_then(|v| v.as_str()) {
        scim_body.user_name = Some(un.to_string());
    }
    patch_user_inner(state, auth, id, scim_body).await
}

async fn patch_user_inner(
    state: AppState,
    auth: ScimAuth,
    id: String,
    body: ScimUserBody,
) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let row: Option<(Uuid, bool)> = sqlx::query_as(
        r#"
        SELECT user_id, active FROM scim_external_identity
        WHERE org_id = $1 AND resource_type = 'User' AND external_id = $2
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let Some((user_id, prev_active)) = row else {
        return scim_error(StatusCode::NOT_FOUND, "User not found");
    };
    let active = body.active.unwrap_or(prev_active);
    let _ = sqlx::query(
        "UPDATE scim_external_identity SET active = $3, updated_at = now()
         WHERE org_id = $1 AND external_id = $2 AND resource_type = 'User'",
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .bind(active)
    .execute(&mut *tx)
    .await;

    if !active {
        let _ = sqlx::query(
            "UPDATE membership SET revoked_at = now(), status = 'revoked', updated_at = now()
             WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(auth.org_id.as_uuid())
        .bind(user_id)
        .execute(&mut *tx)
        .await;
        let _ = sqlx::query(
            "UPDATE auth_session SET revoked_at = now(), revoke_reason = 'scim_deprovision'
             WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(auth.org_id.as_uuid())
        .bind(user_id)
        .execute(&mut *tx)
        .await;
    } else {
        let _ = sqlx::query(
            "UPDATE membership SET revoked_at = NULL, status = 'active', updated_at = now()
             WHERE org_id = $1 AND user_id = $2",
        )
        .bind(auth.org_id.as_uuid())
        .bind(user_id)
        .execute(&mut *tx)
        .await;
    }
    let _ = tx.commit().await;

    workspace::audit_mutation(
        &state.pool,
        auth.org_id.as_uuid(),
        user_id,
        if active {
            "scim.user.update"
        } else {
            "scim.user.deprovision"
        },
        "scim_user",
        &id,
        json!({ "active": active, "token_id": auth.token_id }),
    )
    .await;

    get_user(State(state), auth, Path(id)).await
}

async fn delete_user(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
) -> Response {
    let body = ScimUserBody {
        schemas: vec![],
        user_name: None,
        display_name: None,
        external_id: Some(id.clone()),
        active: Some(false),
        emails: None,
    };
    let _ = patch_user_inner(state, auth, id, body).await;
    (StatusCode::NO_CONTENT).into_response()
}

fn group_resource(
    external_id: &str,
    display: &str,
    members: &[String],
    team_public: &str,
) -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Group"],
        "id": external_id,
        "externalId": external_id,
        "displayName": display,
        "members": members.iter().map(|m| json!({ "value": m })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "Group",
            "location": format!("/api/v1/scim/v2/Groups/{external_id}"),
        },
        "companyos:teamId": team_public,
    })
}

#[derive(Debug, Deserialize)]
struct ScimGroupBody {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "externalId")]
    external_id: Option<String>,
    members: Option<Vec<Value>>,
}

async fn list_groups(State(state): State<AppState>, auth: ScimAuth) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let rows: Vec<(String, Option<Uuid>, Value)> = sqlx::query_as(
        r#"
        SELECT external_id, team_id, raw_payload FROM scim_external_identity
        WHERE org_id = $1 AND resource_type = 'Group' ORDER BY created_at
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();
    let _ = tx.commit().await;

    let mut resources = Vec::new();
    for (ext, team_id, raw) in rows {
        let display = raw
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or(ext.as_str())
            .to_string();
        let team_public = if let Some(tid) = team_id {
            sqlx::query_scalar::<_, String>("SELECT public_id FROM team WHERE id = $1")
                .bind(tid)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
        } else {
            String::new()
        };
        let members: Vec<String> = raw
            .get("members")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("value").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        resources.push(group_resource(&ext, &display, &members, &team_public));
    }
    let total = resources.len();
    scim_json(
        StatusCode::OK,
        json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
            "totalResults": total,
            "Resources": resources,
        }),
    )
}

async fn create_group(
    State(state): State<AppState>,
    auth: ScimAuth,
    Json(body): Json<ScimGroupBody>,
) -> Response {
    let display = body
        .display_name
        .clone()
        .unwrap_or_else(|| "SCIM Group".into());
    let external_id = body
        .external_id
        .clone()
        .unwrap_or_else(|| format!("scim-group-{}", new_uuid_v7()));

    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }

    let team_id = new_uuid_v7();
    let team_public = PublicId::new(IdKind::Team, team_id);
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO team (id, org_id, public_id, name, created_at, updated_at)
        VALUES ($1,$2,$3,$4,now(),now())
        "#,
    )
    .bind(team_id)
    .bind(auth.org_id.as_uuid())
    .bind(team_public.to_string())
    .bind(&display)
    .execute(&mut *tx)
    .await
    {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    let members = body.members.clone().unwrap_or_default();
    let sid = new_uuid_v7();
    let payload = json!({ "displayName": display, "members": members });
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO scim_external_identity (
            id, org_id, resource_type, external_id, team_id, active, raw_payload
        ) VALUES ($1,$2,'Group',$3,$4,true,$5)
        "#,
    )
    .bind(sid)
    .bind(auth.org_id.as_uuid())
    .bind(&external_id)
    .bind(team_id)
    .bind(&payload)
    .execute(&mut *tx)
    .await
    {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    let _ = tx.commit().await;

    let member_ids: Vec<String> = members
        .iter()
        .filter_map(|m| m.get("value").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    scim_json(
        StatusCode::CREATED,
        group_resource(
            &external_id,
            &display,
            &member_ids,
            &team_public.to_string(),
        ),
    )
}

async fn get_group(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let row: Option<(String, Option<Uuid>, Value)> = sqlx::query_as(
        r#"
        SELECT external_id, team_id, raw_payload FROM scim_external_identity
        WHERE org_id = $1 AND resource_type = 'Group' AND external_id = $2
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let _ = tx.commit().await;
    let Some((ext, team_id, raw)) = row else {
        return scim_error(StatusCode::NOT_FOUND, "Group not found");
    };
    let display = raw
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or(&ext)
        .to_string();
    let team_public = if let Some(tid) = team_id {
        sqlx::query_scalar::<_, String>("SELECT public_id FROM team WHERE id = $1")
            .bind(tid)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let members: Vec<String> = raw
        .get("members")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("value").and_then(|v| v.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    scim_json(
        StatusCode::OK,
        group_resource(&ext, &display, &members, &team_public),
    )
}

async fn replace_group(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
    Json(body): Json<ScimGroupBody>,
) -> Response {
    patch_group_inner(state, auth, id, body).await
}

async fn patch_group(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let display = body
        .get("displayName")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let members = body.get("members").and_then(|v| v.as_array()).cloned();
    patch_group_inner(
        state,
        auth,
        id,
        ScimGroupBody {
            display_name: display,
            external_id: None,
            members,
        },
    )
    .await
}

async fn patch_group_inner(
    state: AppState,
    auth: ScimAuth,
    id: String,
    body: ScimGroupBody,
) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let row: Option<(Option<Uuid>, Value)> = sqlx::query_as(
        r#"
        SELECT team_id, raw_payload FROM scim_external_identity
        WHERE org_id = $1 AND resource_type = 'Group' AND external_id = $2
        "#,
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let Some((team_id, mut raw)) = row else {
        return scim_error(StatusCode::NOT_FOUND, "Group not found");
    };
    if let Some(d) = body.display_name {
        raw["displayName"] = json!(d);
        if let Some(tid) = team_id {
            let _ = sqlx::query(
                "UPDATE team SET name = $2, updated_at = now() WHERE id = $1 AND org_id = $3",
            )
            .bind(tid)
            .bind(d)
            .bind(auth.org_id.as_uuid())
            .execute(&mut *tx)
            .await;
        }
    }
    if let Some(m) = body.members {
        raw["members"] = json!(m);
    }
    let _ = sqlx::query(
        "UPDATE scim_external_identity SET raw_payload = $3, updated_at = now()
         WHERE org_id = $1 AND external_id = $2 AND resource_type = 'Group'",
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .bind(&raw)
    .execute(&mut *tx)
    .await;
    let _ = tx.commit().await;
    get_group(State(state), auth, Path(id)).await
}

async fn delete_group(
    State(state): State<AppState>,
    auth: ScimAuth,
    Path(id): Path<String>,
) -> Response {
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(_) => return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "db"),
    };
    if set_session_org_id(&mut tx, auth.org_id).await.is_err() {
        return scim_error(StatusCode::INTERNAL_SERVER_ERROR, "tenancy");
    }
    let _ = sqlx::query(
        "UPDATE scim_external_identity SET active = false, updated_at = now()
         WHERE org_id = $1 AND resource_type = 'Group' AND external_id = $2",
    )
    .bind(auth.org_id.as_uuid())
    .bind(&id)
    .execute(&mut *tx)
    .await;
    let _ = tx.commit().await;
    (StatusCode::NO_CONTENT).into_response()
}
