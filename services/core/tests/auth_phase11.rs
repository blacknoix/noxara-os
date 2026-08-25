//! Phase 1.1 auth DoD integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::{hash_token, KeyRing, REFRESH_COOKIE_NAME};
use companyos_authz::{principal_requires_mfa, Principal, Role};
use companyos_core::auth::mfa;
use companyos_core::auth::password;
use companyos_core::auth::sessions::{self, RefreshOutcome};
use companyos_core::state::AppState;
use companyos_core::{build_router, migrate};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn pool() -> Option<sqlx::PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    migrate(&pool).await.ok()?;
    Some(pool)
}

async fn app_state(pool: sqlx::PgPool) -> AppState {
    let ring = KeyRing::from_secret("test-auth-secret-phase11");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

struct SeededUser {
    user_id: Uuid,
    user_public: String,
    #[allow(dead_code)]
    email: String,
    org: OrgId,
    membership_id: Uuid,
    role: String,
}

async fn seed_user(
    pool: &sqlx::PgPool,
    email: &str,
    role: &str,
    password: &str,
    verified: bool,
    mfa_on: bool,
) -> SeededUser {
    let user_id = new_uuid_v7();
    let org = OrgId::generate();
    let membership_id = new_uuid_v7();
    let user_public = PublicId::new(IdKind::User, user_id).as_str();
    let (hash, salt) = password::hash_password(password).unwrap();

    sqlx::query("INSERT INTO organization (id, public_id, name) VALUES ($1,$2,$3)")
        .bind(org.as_uuid())
        .bind(org.to_public().as_str())
        .bind(format!("Org {email}"))
        .execute(pool)
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at, mfa_enabled_at, mfa_totp_secret_encrypted
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(user_id)
    .bind(&user_public)
    .bind(email)
    .bind(email.to_ascii_lowercase())
    .bind(&hash)
    .bind(&salt)
    .bind("Test User")
    .bind(if verified {
        Some(chrono::Utc::now())
    } else {
        None
    })
    .bind(if mfa_on {
        Some(chrono::Utc::now())
    } else {
        None
    })
    .bind(if mfa_on {
        Some(mfa::generate_totp_secret())
    } else {
        None
    })
    .execute(pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(membership_id)
    .bind(org.as_uuid())
    .bind(user_id)
    .bind(format!("mem_{membership_id}"))
    .bind(role)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    SeededUser {
        user_id,
        user_public,
        email: email.to_string(),
        org,
        membership_id,
        role: role.to_string(),
    }
}

async fn json_request(
    app: axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    bearer: Option<&str>,
    cookie: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut builder = Request::builder().method(method).uri(path);
    builder = builder.header("content-type", "application/json");
    builder = builder.header("x-request-id", "test-req");
    if let Some(b) = bearer {
        builder = builder.header("authorization", format!("Bearer {b}"));
    }
    if let Some(c) = cookie {
        builder = builder.header("cookie", format!("{REFRESH_COOKIE_NAME}={c}"));
    }
    let req = builder
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    let refresh = set_cookie.and_then(|c| {
        c.split(';')
            .next()
            .and_then(|p| p.strip_prefix(&format!("{REFRESH_COOKIE_NAME}=")))
            .map(|s| s.to_string())
    });
    (status, val, refresh)
}

#[tokio::test]
async fn refresh_reuse_after_rotation_revokes_family() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let ring = state.auth_keys.ring.clone();
    let user = seed_user(
        &pool,
        &format!("reuse-{}@test.local", new_uuid_v7()),
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;

    let mut tx = pool.begin().await.unwrap();
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        user.user_id,
        &user.user_public,
        user.org,
        user.membership_id,
        std::slice::from_ref(&user.role),
        1,
        Some("test"),
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let first = issued.refresh_token.clone();
    let rotated = match sessions::rotate_refresh(&pool, &ring, &first)
        .await
        .unwrap()
    {
        RefreshOutcome::Issued(i) => i.refresh_token,
        RefreshOutcome::ReuseDetected { .. } => panic!("unexpected reuse on first rotate"),
    };

    // Replay the old (rotated) token — must revoke family.
    match sessions::rotate_refresh(&pool, &ring, &first)
        .await
        .unwrap()
    {
        RefreshOutcome::ReuseDetected { family_id } => {
            assert_eq!(family_id, issued.family_id);
        }
        RefreshOutcome::Issued(_) => panic!("reuse must revoke family"),
    }

    // New token also dead.
    let err = sessions::rotate_refresh(&pool, &ring, &rotated)
        .await
        .expect_err("family revoked");
    assert!(
        err.contains("revoked") || err.contains("invalid") || err.contains("expired"),
        "got {err}"
    );

    let revoked: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM auth_session WHERE family_id = $1 AND revoked_at IS NOT NULL",
    )
    .bind(issued.family_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(revoked.0 >= 1);
}

#[tokio::test]
async fn membership_revocation_invalidates_within_10_seconds() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());
    let user = seed_user(
        &pool,
        &format!("rev-{}@test.local", new_uuid_v7()),
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;

    let mut tx = pool.begin().await.unwrap();
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user.user_id,
        &user.user_public,
        user.org,
        user.membership_id,
        std::slice::from_ref(&user.role),
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // Hello works before revoke.
    let (status, _, _) = json_request(
        app.clone(),
        "GET",
        "/api/v1/hello",
        None,
        Some(&issued.access_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let start = std::time::Instant::now();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, user.org).await.unwrap();
    sqlx::query(
        r#"
        UPDATE membership SET revoked_at = now(), policy_version = policy_version + 1
        WHERE id = $1
        "#,
    )
    .bind(user.membership_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sessions::revoke_org_user_sessions(&mut tx, user.org.as_uuid(), user.user_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (status, body, _) = json_request(
        app,
        "GET",
        "/api/v1/hello",
        None,
        Some(&issued.access_token),
        None,
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert!(
        elapsed < Duration::from_secs(10),
        "revocation took {elapsed:?}"
    );
}

#[tokio::test]
async fn brute_force_login_locks_account_not_500() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let app = build_router(state);
    let email = format!("lock-{}@test.local", new_uuid_v7());
    let _user = seed_user(
        &pool,
        &email,
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;

    let mut saw_lock = false;
    for i in 0..12 {
        let (status, body, _) = json_request(
            app.clone(),
            "POST",
            "/api/v1/auth/login",
            Some(serde_json::json!({
                "email": email,
                "password": "wrong-password-xx"
            })),
            None,
            None,
        )
        .await;
        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "iter {i}: {body}"
        );
        assert!(
            status == StatusCode::UNAUTHORIZED
                || status == StatusCode::FORBIDDEN
                || status == StatusCode::TOO_MANY_REQUESTS,
            "iter {i}: {status} {body}"
        );
        if status == StatusCode::FORBIDDEN
            || body.get("code").and_then(|c| c.as_str()) == Some("account_locked")
        {
            saw_lock = true;
            break;
        }
    }
    assert!(saw_lock, "expected account lockout");
}

#[tokio::test]
async fn org_a_token_cannot_read_org_b_hello() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());

    let user_a = seed_user(
        &pool,
        &format!("a-{}@test.local", new_uuid_v7()),
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;
    let user_b = seed_user(
        &pool,
        &format!("b-{}@test.local", new_uuid_v7()),
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;

    // Seed hello in org B.
    let hello_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, user_b.org).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO hello_message (id, org_id, public_id, message, created_by)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(hello_id)
    .bind(user_b.org.as_uuid())
    .bind(PublicId::new(IdKind::Hello, hello_id).as_str())
    .bind("secret B")
    .bind(user_b.user_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let issued_a = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user_a.user_id,
        &user_a.user_public,
        user_a.org,
        user_a.membership_id,
        std::slice::from_ref(&user_a.role),
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body, _) = json_request(
        app,
        "GET",
        "/api/v1/hello",
        None,
        Some(&issued_a.access_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert!(
        items
            .iter()
            .all(|i| i["org_id"] == user_a.org.to_public().as_str()),
        "org A token must not see org B data: {body}"
    );
    assert!(items.iter().all(|i| i["message"] != "secret B"));
}

#[tokio::test]
async fn owner_requires_mfa_at_policy_level() {
    assert!(Role::Owner.requires_mfa());
    assert!(Role::Admin.requires_mfa());
    assert!(principal_requires_mfa(&Principal::with_roles(vec![
        Role::Owner
    ])));

    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let app = build_router(state);
    let email = format!("owner-{}@test.local", new_uuid_v7());
    let _user = seed_user(&pool, &email, "owner", "correct-horse-battery", true, false).await;

    let (status, body, _) = json_request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(serde_json::json!({
            "email": email,
            "password": "correct-horse-battery"
        })),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["mfa_required"], true);
    assert!(body["challenge_token"].as_str().unwrap().len() > 10);
}

#[tokio::test]
async fn switch_org_issues_new_access_token_with_target_org() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());
    let email = format!("switch-{}@test.local", new_uuid_v7());
    let user = seed_user(
        &pool,
        &email,
        "member",
        "correct-horse-battery",
        true,
        false,
    )
    .await;

    // Second org membership.
    let org_b = OrgId::generate();
    let mem_b = new_uuid_v7();
    sqlx::query("INSERT INTO organization (id, public_id, name) VALUES ($1,$2,$3)")
        .bind(org_b.as_uuid())
        .bind(org_b.to_public().as_str())
        .bind("Org B")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_b).await.unwrap();
    sqlx::query(
        "INSERT INTO membership (id, org_id, user_id, public_id, role) VALUES ($1,$2,$3,$4,'member')",
    )
    .bind(mem_b)
    .bind(org_b.as_uuid())
    .bind(user.user_id)
    .bind(format!("mem_{mem_b}"))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user.user_id,
        &user.user_public,
        user.org,
        user.membership_id,
        &["member".into()],
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body, _) = json_request(
        app,
        "POST",
        "/api/v1/auth/switch-org",
        Some(serde_json::json!({ "org_id": org_b.to_public().as_str() })),
        Some(&issued.access_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let new_access = body["access_token"].as_str().unwrap();
    assert_ne!(new_access, issued.access_token);
    let claims =
        companyos_auth_token::verify_access_token(&state.auth_keys.ring, new_access).unwrap();
    assert_eq!(claims.org_uuid, org_b.as_uuid());
    assert_ne!(claims.org_uuid, user.org.as_uuid());
}

#[tokio::test]
async fn sso_disabled_returns_feature_disabled() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    std::env::remove_var("COMPANYOS_SSO_ENABLED");
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());
    let user = seed_user(
        &pool,
        &format!("sso-{}@test.local", new_uuid_v7()),
        "owner",
        "correct-horse-battery",
        true,
        true,
    )
    .await;

    // Enable MFA secret so we can mint session for owner.
    let secret: (String,) =
        sqlx::query_as("SELECT mfa_totp_secret_encrypted FROM user_identity WHERE id = $1")
            .bind(user.user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let _ = secret;

    let mut tx = pool.begin().await.unwrap();
    let issued = sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        user.user_id,
        &user.user_public,
        user.org,
        user.membership_id,
        &["owner".into()],
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body, _) = json_request(
        app,
        "GET",
        "/api/v1/auth/sso/configs",
        None,
        Some(&issued.access_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "feature_disabled");
}

#[tokio::test]
async fn password_breach_fixture_rejected_on_register() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };
    let state = app_state(pool).await;
    let app = build_router(state);
    let (status, body, _) = json_request(
        app,
        "POST",
        "/api/v1/auth/register",
        Some(serde_json::json!({
            "email": format!("breach-{}@test.local", new_uuid_v7()),
            "password": "companyos-breached-fixture",
            "display_name": "Breach",
            "org_name": "Bad Org"
        })),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[allow(dead_code)]
fn _hash_sanity() {
    assert_ne!(hash_token("a"), hash_token("b"));
}
