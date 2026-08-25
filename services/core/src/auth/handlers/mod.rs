//! Auth HTTP handlers — `/api/v1/auth/...`.

mod flows;
pub mod types;

use std::time::Duration;

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_authz::{principal_requires_mfa, Principal, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_auth_lookup_user, set_session_org_id, Actor, OrgId};
use uuid::Uuid;

use super::audit;
use super::breach::{is_breached, BreachCheckMode};
use super::lockout;
use super::mail::{self, OutboundMail};
use super::password;
use super::sessions;
use crate::state::AppState;
use types::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(flows::logout))
        .route("/api/v1/auth/refresh", post(flows::refresh))
        .route("/api/v1/auth/verify-email", post(flows::verify_email))
        .route(
            "/api/v1/auth/resend-verification",
            post(flows::resend_verification),
        )
        .route("/api/v1/auth/magic-link", post(flows::request_magic_link))
        .route(
            "/api/v1/auth/magic-link/consume",
            post(flows::consume_magic_link),
        )
        .route(
            "/api/v1/auth/password-reset/request",
            post(flows::request_password_reset),
        )
        .route(
            "/api/v1/auth/password-reset/confirm",
            post(flows::confirm_password_reset),
        )
        .route("/api/v1/auth/mfa/setup", post(flows::mfa_setup))
        .route("/api/v1/auth/mfa/confirm", post(flows::mfa_confirm))
        .route("/api/v1/auth/mfa/verify", post(flows::mfa_verify))
        .route("/api/v1/auth/switch-org", post(flows::switch_org))
        .route("/api/v1/auth/sessions", get(flows::list_sessions))
        .route("/api/v1/auth/sessions/{id}", delete(flows::revoke_session))
        .route("/api/v1/auth/sessions", delete(flows::revoke_all_sessions))
        .route(
            "/api/v1/auth/oauth/{provider}/start",
            get(flows::oauth_start),
        )
        .route(
            "/api/v1/auth/oauth/{provider}/callback",
            get(flows::oauth_callback),
        )
        .route("/api/v1/auth/me", get(flows::me))
        .route("/api/v1/auth/memberships", get(flows::list_memberships))
        .route(
            "/api/v1/auth/memberships/{user_id}/revoke",
            post(flows::revoke_membership),
        )
        .route(
            "/api/v1/auth/sso/configs",
            get(flows::list_sso).post(flows::create_sso),
        )
        .route("/api/v1/auth/jwks.json", get(flows::jwks))
        .route("/api/v1/auth/jwks/rotate", post(flows::rotate_jwks))
}

pub(super) fn req_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

pub(super) fn client_meta(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    (ip, ua)
}

pub(super) fn cookie_secure() -> bool {
    !matches!(
        std::env::var("AUTH_COOKIE_SECURE").as_deref(),
        Ok("0") | Ok("false") | Ok("FALSE")
    ) && std::env::var("PUBLIC_APP_URL")
        .map(|u| u.starts_with("https://"))
        .unwrap_or(false)
}

pub(super) fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub(super) fn token_response(issued: sessions::IssuedTokens) -> Response {
    let body = TokenResponse {
        access_token: issued.access_token,
        token_type: "Bearer".into(),
        expires_in: issued.expires_in,
        session_id: issued.session_id.to_string(),
    };
    let cookie = sessions::refresh_cookie(&issued.refresh_token, cookie_secure());
    let mut res = (StatusCode::OK, Json(body)).into_response();
    if let Ok(val) = HeaderValue::from_str(&cookie.to_string()) {
        res.headers_mut().insert(header::SET_COOKIE, val);
    }
    res
}

pub(super) fn mfa_challenge_response(
    challenge_token: String,
    request_id: &str,
) -> Result<Response, AppError> {
    let body = MfaChallengeResponse {
        mfa_required: true,
        challenge_token,
        message: "MFA required for Owner/Admin".into(),
    };
    let mut res = (StatusCode::UNAUTHORIZED, Json(body)).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    // Also emit problem+json-compatible code via header for clients.
    let _ = request_id;
    Ok(res)
}

pub(super) async fn rate_limit(
    state: &AppState,
    key: &str,
    request_id: &str,
) -> Result<(), AppError> {
    match state.rate_limiter.check_and_hit(key) {
        Ok(delay) => {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            Ok(())
        }
        Err(delay) => {
            tokio::time::sleep(delay.min(Duration::from_secs(2))).await;
            Err(AppError::new(
                ErrorCode::TooManyRequests,
                request_id,
                format!("rate limited; retry after {}ms", delay.as_millis()),
            ))
        }
    }
}

pub(super) async fn idempotent_get(
    pool: &sqlx::PgPool,
    scope: &str,
    key: &str,
) -> Result<Option<(i32, serde_json::Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT response_status, response_body FROM auth_idempotency WHERE scope = $1 AND key = $2",
    )
    .bind(scope)
    .bind(key)
    .fetch_optional(pool)
    .await
}

