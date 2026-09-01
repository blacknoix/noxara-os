//! Remaining auth flows: verify, magic link, reset, MFA, sessions, OAuth, SSO.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_authz::{self as authz, perms, Principal, Role};
use companyos_errors::{AppError, ErrorCode};
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_auth_lookup_user, set_session_org_id, Actor, OrgId};
use serde::Deserialize;
use uuid::Uuid;

use super::types::*;
use super::{
    client_meta, cookie_secure, idempotency_key, idempotent_get, idempotent_put,
    mfa_challenge_response, normalize_email, rate_limit, req_id, token_response,
};
use crate::auth::audit;
use crate::auth::breach::{is_breached, BreachCheckMode};
use crate::auth::extract::AuthUser;
use crate::auth::lockout;
use crate::auth::mail::{self, OutboundMail};
use crate::auth::mfa;
use crate::auth::oauth::{self, OAuthProvider};
use crate::auth::password;
use crate::auth::sessions::{self, RefreshOutcome};
use crate::auth::sso::{self, UpsertSsoRequest};
use crate::auth::sso_login;
use crate::state::AppState;

fn refresh_from_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(v) =
            part.strip_prefix(&format!("{}=", companyos_auth_token::REFRESH_COOKIE_NAME))
        {
            return Some(v.to_string());
        }
    }
    None
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    // Best-effort: if Bearer present and valid, revoke that session.
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        if let Ok(claims) = companyos_auth_token::verify_access_token(&state.auth_keys.ring, auth) {
            let _ = sessions::revoke_session(&state.pool, claims.user_id, claims.sid).await;
            audit::record(
                &state.pool,
                Some(claims.org_uuid),
                Some(claims.user_id),
                "auth.logout",
                None,
                None,
                serde_json::json!({}),
            )
            .await;
        }
    }
    let cookie = sessions::clear_refresh_cookie(cookie_secure());
    let mut res = (
        StatusCode::OK,
        Json(MessageResponse {
            message: "logged out".into(),
        }),
    )
        .into_response();
    if let Ok(val) = HeaderValue::from_str(&cookie.to_string()) {
        res.headers_mut().insert(header::SET_COOKIE, val);
    }
    let _ = request_id;
    Ok(res)
}

pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    rate_limit(&state, &format!("refresh:{}", request_id), &request_id).await?;
    let Some(rt) = refresh_from_cookie(&headers) else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "refresh cookie missing",
        ));
    };
    match sessions::rotate_refresh(&state.pool, &state.auth_keys.ring, &rt).await {
        Ok(RefreshOutcome::Issued(issued)) => {
            audit::record(
                &state.pool,
                None,
                None,
                "auth.refresh",
                None,
                None,
                serde_json::json!({ "session_id": issued.session_id }),
            )
            .await;
            Ok(token_response(issued))
        }
        Ok(RefreshOutcome::ReuseDetected { family_id }) => {
            audit::record(
                &state.pool,
                None,
                None,
                "auth.refresh_reuse_detected",
                None,
                None,
                serde_json::json!({ "family_id": family_id }),
            )
            .await;
            let cookie = sessions::clear_refresh_cookie(cookie_secure());
            let mut res = AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "refresh token reuse detected; family revoked",
            )
            .into_response();
            if let Ok(val) = HeaderValue::from_str(&cookie.to_string()) {
                res.headers_mut().insert(header::SET_COOKIE, val);
            }
            Ok(res)
        }
        Err(e) => Err(AppError::new(ErrorCode::Unauthorized, request_id, e)),
    }
}

pub async fn verify_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<VerifyEmailRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = req_id(&headers);
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT id, user_id FROM email_token
        WHERE token_hash = $1 AND purpose = 'email_verify'
          AND consumed_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(hash_token(&body.token))
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((token_id, user_id)) = row else {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid or expired verification token",
        ));
    };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query("UPDATE email_token SET consumed_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit::record(
        &state.pool,
        None,
        Some(user_id),
        "auth.email_verified",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    Ok(Json(MessageResponse {
        message: "email verified".into(),
    }))
}

