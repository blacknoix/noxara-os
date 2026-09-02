//! Modest CI load harness for top endpoints (budgets, not production SLOs).
//!
//! Budgets: read p95 ≤ 200ms, write p95 ≤ 400ms against local in-process router.
//! Gated behind `OPS_LOAD_HARNESS=1` so default `cargo test --workspace` stays stable.

use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_core::{build_router, migrate};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn percentile(sorted_ms: &[u128], p: f64) -> u128 {
    if sorted_ms.is_empty() {
        return 0;
    }
    let idx = ((p / 100.0) * (sorted_ms.len() as f64 - 1.0)).round() as usize;
    sorted_ms[idx.min(sorted_ms.len() - 1)]
}

#[tokio::test]
async fn top_endpoints_respect_local_p95_budgets() {
    if std::env::var("OPS_LOAD_HARNESS").ok().as_deref() != Some("1") {
        eprintln!("skipping load harness (set OPS_LOAD_HARNESS=1 to run)");
        return;
    }
    let Some(url) = test_database_url() else {
        eprintln!("skipping load harness: no TEST_DATABASE_URL");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .expect("db");
    migrate(&pool).await.expect("migrate");
    let ring = KeyRing::from_secret("ops-load-harness");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    let app = build_router(companyos_core::state::AppState::new(
        pool.clone(),
        ring.clone(),
    ));

    let email = format!("load-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "email": email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Load",
                        "org_name": "Load Org"
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
    let org = OrgId::from_public(&body["org_id"].as_str().unwrap().parse().unwrap()).unwrap();
    let user_id = body["user_id"]
        .as_str()
        .unwrap()
        .parse::<PublicId>()
        .unwrap()
        .uuid();
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(user_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();
    companyos_core::workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (mem_id, pol): (uuid::Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let tok = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        user_id,
        &PublicId::new(IdKind::User, user_id).as_str(),
        org,
        mem_id,
        &["owner".into()],
        pol,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let token = tok.access_token;

    let iterations = 30u32;
    let mut read_ms = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        let t0 = Instant::now();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        read_ms.push(t0.elapsed().as_millis());
    }
    read_ms.sort_unstable();
    let read_p95 = percentile(&read_ms, 95.0);

    let mut write_ms = Vec::with_capacity(iterations as usize);
    for i in 0..iterations {
        let t0 = Instant::now();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/hello")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .header("x-request-id", format!("load-write-{i}"))
                    .body(Body::from(
                        json!({ "message": format!("load harness {i}") }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        assert!(
            status.is_success(),
            "hello write failed: {status} {:?}",
            res.into_body().collect().await.ok()
        );
        write_ms.push(t0.elapsed().as_millis());
    }
    write_ms.sort_unstable();
    let write_p95 = percentile(&write_ms, 95.0);

    println!("LOAD_HARNESS read_p95_ms={read_p95} write_p95_ms={write_p95} n={iterations}");
    println!("LOAD_HARNESS budgets: read<=200 write<=400 (local CI — not production SLOs)");

    assert!(
        read_p95 <= 200,
        "read p95 {read_p95}ms exceeded local budget 200ms"
    );
    assert!(
        write_p95 <= 400,
        "write p95 {write_p95}ms exceeded local budget 400ms"
    );
}