pub(super) async fn idempotent_put(
    pool: &sqlx::PgPool,
    scope: &str,
    key: &str,
    status: i32,
    body: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO auth_idempotency (id, scope, key, response_status, response_body)
        VALUES ($1,$2,$3,$4,$5)
        ON CONFLICT (scope, key) DO NOTHING
        "#,
    )
    .bind(new_uuid_v7())
    .bind(scope)
    .bind(key)
    .bind(status)
    .bind(body)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// POST /api/v1/auth/register
#[utoipa::path(post, path = "/api/v1/auth/register", tag = "auth",
    request_body = RegisterRequest,
    responses((status = 201, body = RegisterResponse)))]
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    let request_id = req_id(&headers);
    rate_limit(
        &state,
        &format!("register:{}", client_meta(&headers).0.unwrap_or_default()),
        &request_id,
    )
    .await?;

    let email_norm = normalize_email(&body.email);
    if email_norm.is_empty() || !email_norm.contains('@') {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid email",
        ));
    }
    if body.org_name.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "org_name required",
        ));
    }

    let breach_mode = BreachCheckMode::from_env();
    if is_breached(&breach_mode, &body.password)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "password appears in a breach list; choose another",
        ));
    }

    let (password_hash, password_salt) = password::hash_password(&body.password).map_err(|e| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            e.to_string(),
        )
    })?;

    let user_id = new_uuid_v7();
    let user_public = PublicId::new(IdKind::User, user_id);
    let org = OrgId::generate();
    let membership_id = new_uuid_v7();
    let membership_public = format!("mem_{membership_id}");

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO organization (id, public_id, name) VALUES ($1,$2,$3)
        "#,
    )
    .bind(org.as_uuid())
    .bind(org.to_public().as_str())
    .bind(body.org_name.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let insert_user = sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt, display_name
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        "#,
    )
    .bind(user_id)
    .bind(user_public.as_str())
    .bind(body.email.trim())
    .bind(&email_norm)
    .bind(&password_hash)
    .bind(&password_salt)
    .bind(body.display_name.trim())
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert_user {
        if e.to_string().contains("user_identity_email_normalized") {
            return Err(AppError::new(
                ErrorCode::Conflict,
                request_id,
                "email already registered",
            ));
        }
        return Err(AppError::new(
            ErrorCode::Internal,
            request_id,
            e.to_string(),
        ));
    }

    set_session_org_id(&mut tx, org)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, policy_version)
        VALUES ($1,$2,$3,$4,'owner',1)
        "#,
    )
    .bind(membership_id)
    .bind(org.as_uuid())
    .bind(user_id)
    .bind(&membership_public)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let envelope = EventEnvelope::new(
        org,
        Context::Auth,
        "membership",
        "created",
        1,
        Actor::human(user_id),
        serde_json::json!({
            "membership_id": membership_public,
            "user_id": user_public.as_str(),
            "role": "owner",
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let verify_raw = generate_opaque_token();
    sqlx::query(
        r#"
        INSERT INTO email_token (id, user_id, purpose, token_hash, org_id, expires_at)
        VALUES ($1,$2,'email_verify',$3,$4, now() + interval '48 hours')
        "#,
    )
    .bind(new_uuid_v7())
    .bind(user_id)
    .bind(hash_token(&verify_raw))
    .bind(org.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let link = format!(
        "{}/verify-email?token={}",
        mail::public_app_base(),
        urlencoding::encode(&verify_raw)
    );
    let _ = mail::send_mail(OutboundMail {
        to: email_norm.clone(),
        subject: "Verify your CompanyOS email".into(),
        body: format!("Verify your email: {link}"),
    })
    .await;

    let (ip, ua) = client_meta(&headers);
    audit::record(
        &state.pool,
        Some(org.as_uuid()),
        Some(user_id),
        "auth.register",
        ip.as_deref(),
        ua.as_deref(),
        serde_json::json!({ "email": email_norm }),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            user_id: user_public.as_str(),
            org_id: org.to_public().as_str(),
            email: email_norm,
            verification_required: true,
        }),
    ))
}