pub async fn resend_verification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResendVerificationRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = req_id(&headers);
    rate_limit(
        &state,
        &format!("resend_verify:{}", normalize_email(&body.email)),
        &request_id,
    )
    .await?;

    if let Some(key) = idempotency_key(&headers) {
        if let Ok(Some(_)) = idempotent_get(&state.pool, "resend_verification", &key).await {
            return Ok(Json(MessageResponse {
                message: "if the account exists, a verification email was sent".into(),
            }));
        }
        let _ = idempotent_put(
            &state.pool,
            "resend_verification",
            &key,
            200,
            serde_json::json!({"message": "ok"}),
        )
        .await;
    }

    let email_norm = normalize_email(&body.email);
    if let Ok(Some((user_id,))) = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM user_identity WHERE email_normalized = $1 AND email_verified_at IS NULL",
    )
    .bind(&email_norm)
    .fetch_optional(&state.pool)
    .await
    {
        let raw = generate_opaque_token();
        let _ = sqlx::query(
            r#"
            INSERT INTO email_token (id, user_id, purpose, token_hash, expires_at)
            VALUES ($1,$2,'email_verify',$3, now() + interval '48 hours')
            "#,
        )
        .bind(new_uuid_v7())
        .bind(user_id)
        .bind(hash_token(&raw))
        .execute(&state.pool)
        .await;
        let link = format!(
            "{}/verify-email?token={}",
            mail::public_app_base(),
            urlencoding::encode(&raw)
        );
        let _ = mail::send_mail(OutboundMail {
            to: email_norm,
            subject: "Verify your CompanyOS email".into(),
            body: format!("Verify your email: {link}"),
        })
        .await;
    }

    Ok(Json(MessageResponse {
        message: "if the account exists, a verification email was sent".into(),
    }))
}

pub async fn request_magic_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MagicLinkRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = req_id(&headers);
    rate_limit(
        &state,
        &format!("magic:{}", normalize_email(&body.email)),
        &request_id,
    )
    .await?;

    if let Some(key) = idempotency_key(&headers) {
        if let Ok(Some(_)) = idempotent_get(&state.pool, "magic_link", &key).await {
            return Ok(Json(MessageResponse {
                message: "if the account exists, a magic link was sent".into(),
            }));
        }
        let _ = idempotent_put(
            &state.pool,
            "magic_link",
            &key,
            200,
            serde_json::json!({"ok": true}),
        )
        .await;
    }

    let email_norm = normalize_email(&body.email);
    if let Ok(Some((user_id,))) =
        sqlx::query_as::<_, (Uuid,)>("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(&email_norm)
            .fetch_optional(&state.pool)
            .await
    {
        let raw = generate_opaque_token();
        let org_uuid = if let Some(org_public) = body.org_id.as_deref() {
            let pub_id: PublicId = org_public.parse().map_err(|_| {
                AppError::new(
                    ErrorCode::ValidationFailed,
                    request_id.clone(),
                    "bad org_id",
                )
            })?;
            Some(pub_id.uuid())
        } else {
            None
        };
        let _ = sqlx::query(
            r#"
            INSERT INTO email_token (id, user_id, purpose, token_hash, org_id, expires_at)
            VALUES ($1,$2,'magic_link',$3,$4, now() + interval '15 minutes')
            "#,
        )
        .bind(new_uuid_v7())
        .bind(user_id)
        .bind(hash_token(&raw))
        .bind(org_uuid)
        .execute(&state.pool)
        .await;
        let link = format!(
            "{}/magic-link?token={}",
            mail::public_app_base(),
            urlencoding::encode(&raw)
        );
        let _ = mail::send_mail(OutboundMail {
            to: email_norm,
            subject: "Your CompanyOS magic link".into(),
            body: format!("Sign in: {link}"),
        })
        .await;
        audit::record(
            &state.pool,
            org_uuid,
            Some(user_id),
            "auth.magic_link_sent",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
    }

    Ok(Json(MessageResponse {
        message: "if the account exists, a magic link was sent".into(),
    }))
}

pub async fn consume_magic_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MagicLinkConsumeRequest>,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    let (ip, ua) = client_meta(&headers);
    let row: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
        r#"
        SELECT id, user_id, org_id FROM email_token
        WHERE token_hash = $1 AND purpose = 'magic_link'
          AND consumed_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(hash_token(&body.token))
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((token_id, user_id, preferred_org)) = row else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid or expired magic link",
        ));
    };

    sqlx::query("UPDATE email_token SET consumed_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    issue_for_user(
        &state,
        user_id,
        preferred_org,
        body.device_label.as_deref(),
        ip.as_deref(),
        ua.as_deref(),
        &request_id,
    )
    .await
}

