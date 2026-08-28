//! Phase 2.6 — dual mocked OIDC IdP login tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use companyos_auth_token::KeyRing;
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

#[derive(Clone)]
struct MockIdp {
    idp_key: String,
    email: String,
    sub: String,
}

async fn mock_token(
    Path(idp): Path<String>,
    Form(_form): Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "access_token": format!("mock-token-{idp}"),
        "token_type": "bearer",
        "expires_in": 3600
    }))
}

async fn mock_userinfo(
    Path(idp): Path<String>,
    axum::extract::State(idps): axum::extract::State<Arc<Vec<MockIdp>>>,
) -> impl IntoResponse {
    let found = idps.iter().find(|i| i.idp_key == idp);
    match found {
        Some(i) => Json(serde_json::json!({
            "sub": i.sub,
            "email": i.email,
            "email_verified": true,
            "name": "SSO User"
        }))
        .into_response(),
        None => (StatusCode::NOT_FOUND, "unknown idp").into_response(),
    }
}

async fn start_mock_idps(idps: Vec<MockIdp>) -> String {
    let state = Arc::new(idps);
    let app = Router::new()
        .route("/{idp}/token", post(mock_token))
        .route("/{idp}/userinfo", get(mock_userinfo))
        .route(
            "/{idp}/authorize",
            get(
                |Path(idp): Path<String>, Query(q): Query<Value>| async move {
                    // Not used by exchange tests; return a stub.
                    format!(
                        "ok:{idp}:{}",
                        q.get("state").and_then(|v| v.as_str()).unwrap_or("")
                    )
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    format!("http://{addr}")
}

struct Seeded {
    pool: sqlx::PgPool,
    org: OrgId,
    user_id: Uuid,
    email: String,
    #[allow(dead_code)]
    token: String,
}

async fn seed_org() -> Option<Seeded> {
    let pool = pool().await?;
    let ring = KeyRing::from_secret("test-sso-phase26");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let state = AppState::new(pool.clone(), ring);
    let app = build_router(state);

    let email = format!("sso-user-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "SSO User",
                        "org_name": "SSO Enterprise Co"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let user_public = body["user_id"].as_str().unwrap().to_string();
    let org_public = body["org_id"].as_str().unwrap().to_string();
    let org = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let user_id = user_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(user_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();

    companyos_core::workspace::provisioning::process_pending(&pool, org, "sso-test")
        .await
        .ok();

    // Enable SSO feature + enterprise plan
    sqlx::query(
        "INSERT INTO org_feature_flag (org_id, flag, enabled) VALUES ($1, 'sso', true)
         ON CONFLICT (org_id, flag) DO UPDATE SET enabled = true",
    )
    .bind(org.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE organization SET plan = 'enterprise' WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let membership_id: Uuid =
        sqlx::query_scalar("SELECT id FROM membership WHERE org_id = $1 AND user_id = $2")
            .bind(org.as_uuid())
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &KeyRing::from_secret("test-sso-phase26"),
        user_id,
        &user_public,
        org,
        membership_id,
        &["owner".to_string()],
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        org,
        user_id,
        email,
        token: issued.access_token,
    })
}

#[tokio::test]
async fn two_mocked_idps_can_complete_oidc_token_exchange_and_login() {
    std::env::set_var("COMPANYOS_SSO_ENABLED", "1");
    let Some(seeded) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };

    let mock_base = start_mock_idps(vec![
        MockIdp {
            idp_key: "okta-mock".into(),
            email: seeded.email.clone(),
            sub: "okta-sub-1".into(),
        },
        MockIdp {
            idp_key: "azure-mock".into(),
            email: seeded.email.clone(),
            sub: "azure-sub-1".into(),
        },
    ])
    .await;
    std::env::set_var("SSO_MOCK_BASE", &mock_base);

    let ring = KeyRing::from_secret("test-sso-phase26");
    companyos_core::auth::ensure_bootstrap_key(&seeded.pool, &ring)
        .await
        .unwrap();
    let state = AppState::new(seeded.pool.clone(), ring);
    let app = build_router(state.clone());

    for (idp_key, display) in [("okta-mock", "Okta Mock"), ("azure-mock", "Azure Mock")] {
        // Insert config directly (admin API also works when flagged)
        let config_id = new_uuid_v7();
        let public = PublicId::new(IdKind::SsoConfig, config_id).as_str();
        let mut tx = seeded.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, seeded.org).await.unwrap();
        sqlx::query(
            r#"
            INSERT INTO sso_configuration (id, org_id, public_id, protocol, display_name, config, enabled)
            VALUES ($1,$2,$3,'oidc',$4,$5,true)
            "#,
        )
        .bind(config_id)
        .bind(seeded.org.as_uuid())
        .bind(&public)
        .bind(display)
        .bind(serde_json::json!({
            "idp_key": idp_key,
            "client_id": "test-client",
            "client_secret": "test-secret"
        }))
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let redirect = "http://localhost/api/v1/auth/sso/callback";
        let start = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/auth/sso/{public}/start?redirect_uri={}",
                        urlencoding::encode(redirect)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = start
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap()
            .to_string();
        assert!(location.contains(idp_key));
        let state_param = location
            .split('&')
            .find_map(|p| p.strip_prefix("state="))
            .map(|s| urlencoding::decode(s).unwrap().into_owned())
            .expect("state");

        let cb = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/auth/sso/callback?code=mock-code-{idp_key}&state={}",
                        urlencoding::encode(&state_param)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            cb.status(),
            StatusCode::OK,
            "idp {idp_key} login failed: {:?}",
            cb.status()
        );
        let body: Value =
            serde_json::from_slice(&cb.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(body["access_token"].as_str().unwrap().len() > 10);

        let mut tx = seeded.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, seeded.org).await.unwrap();
        let linked: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sso_identity_link WHERE org_id = $1 AND user_id = $2 AND sso_config_id = $3",
        )
        .bind(seeded.org.as_uuid())
        .bind(seeded.user_id)
        .bind(config_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(linked.0, 1);
    }

    // Pure exchange helper also works for both IdPs.
    for idp_key in ["okta-mock", "azure-mock"] {
        let endpoints =
            companyos_core::auth::sso_login::endpoints_from_config(&serde_json::json!({
                "idp_key": idp_key,
                "client_id": "c",
                "client_secret": "s"
            }))
            .unwrap();
        let info = companyos_core::auth::sso_login::exchange_code_for_userinfo(
            &endpoints,
            "any-code",
            "any-verifier",
            "http://localhost/cb",
        )
        .await
        .unwrap();
        assert_eq!(info.email.as_deref(), Some(seeded.email.as_str()));
    }
}
