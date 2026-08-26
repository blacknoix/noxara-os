//! Phase 1.8 analytics tests — facts only from events; reconcile counts match.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_analytics::state::AppState;
use companyos_auth_token::KeyRing;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{Actor, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_analytics::migrate(&pool).await.ok()?;
    Some(pool)
}

fn app(pool: PgPool) -> Router {
    let ring = KeyRing::from_secret("analytics-test");
    companyos_analytics::build_router(AppState::new(pool, ring))
}

#[tokio::test]
async fn facts_only_from_events_and_reconcile() {
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let router = app(pool);
    let org = OrgId::generate();
    let org_pub = org.to_public().as_str();
    let user = format!("usr_{}", new_uuid_v7().simple());
    // Use a proper usr_ public id for local auth.
    let user_pub =
        companyos_ids::PublicId::new(companyos_ids::IdKind::User, new_uuid_v7()).as_str();

    // Non-invoice event must not create a fact.
    let other = EventEnvelope::new(
        org,
        Context::Sales,
        "customer",
        "created",
        1,
        Actor::human(new_uuid_v7()),
        json!({"customer_id": "cus_x"}),
    );
    let r = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/analytics/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&other).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let b: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b["accepted"], false);

    // Two invoice.issued events → two facts.
    for i in 0..2 {
        let env = EventEnvelope::new(
            org,
            Context::Finance,
            "invoice",
            "issued",
            1,
            Actor::human(new_uuid_v7()),
            json!({
                "invoice_id": format!("inv_{i}"),
                "amount_minor": 1000 + i,
                "currency": "USD"
            }),
        );
        let res = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/internal/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&env).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    let facts = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/analytics/facts/invoice-issued?org_id={org_pub}"
                ))
                .header("x-companyos-dev-org-id", &org_pub)
                .header("x-companyos-dev-user-id", &user_pub)
                .header("x-request-id", "facts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(facts.status(), StatusCode::OK);
    let fb: Value =
        serde_json::from_slice(&facts.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(fb["facts"].as_array().unwrap().len(), 2);

    let recon = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/analytics/reconcile/nightly")
                .header("content-type", "application/json")
                .header("x-companyos-dev-org-id", &org_pub)
                .header("x-companyos-dev-user-id", &user_pub)
                .header("x-request-id", "recon")
                .body(Body::from(
                    json!({"expected_count": 2, "org_id": org.as_uuid()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recon.status(), StatusCode::OK);
    let rb: Value =
        serde_json::from_slice(&recon.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(rb["matched"], true);
    assert_eq!(rb["mirror_count"], 2);
    let _ = user;
}