async fn issue_for_user(
    state: &AppState,
    user_id: Uuid,
    preferred_org: Option<Uuid>,
    device_label: Option<&str>,
    ip: Option<&str>,
    ua: Option<&str>,
    request_id: &str,
) -> Result<Response, AppError> {
    let user: (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT public_id, mfa_enabled_at FROM user_identity WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;

    let memberships: Vec<(Uuid, Uuid, String, i64)> = {
        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        set_auth_lookup_user(&mut tx, user_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        let rows = sqlx::query_as(
            r#"
        SELECT id, org_id, role, policy_version FROM membership
        WHERE user_id = $1 AND revoked_at IS NULL AND status = 'active'
        ORDER BY created_at ASC
        "#,
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        rows
    };

    let membership = if let Some(org) = preferred_org {
        memberships.into_iter().find(|m| m.1 == org)
    } else {
        memberships.into_iter().next()
    }
    .ok_or_else(|| {
        AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "no active organization membership",
        )
    })?;

    let roles = vec![membership.2.clone()];
    let principal = Principal::with_roles(roles.iter().filter_map(|r| Role::parse(r)).collect());
    if authz::principal_requires_mfa(&principal) {
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
        .bind(membership.1)
        .bind(serde_json::json!({
            "membership_id": membership.0,
            "role": membership.2,
            "policy_version": membership.3,
            "needs_setup": user.1.is_none()
        }))
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
        return mfa_challenge_response(challenge, request_id);
    }

    let org = OrgId::new(membership.1);
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user_id,
        &user.0,
        org,
        membership.0,
        &roles,
        membership.3,
        device_label,
        ua,
        ip,
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e))?;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(token_response(issued))
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PasswordResetRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = req_id(&headers);
    rate_limit(
        &state,
        &format!("pwreset:{}", normalize_email(&body.email)),
        &request_id,
    )
    .await?;

    if let Some(key) = idempotency_key(&headers) {
        if let Ok(Some(_)) = idempotent_get(&state.pool, "password_reset", &key).await {
            return Ok(Json(MessageResponse {
                message: "if the account exists, a reset email was sent".into(),
            }));
        }
        let _ = idempotent_put(
            &state.pool,
            "password_reset",
            &key,
            200,
            serde_json::json!({"ok": true}),
        )
        .await;
    }

    let email_norm = normalize_email(&body.email);
    if let Ok(Some((user_id,))) =
        sqlx::query_as::<_, (Uuid,)>("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(&email_norm)
            .fetch_optional(&state.pool)
            .await
    {
        let raw = generate_opaque_token();
        let _ = sqlx::query(
            r#"
            INSERT INTO email_token (id, user_id, purpose, token_hash, expires_at)
            VALUES ($1,$2,'password_reset',$3, now() + interval '1 hour')
            "#,
        )
        .bind(new_uuid_v7())
        .bind(user_id)
        .bind(hash_token(&raw))
        .execute(&state.pool)
        .await;
        let link = format!(
            "{}/reset-password?token={}",
            mail::public_app_base(),
            urlencoding::encode(&raw)
        );
        let _ = mail::send_mail(OutboundMail {
            to: email_norm,
            subject: "Reset your CompanyOS password".into(),
            body: format!("Reset password: {link}"),
        })
        .await;
        audit::record(
            &state.pool,
            None,
            Some(user_id),
            "auth.password_reset_requested",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
    }

    Ok(Json(MessageResponse {
        message: "if the account exists, a reset email was sent".into(),
    }))
}

pub async fn confirm_password_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PasswordResetConfirm>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = req_id(&headers);
    let breach_mode = BreachCheckMode::from_env();
    if is_breached(&breach_mode, &body.new_password)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "password appears in a breach list",
        ));
    }
    let (hash, salt) = password::hash_password(&body.new_password).map_err(|e| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            e.to_string(),
        )
    })?;

    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT id, user_id FROM email_token
        WHERE token_hash = $1 AND purpose = 'password_reset'
          AND consumed_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(hash_token(&body.token))
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((token_id, user_id)) = row else {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "invalid or expired reset token",
        ));
    };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query(
        r#"
        UPDATE user_identity
        SET password_hash = $2, password_salt = $3, failed_login_count = 0,
            locked_until = NULL, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .bind(&hash)
    .bind(&salt)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query("UPDATE email_token SET consumed_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let _ = sessions::revoke_all_sessions(&state.pool, user_id).await;
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit::record(
        &state.pool,
        None,
        Some(user_id),
        "auth.password_reset_completed",
        None,
        None,
        serde_json::json!({}),
    )
    .await;

    Ok(Json(MessageResponse {
        message: "password updated".into(),
    }))
}

pub async fn mfa_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MfaSetupRequest>,
) -> Result<Json<MfaSetupResponse>, AppError> {
    let request_id = req_id(&headers);
    let user_id = if let Some(challenge) = body.challenge_token.as_deref() {
        let row: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
            r#"
            SELECT user_id, payload FROM email_token
            WHERE token_hash = $1 AND purpose = 'mfa_pending'
              AND consumed_at IS NULL AND expires_at > now()
            "#,
        )
        .bind(hash_token(challenge))
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        let Some((uid, payload)) = row else {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                request_id,
                "invalid MFA challenge",
            ));
        };
        let _ = payload.get("needs_setup");
        uid
    } else {
        // Require Bearer for already-authenticated setup.
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Unauthorized,
                    request_id.clone(),
                    "Bearer or challenge_token required",
                )
            })?;
        let claims = companyos_auth_token::verify_access_token(&state.auth_keys.ring, auth)
            .map_err(|e| {
                AppError::new(ErrorCode::Unauthorized, request_id.clone(), e.to_string())
            })?;
        claims.user_id
    };

    let secret = mfa::generate_totp_secret();
    let email: (String,) = sqlx::query_as("SELECT email FROM user_identity WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let uri = mfa::provisioning_uri(&secret, &email.0)
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?;
    sqlx::query(
        "UPDATE user_identity SET mfa_totp_secret_encrypted = $2, updated_at = now() WHERE id = $1",
    )
    .bind(user_id)
    .bind(&secret)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id, e.to_string()))?;
    Ok(Json(MfaSetupResponse {
        secret,
        otpauth_uri: uri,
    }))
}