/// POST /api/v1/auth/login
#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", request_body = LoginRequest)]
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    let (ip, ua) = client_meta(&headers);
    let rl_key = format!(
        "login:{}:{}",
        normalize_email(&body.email),
        ip.clone().unwrap_or_default()
    );
    rate_limit(&state, &rl_key, &request_id).await?;

    let email_norm = normalize_email(&body.email);
    #[allow(clippy::type_complexity)]
    let user: Option<(
        Uuid,
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        r#"
            SELECT id, public_id, password_hash, email_verified_at, locked_until, mfa_enabled_at
            FROM user_identity WHERE email_normalized = $1
            "#,
    )
    .bind(&email_norm)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((user_id, user_public, password_hash, verified_at, locked_until, mfa_at)) = user
    else {
        state.rate_limiter.register_failure(&rl_key);
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid email or password",
        ));
    };

    if locked_until.is_some_and(|u| u > chrono::Utc::now()) {
        return Err(AppError::new(
            ErrorCode::AccountLocked,
            request_id,
            "account locked due to failed attempts; try later",
        ));
    }

    let Some(hash) = password_hash else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid email or password",
        ));
    };

    if password::verify_password(&body.password, &hash).is_err() {
        let locked = lockout::record_failure(&state.pool, user_id)
            .await
            .unwrap_or(false);
        state.rate_limiter.register_failure(&rl_key);
        audit::record(
            &state.pool,
            None,
            Some(user_id),
            "auth.login_failed",
            ip.as_deref(),
            ua.as_deref(),
            serde_json::json!({}),
        )
        .await;
        if locked {
            return Err(AppError::new(
                ErrorCode::AccountLocked,
                request_id,
                "account locked due to failed attempts",
            ));
        }
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid email or password",
        ));
    }

    if verified_at.is_none() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "email not verified",
        ));
    }

    let memberships: Vec<(Uuid, Uuid, String, String, i64)> = {
        let mut tx =
            state.pool.begin().await.map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
        set_auth_lookup_user(&mut tx, user_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        let rows = sqlx::query_as(
            r#"
        SELECT m.id, m.org_id, o.public_id, m.role, m.policy_version
        FROM membership m
        JOIN organization o ON o.id = m.org_id
        WHERE m.user_id = $1 AND m.revoked_at IS NULL
        ORDER BY m.created_at ASC
        "#,
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        rows
    };

    if memberships.is_empty() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "no active organization membership",
        ));
    }

    let (membership_id, org_uuid, org_public, role, policy_version) =
        if let Some(wanted) = body.org_id.as_deref() {
            memberships
                .into_iter()
                .find(|m| m.2 == wanted)
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::Forbidden,
                        request_id.clone(),
                        "not a member of requested org",
                    )
                })?
        } else {
            memberships.into_iter().next().unwrap()
        };

    let roles = vec![role.clone()];
    let principal = Principal::with_roles(roles.iter().filter_map(|r| Role::parse(r)).collect());
    if principal_requires_mfa(&principal) && mfa_at.is_none() {
        // Still allow login to reach MFA setup, but do not issue access token.
        // Issue a short-lived MFA challenge token stored as email_token purpose mfa_pending.
        let challenge = generate_opaque_token();
        sqlx::query(
            r#"
            INSERT INTO email_token (id, user_id, purpose, token_hash, org_id, payload, expires_at)
            VALUES ($1,$2,'mfa_pending',$3,$4,$5, now() + interval '10 minutes')
            "#,
        )
        .bind(new_uuid_v7())
        .bind(user_id)
        .bind(hash_token(&challenge))
        .bind(org_uuid)
        .bind(serde_json::json!({
            "membership_id": membership_id,
            "role": role,
            "policy_version": policy_version,
            "needs_setup": true
        }))
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

        audit::record(
            &state.pool,
            Some(org_uuid),
            Some(user_id),
            "auth.mfa_required",
            ip.as_deref(),
            ua.as_deref(),
            serde_json::json!({ "needs_setup": true }),
        )
        .await;

        return mfa_challenge_response(challenge, &request_id);
    }

    if principal_requires_mfa(&principal) && mfa_at.is_some() {
        let challenge = generate_opaque_token();
        sqlx::query(
            r#"
            INSERT INTO email_token (id, user_id, purpose, token_hash, org_id, payload, expires_at)
            VALUES ($1,$2,'mfa_pending',$3,$4,$5, now() + interval '10 minutes')
            "#,
        )
        .bind(new_uuid_v7())
        .bind(user_id)
        .bind(hash_token(&challenge))
        .bind(org_uuid)
        .bind(serde_json::json!({
            "membership_id": membership_id,
            "role": role,
            "policy_version": policy_version,
            "needs_setup": false
        }))
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

        return mfa_challenge_response(challenge, &request_id);
    }

    let _ = lockout::clear_failures(&state.pool, user_id).await;
    state.rate_limiter.reset(&rl_key);

    let org = OrgId::new(org_uuid);
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user_id,
        &user_public,
        org,
        membership_id,
        &roles,
        policy_version,
        body.device_label.as_deref(),
        ua.as_deref(),
        ip.as_deref(),
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit::record(
        &state.pool,
        Some(org_uuid),
        Some(user_id),
        "auth.login",
        ip.as_deref(),
        ua.as_deref(),
        serde_json::json!({ "org_id": org_public }),
    )
    .await;

    Ok(token_response(issued))
}
