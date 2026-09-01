//! TRD 8.2 game day — OpenSearch down → Postgres mirror fallback + banner.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::new_uuid_v7;
use companyos_search::state::AppState as SearchAppState;
use companyos_tenancy::Actor;
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_search::migrate(&pool).await.ok()?;
    Some(pool)
}

#[tokio::test]
async fn opensearch_down_falls_back_to_postgres_mirror_with_banner() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no TEST_DATABASE_URL");
        return;
    };

    let ring = KeyRing::from_secret("gameday-search-secret");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .unwrap();
    let core = companyos_core::build_router(companyos_core::state::AppState::new(
        pool.clone(),
        ring.clone(),
    ));

    let email = format!("gameday-search-{}@test.local", new_uuid_v7());
    let res = core
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "email": email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "GameDay",
                        "org_name": "Search Game Day"
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
    let org_public = body["org_id"].as_str().unwrap().to_string();
    let org = companyos_tenancy::OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let user_id = body["user_id"]
        .as_str()
        .unwrap()
        .parse::<companyos_ids::PublicId>()
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
    companyos_tenancy::set_session_org_id(&mut tx, org)
        .await
        .unwrap();
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
        &companyos_ids::PublicId::new(companyos_ids::IdKind::User, user_id).as_str(),
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

    std::env::set_var("OPENSEARCH_URL", "http://127.0.0.1:9");
    let state = SearchAppState::new(pool.clone(), ring.clone());
    assert!(state.opensearch_url.is_some());

    let envelope = EventEnvelope::new(
        org,
        Context::Sales,
        "deal",
        "created",
        1,
        Actor::human(user_id),
        json!({
            "id": "deal_gameday_1",
            "name": "Acme renewal",
            "summary": "game day mirror"
        }),
    );
    let app = companyos_search::build_router(state.clone());
    let ingest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/search/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&envelope).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        ingest.status(),
        StatusCode::OK,
        "ingest must succeed with OS down"
    );

    let uri = format!("/api/v1/search/query?org_id={org_public}&region=us&q=acme");
    let res = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {}", tok.access_token))
                .header("x-request-id", "gameday-search-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["degraded"], true, "{body}");
    assert_eq!(body["banner"], "search_opensearch_fallback");
    assert!(
        body["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["title"].as_str().unwrap_or("").contains("Acme")),
        "expected mirror hit: {body}"
    );

    std::env::remove_var("OPENSEARCH_URL");
}