pub async fn mfa_confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MfaConfirmRequest>,
) -> Result<Json<MfaConfirmResponse>, AppError> {
    let request_id = req_id(&headers);
    let user_id = if let Some(challenge) = body.challenge_token.as_deref() {
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT user_id FROM email_token
            WHERE token_hash = $1 AND purpose = 'mfa_pending'
              AND consumed_at IS NULL AND expires_at > now()
            "#,
        )
        .bind(hash_token(challenge))
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        row.map(|r| r.0).ok_or_else(|| {
            AppError::new(
                ErrorCode::Unauthorized,
                request_id.clone(),
                "invalid MFA challenge",
            )
        })?
    } else {
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Unauthorized,
                    request_id.clone(),
                    "Bearer or challenge_token required",
                )
            })?;
        companyos_auth_token::verify_access_token(&state.auth_keys.ring, auth)
            .map_err(|e| AppError::new(ErrorCode::Unauthorized, request_id.clone(), e.to_string()))?
            .user_id
    };

    let row: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT mfa_totp_secret_encrypted, email FROM user_identity WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let Some((Some(secret), email)) = row else {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            request_id,
            "call mfa/setup first",
        ));
    };
    let ok = mfa::verify_totp(&secret, &email, &body.code)
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?;
    if !ok {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid MFA code",
        ));
    }
    let (plain, hashes) = mfa::generate_recovery_codes(10);
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query(
        "UPDATE user_identity SET mfa_enabled_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    sqlx::query("DELETE FROM mfa_recovery_code WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    for h in &hashes {
        sqlx::query("INSERT INTO mfa_recovery_code (id, user_id, code_hash) VALUES ($1,$2,$3)")
            .bind(new_uuid_v7())
            .bind(user_id)
            .bind(h)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    audit::record(
        &state.pool,
        None,
        Some(user_id),
        "auth.mfa_enabled",
        None,
        None,
        serde_json::json!({}),
    )
    .await;
    Ok(Json(MfaConfirmResponse {
        recovery_codes: plain,
        enabled: true,
    }))
}

pub async fn mfa_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MfaVerifyRequest>,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    let (ip, ua) = client_meta(&headers);
    rate_limit(&state, &format!("mfa_verify:{}", request_id), &request_id).await?;

    let row: Option<(Uuid, Uuid, Option<Uuid>, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT id, user_id, org_id, payload FROM email_token
        WHERE token_hash = $1 AND purpose = 'mfa_pending'
          AND consumed_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(hash_token(&body.challenge_token))
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let Some((token_id, user_id, org_id, payload)) = row else {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid MFA challenge",
        ));
    };

    let user_row: (String, Option<String>, String) = sqlx::query_as(
        "SELECT public_id, mfa_totp_secret_encrypted, email FROM user_identity WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let mut ok = false;
    if let Some(code) = body.code.as_deref() {
        if let Some(secret) = user_row.1.as_deref() {
            ok = mfa::verify_totp(secret, &user_row.2, code).unwrap_or(false);
        }
    }
    if !ok {
        if let Some(rc) = body.recovery_code.as_deref() {
            let codes: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT id, code_hash FROM mfa_recovery_code WHERE user_id = $1 AND used_at IS NULL",
            )
            .bind(user_id)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
            for (id, hash) in codes {
                if mfa::recovery_code_matches(rc, &hash) {
                    sqlx::query("UPDATE mfa_recovery_code SET used_at = now() WHERE id = $1")
                        .bind(id)
                        .execute(&state.pool)
                        .await
                        .ok();
                    ok = true;
                    break;
                }
            }
        }
    }
    if !ok {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            "invalid MFA code",
        ));
    }

    sqlx::query("UPDATE email_token SET consumed_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(&state.pool)
        .await
        .ok();

    let membership_id: Uuid = payload
        .get("membership_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| {
            payload
                .get("membership_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .or_else(|| {
            payload
                .get("membership_id")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        })
        .unwrap_or_else(Uuid::nil);

    // membership_id may be serialized as UUID in JSON
    let membership_id = if membership_id.is_nil() {
        payload
            .get("membership_id")
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    Uuid::parse_str(s).ok()
                } else {
                    serde_json::from_value::<Uuid>(v.clone()).ok()
                }
            })
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Internal,
                    request_id.clone(),
                    "challenge missing membership_id",
                )
            })?
    } else {
        membership_id
    };

    let role = payload
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("member")
        .to_string();
    let policy_version = payload
        .get("policy_version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let org_uuid = org_id.ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            request_id.clone(),
            "challenge missing org",
        )
    })?;

    let _ = lockout::clear_failures(&state.pool, user_id).await;
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
        &user_row.0,
        org,
        membership_id,
        &[role],
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
        "auth.mfa_verified",
        ip.as_deref(),
        ua.as_deref(),
        serde_json::json!({}),
    )
    .await;

    Ok(token_response(issued))
}

