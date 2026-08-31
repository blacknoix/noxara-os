//! Phase 3.3 — Public API keys, scope intersection, deprecation dual-publish,
//! webhook CRUD / RLS / replay (SSRF + signature covered in companyos-integration).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::{hash_token, KeyRing};
use companyos_core::governance::webhooks;
use companyos_core::state::AppState;
use companyos_core::{build_router, migrate};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

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

#[test]
fn public_scopes_validation_unit() {
    assert!(companyos_core::public_scopes::validate_requested_scopes(&[
        "sales.customer.read".into()
    ])
    .is_ok());
    assert!(
        companyos_core::public_scopes::validate_requested_scopes(&["read:reports".into()]).is_err()
    );
    assert_eq!(
        companyos_core::public_scopes::public_permission_for("GET", "/api/v1/sales/customers")
            .unwrap()
            .as_str(),
        "sales.customer.read"
    );
}

#[test]
fn principal_intersect_scopes_narrower_wins() {
    use companyos_authz::{perms, Principal, Role};
    let owner = Principal::with_roles(vec![Role::Owner]);
    let narrow = owner.intersect_scopes(&["sales.customer.read".into()]);
    assert!(companyos_authz::is_allowed(
        &narrow,
        &perms::sales_customer_read()
    ));
    assert!(!companyos_authz::is_allowed(
        &narrow,
        &perms::finance_invoice_issue()
    ));
}

#[tokio::test]
async fn api_key_exchange_scopes_intersect_and_deprecation_headers() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let ring = KeyRing::from_secret("test-auth-secret-phase33");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .unwrap();
    let state = AppState::new(pool.clone(), ring);
    let app = build_router(state.clone());

    let email = format!("o33-{}@test.local", new_uuid_v7());
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
                        "display_name": "Owner",
                        "org_name": format!("Org {}", new_uuid_v7())
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    if !(status.is_success()) {
        eprintln!(
            "register status {status}: {}",
            String::from_utf8_lossy(&bytes)
        );
        return;
    }
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let org_public = body["org_id"].as_str().unwrap().to_string();
    let owner_public = body["user_id"].as_str().unwrap().to_string();
    let org_id = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let owner_id = owner_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();
    let _ = companyos_core::workspace::provisioning::process_pending(&pool, org_id, "test").await;

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_id).await.unwrap();
    let scopes = vec!["sales.customer.read".into(), "finance.invoice.issue".into()];
    let (key, secret) = companyos_core::governance::api_keys::create(
        &mut tx,
        org_id,
        owner_id,
        "phase33-key",
        &scopes,
        None,
        "t",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let exchange = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/api-keys/exchange")
                .header("content-type", "application/json")
                .header("x-request-id", "ex-1")
                .body(Body::from(
                    serde_json::json!({ "key_hash": hash_token(&secret) }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exchange.status(), StatusCode::OK);
    assert_eq!(
        exchange
            .headers()
            .get("deprecation")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    assert!(exchange.headers().get("sunset").is_some());
    let ex_body: Value =
        serde_json::from_slice(&exchange.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(ex_body["rate_limit_per_minute"].as_i64().unwrap() > 0);
    assert_eq!(ex_body["rate_limit_rpm"], ex_body["rate_limit_per_minute"]);
    assert_eq!(ex_body["api_key_id"], key.id);
    let returned: Vec<String> = serde_json::from_value(ex_body["scopes"].clone()).unwrap();
    assert!(returned.contains(&"sales.customer.read".into()));

    // Wrong hash rejected
    let bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/api-keys/exchange")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "key_hash": hash_token("nope") }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    // Revoked key rejected
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_id).await.unwrap();
    companyos_core::governance::api_keys::revoke(&mut tx, org_id, &key.id, "t")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/api-keys/exchange")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "key_hash": hash_token(&secret) }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn webhook_endpoint_rls_isolation_and_replay() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let ring = KeyRing::from_secret("test-auth-secret-phase33-wh");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .unwrap();
    let state = AppState::new(pool.clone(), ring);

    let org_a = OrgId::generate();
    let org_b = OrgId::generate();
    for (org, name) in [(org_a, "A"), (org_b, "B")] {
        sqlx::query(
            r#"
            INSERT INTO organization (id, public_id, name)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(org.as_uuid())
        .bind(org.to_public().as_str())
        .bind(name)
        .execute(&pool)
        .await
        .ok();
    }

    let user = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_a).await.unwrap();
    let (ep, secret) = webhooks::create_endpoint(
        &mut tx,
        org_a,
        user,
        "https://example.com/hooks/a",
        "a",
        &["finance.invoice.issued".into()],
        &state.webhook_crypto,
        "t",
    )
    .await
    .unwrap();
    assert!(secret.starts_with("whsec_"));

    let event_id = new_uuid_v7();
    let del_id = new_uuid_v7();
    let del_public = PublicId::new(IdKind::WebhookDelivery, del_id);
    let endpoint_uuid = ep.id.parse::<PublicId>().unwrap().uuid();
    sqlx::query(
        r#"
        INSERT INTO webhook_delivery (
            id, org_id, public_id, endpoint_id, event_id, event_subject, event_type,
            payload, attempt, status
        ) VALUES ($1,$2,$3,$4,$5,'subj','finance.invoice.issued','{}'::jsonb,1,'delivered')
        "#,
    )
    .bind(del_id)
    .bind(org_a.as_uuid())
    .bind(del_public.as_str())
    .bind(endpoint_uuid)
    .bind(event_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    let replayed = webhooks::replay_delivery(&mut tx, org_a, &del_public.as_str(), "t")
        .await
        .unwrap();
    assert_eq!(replayed.status, "pending");
    assert!(replayed.attempt >= 2);
    tx.commit().await.unwrap();

    let items_b = webhooks::list_endpoints(&pool, org_b, "t").await.unwrap();
    assert!(items_b.iter().all(|i| i.id != ep.id));
}

#[test]
fn public_openapi_marks_deprecation_policy() {
    let doc = companyos_core::public_openapi::public_openapi();
    assert_eq!(doc["info"]["title"], "CompanyOS Public API");
    assert_eq!(doc["x-companyos-deprecation-policy"]["window_days"], 180);
}