pub async fn switch_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<SwitchOrgRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let request_id = req_id(&headers);
    let org_pub: PublicId = body.org_id.parse().map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "invalid org_id",
        )
    })?;
    let org = OrgId::from_public(&org_pub).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "org_id must be org_…",
        )
    })?;

    let membership: Option<(Uuid, String, i64)> = {
        let mut tx =
            state.pool.begin().await.map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
        set_auth_lookup_user(&mut tx, user.ctx.actor.user_id)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        let row = sqlx::query_as(
            r#"
        SELECT id, role, policy_version FROM membership
        WHERE user_id = $1 AND org_id = $2 AND revoked_at IS NULL AND status = 'active'
        "#,
        )
        .bind(user.ctx.actor.user_id)
        .bind(org.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        row
    };

    let Some((membership_id, role, policy_version)) = membership else {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "not a member of target org",
        ));
    };

    // Move session to new org (same family) and mint NEW access token.
    sqlx::query(
        r#"
        UPDATE auth_session SET org_id = $2, membership_id = $3, last_seen_at = now()
        WHERE id = $1 AND user_id = $4
        "#,
    )
    .bind(user.session_id)
    .bind(org.as_uuid())
    .bind(membership_id)
    .bind(user.ctx.actor.user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let user_public = PublicId::new(IdKind::User, user.ctx.actor.user_id).as_str();
    let mut region_conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    let region = sessions::load_org_region(&mut region_conn, org)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?;
    let access = sessions::switch_org_access(
        &state.auth_keys.ring,
        user.ctx.actor.user_id,
        &user_public,
        org,
        membership_id,
        &[role],
        policy_version,
        user.session_id,
        user.family_id,
        &region,
    )
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e))?;

    audit::record(
        &state.pool,
        Some(org.as_uuid()),
        Some(user.ctx.actor.user_id),
        "auth.switch_org",
        None,
        None,
        serde_json::json!({ "org_id": body.org_id }),
    )
    .await;

    Ok(Json(TokenResponse {
        access_token: access,
        token_type: "Bearer".into(),
        expires_in: companyos_auth_token::ACCESS_TOKEN_TTL_SECS,
        session_id: user.session_id.to_string(),
    }))
}

pub async fn list_sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SessionListResponse>, AppError> {
    let items = sessions::list_sessions(&state.pool, user.ctx.actor.user_id, Some(user.session_id))
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
    Ok(Json(SessionListResponse { items }))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let sid = Uuid::parse_str(&id).map_err(|_| {
        AppError::new(
            ErrorCode::ValidationFailed,
            user.ctx.request_id.clone(),
            "invalid session id",
        )
    })?;
    let ok = sessions::revoke_session(&state.pool, user.ctx.actor.user_id, sid)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
    if !ok {
        return Err(AppError::new(
            ErrorCode::NotFound,
            user.ctx.request_id.clone(),
            "session not found",
        ));
    }
    Ok(Json(MessageResponse {
        message: "session revoked".into(),
    }))
}

pub async fn revoke_all_sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MessageResponse>, AppError> {
    let n = sessions::revoke_all_sessions(&state.pool, user.ctx.actor.user_id)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
    Ok(Json(MessageResponse {
        message: format!("revoked {n} sessions"),
    }))
}

pub async fn me(user: AuthUser) -> Json<MeResponse> {
    Json(MeResponse {
        user_id: PublicId::new(IdKind::User, user.ctx.actor.user_id).as_str(),
        org_id: user.ctx.org_id.to_public().as_str(),
        roles: user.roles,
        policy_version: user.policy_version,
        session_id: user.session_id.to_string(),
    })
}

pub async fn list_memberships(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MembershipListResponse>, AppError> {
    let rows: Vec<(String, String, String, i64)> = {
        let mut tx = state.pool.begin().await.map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
        set_auth_lookup_user(&mut tx, user.ctx.actor.user_id)
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCode::Internal,
                    user.ctx.request_id.clone(),
                    e.to_string(),
                )
            })?;
        let rows = sqlx::query_as(
            r#"
        SELECT o.public_id, o.name, m.role, m.policy_version
        FROM membership m
        JOIN organization o ON o.id = m.org_id
        WHERE m.user_id = $1 AND m.revoked_at IS NULL AND m.status = 'active'
        ORDER BY o.name
        "#,
        )
        .bind(user.ctx.actor.user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
        tx.commit().await.map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
        rows
    };
    Ok(Json(MembershipListResponse {
        items: rows
            .into_iter()
            .map(|r| MembershipView {
                org_id: r.0,
                org_name: r.1,
                role: r.2,
                policy_version: r.3,
            })
            .collect(),
    }))
}

pub async fn revoke_membership(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let request_id = user.ctx.request_id.clone();
    let principal =
        Principal::with_roles(user.roles.iter().filter_map(|r| Role::parse(r)).collect());
    if !authz::is_allowed(&principal, &perms::admin_membership_manage()) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            request_id,
            "missing admin.membership.manage",
        ));
    }
    let target = if let Ok(u) = Uuid::parse_str(&target_user) {
        u
    } else {
        let pub_id: PublicId = target_user.parse().map_err(|_| {
            AppError::new(
                ErrorCode::ValidationFailed,
                request_id.clone(),
                "bad user id",
            )
        })?;
        pub_id.uuid()
    };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
    set_session_org_id(&mut tx, user.ctx.org_id)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    crate::workspace::last_owner::ensure_not_last_owner(
        &mut tx,
        user.ctx.org_id.as_uuid(),
        target,
        &request_id,
    )
    .await?;

    let updated: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE membership
        SET revoked_at = now(), status = 'revoked',
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

    let _ = sessions::revoke_org_user_sessions(&mut tx, user.ctx.org_id.as_uuid(), target).await;

    let envelope = EventEnvelope::new(
        user.ctx.org_id,
        Context::Auth,
        "membership",
        "revoked",
        1,
        user.ctx.actor.clone(),
        serde_json::json!({
            "user_id": PublicId::new(IdKind::User, target).as_str(),
        }),
    );
    companyos_outbox::insert_event(&mut *tx, &envelope)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    audit::record(
        &state.pool,
        Some(user.ctx.org_id.as_uuid()),
        Some(user.ctx.actor.user_id),
        "auth.membership_revoked",
        None,
        None,
        serde_json::json!({ "target_user": target_user }),
    )
    .await;

    Ok(Json(MessageResponse {
        message: "membership revoked".into(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct OAuthStartQuery {
    pub redirect_uri: Option<String>,
}

pub async fn oauth_start(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<OAuthStartQuery>,
) -> Result<Redirect, AppError> {
    let provider = OAuthProvider::parse(&provider)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "unknown", "unknown oauth provider"))?;
    let client_id = provider.client_id().ok_or_else(|| {
        AppError::new(
            ErrorCode::ServiceUnavailable,
            "unknown",
            format!("{} oauth not configured", provider.as_str()),
        )
    })?;
    let redirect_uri = q.redirect_uri.unwrap_or_else(|| {
        format!(
            "{}/api/v1/auth/oauth/{}/callback",
            mail::public_api_base(),
            provider.as_str()
        )
    });
    let state_token = generate_opaque_token();
    let (verifier, challenge) = oauth::pkce_pair();
    let nonce = generate_opaque_token();
    oauth::store_state(
        &state.pool,
        provider,
        &state_token,
        &verifier,
        &redirect_uri,
        &nonce,
    )
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, "unknown", e.to_string()))?;

    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&nonce={}",
        provider.authorize_url(),
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(provider.scopes()),
        urlencoding::encode(&state_token),
        urlencoding::encode(&challenge),
        urlencoding::encode(&nonce),
    );
    Ok(Redirect::temporary(&url))
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<OAuthCallbackQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    if let Some(err) = q.error {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            format!("oauth error: {err}"),
        ));
    }
    let provider = OAuthProvider::parse(&provider).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            request_id.clone(),
            "unknown oauth provider",
        )
    })?;
    let code = q.code.ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "missing code",
        )
    })?;
    let st = q.state.ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "missing state",
        )
    })?;
    let stored = oauth::take_state(&state.pool, &st)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Unauthorized,
                request_id.clone(),
                "invalid oauth state",
            )
        })?;
    let (_prov, verifier, redirect_uri, _nonce) = stored;
    let access = oauth::exchange_code(provider, &code, &redirect_uri, &verifier)
        .await
        .map_err(|e| AppError::new(ErrorCode::Unauthorized, request_id.clone(), e))?;
    let info = oauth::fetch_userinfo(provider, &access)
        .await
        .map_err(|e| AppError::new(ErrorCode::Unauthorized, request_id.clone(), e))?;
    let email = info.email.ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "oauth account missing email",
        )
    })?;
    let email_norm = normalize_email(&email);

    // Find or create user + link oauth account.
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM oauth_account WHERE provider = $1 AND provider_subject = $2",
    )
    .bind(provider.as_str())
    .bind(&info.sub)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;

    let user_id = if let Some((uid,)) = existing {
        uid
    } else if let Some((uid,)) =
        sqlx::query_as::<_, (Uuid,)>("SELECT id FROM user_identity WHERE email_normalized = $1")
            .bind(&email_norm)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?
    {
        sqlx::query(
            r#"
            INSERT INTO oauth_account (id, user_id, provider, provider_subject, email)
            VALUES ($1,$2,$3,$4,$5)
            ON CONFLICT (provider, provider_subject) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(uid)
        .bind(provider.as_str())
        .bind(&info.sub)
        .bind(&email)
        .execute(&state.pool)
        .await
        .ok();
        // Mark verified — IdP asserted email.
        sqlx::query(
            "UPDATE user_identity SET email_verified_at = COALESCE(email_verified_at, now()) WHERE id = $1",
        )
        .bind(uid)
        .execute(&state.pool)
        .await
        .ok();
        uid
    } else {
        // Create user + personal org.
        let uid = new_uuid_v7();
        let org = OrgId::generate();
        let membership_id = new_uuid_v7();
        let mut tx =
            state.pool.begin().await.map_err(|e| {
                AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string())
            })?;
        sqlx::query("INSERT INTO organization (id, public_id, name) VALUES ($1,$2,$3)")
            .bind(org.as_uuid())
            .bind(org.to_public().as_str())
            .bind(info.name.clone().unwrap_or_else(|| format!("{email} Org")))
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO user_identity (id, public_id, email, email_normalized, display_name, email_verified_at)
            VALUES ($1,$2,$3,$4,$5, now())
            "#,
        )
        .bind(uid)
        .bind(PublicId::new(IdKind::User, uid).as_str())
        .bind(&email)
        .bind(&email_norm)
        .bind(info.name.unwrap_or_else(|| email.clone()))
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        set_session_org_id(&mut tx, org)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO membership (id, org_id, user_id, public_id, role)
            VALUES ($1,$2,$3,$4,'owner')
            "#,
        )
        .bind(membership_id)
        .bind(org.as_uuid())
        .bind(uid)
        .bind(format!("mem_{membership_id}"))
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO oauth_account (id, user_id, provider, provider_subject, email)
            VALUES ($1,$2,$3,$4,$5)
            "#,
        )
        .bind(new_uuid_v7())
        .bind(uid)
        .bind(provider.as_str())
        .bind(&info.sub)
        .bind(&email)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        let envelope = EventEnvelope::new(
            org,
            Context::Auth,
            "membership",
            "created",
            1,
            Actor::human(uid),
            serde_json::json!({ "via": provider.as_str() }),
        );
        companyos_outbox::insert_event(&mut *tx, &envelope)
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::new(ErrorCode::Internal, request_id.clone(), e.to_string()))?;
        uid
    };

    let (ip, ua) = client_meta(&headers);
    issue_for_user(
        &state,
        user_id,
        None,
        Some(&format!("{} oauth", provider.as_str())),
        ip.as_deref(),
        ua.as_deref(),
        &request_id,
    )
    .await
}

#[utoipa::path(get, path = "/api/v1/auth/sso/configs", tag = "auth",
    responses((status = 200, body = SsoListResponse)))]
pub async fn list_sso(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<SsoListResponse>, AppError> {
    sso::ensure_sso_admin(&user.roles, &user.ctx.request_id)?;
    sso::require_sso_feature(&state.pool, user.ctx.org_id.as_uuid(), &user.ctx.request_id).await?;
    let items = sso::list_configs(&state.pool, user.ctx.org_id)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                user.ctx.request_id.clone(),
                e.to_string(),
            )
        })?;
    Ok(Json(SsoListResponse { items }))
}

#[utoipa::path(post, path = "/api/v1/auth/sso/configs", tag = "auth",
    request_body = UpsertSsoRequest, responses((status = 201, body = crate::auth::sso::SsoConfigView)))]
pub async fn create_sso(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(body): Json<UpsertSsoRequest>,
) -> Result<(StatusCode, Json<crate::auth::sso::SsoConfigView>), AppError> {
    sso::ensure_sso_admin(&user.roles, &user.ctx.request_id)?;
    sso::require_sso_feature(&state.pool, user.ctx.org_id.as_uuid(), &user.ctx.request_id).await?;

    if let Some(key) = idempotency_key(&headers) {
        if let Ok(Some((_, cached))) = idempotent_get(&state.pool, "sso.upsert", &key).await {
            if let Ok(view) = serde_json::from_value::<crate::auth::sso::SsoConfigView>(cached) {
                return Ok((StatusCode::CREATED, Json(view)));
            }
        }
        let view = sso::upsert_config(&state.pool, user.ctx.org_id, body)
            .await
            .map_err(|e| {
                AppError::new(ErrorCode::ValidationFailed, user.ctx.request_id.clone(), e)
            })?;
        let _ = idempotent_put(
            &state.pool,
            "sso.upsert",
            &key,
            201,
            serde_json::to_value(&view).unwrap_or(serde_json::json!({})),
        )
        .await;
        return Ok((StatusCode::CREATED, Json(view)));
    }

    let view = sso::upsert_config(&state.pool, user.ctx.org_id, body)
        .await
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, user.ctx.request_id.clone(), e))?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Debug, Deserialize)]
pub struct SsoStartQuery {
    pub redirect_uri: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/sso/{id}/start", tag = "auth",
    params(("id" = String, Path), ("redirect_uri" = Option<String>, Query)),
    responses((status = 307, description = "Redirect to the IdP authorization endpoint")))]
pub async fn sso_start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SsoStartQuery>,
    headers: HeaderMap,
) -> Result<Redirect, AppError> {
    let request_id = req_id(&headers);
    let redirect_uri = q
        .redirect_uri
        .unwrap_or_else(|| format!("{}/api/v1/auth/sso/callback", mail::public_api_base()));
    let url = sso_login::start_oidc_login(&state.pool, &id, &redirect_uri, &request_id).await?;
    Ok(Redirect::temporary(&url))
}

#[utoipa::path(get, path = "/api/v1/auth/sso/callback", tag = "auth",
    params(("code" = Option<String>, Query), ("state" = Option<String>, Query)),
    responses((status = 200, body = TokenResponse)))]
pub async fn sso_callback(
    State(state): State<AppState>,
    Query(q): Query<OAuthCallbackQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let request_id = req_id(&headers);
    let (ip, ua) = client_meta(&headers);
    if let Some(err) = q.error {
        return Err(AppError::new(
            ErrorCode::Unauthorized,
            request_id,
            format!("sso error: {err}"),
        ));
    }
    let code = q.code.ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "missing code",
        )
    })?;
    let st = q.state.ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            request_id.clone(),
            "missing state",
        )
    })?;
    let issued = sso_login::complete_oidc_login(
        &state,
        &st,
        &code,
        &request_id,
        ip.as_deref(),
        ua.as_deref(),
    )
    .await?;
    Ok(token_response(issued))
}

pub async fn jwks(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.auth_keys.ring.jwks_json())
}

pub async fn rotate_jwks(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MessageResponse>, AppError> {
    // Restricted: owner only (via authz).
    let principal =
        Principal::with_roles(user.roles.iter().filter_map(|r| Role::parse(r)).collect());
    if !authz::is_allowed(&principal, &perms::admin_membership_manage()) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            user.ctx.request_id.clone(),
            "not allowed to rotate JWKS",
        ));
    }
    let kid = format!("kid-{}", new_uuid_v7());
    let secret = generate_opaque_token();
    state
        .auth_keys
        .ring
        .upsert(companyos_auth_token::SigningKey {
            kid: kid.clone(),
            secret: secret.clone(),
            active: true,
        });
    sqlx::query("UPDATE jwks_signing_key SET is_active = false WHERE is_active = true")
        .execute(&state.pool)
        .await
        .ok();
    sqlx::query(
        r#"
        INSERT INTO jwks_signing_key (kid, algorithm, secret_material, is_active)
        VALUES ($1,'HS256',$2,true)
        "#,
    )
    .bind(&kid)
    .bind(&secret)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            user.ctx.request_id.clone(),
            e.to_string(),
        )
    })?;
    Ok(Json(MessageResponse {
        message: format!("rotated to {kid}"),
    }))
}
