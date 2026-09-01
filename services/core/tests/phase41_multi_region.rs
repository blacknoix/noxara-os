//! Phase 4.1 — multi-region foundations: residency, immutability, failover drill.

use std::sync::OnceLock;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_core::{build_router, migrate, state::AppState};
use companyos_ids::{new_uuid_v7, PublicId};
use companyos_tenancy::{
    run_failover_drill, CellHealth, CellId, ControlPlane, OrgId, RegionCode,
    CI_FAILOVER_DRILL_BUDGET,
};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceLock<()> = OnceLock::new();
static MIGRATE_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

async fn pool() -> Option<sqlx::PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    if MIGRATED.get().is_none() {
        let _guard = MIGRATE_LOCK.lock().await;
        if MIGRATED.get().is_none() {
            migrate(&pool).await.ok()?;
            let _ = MIGRATED.set(());
        }
    }
    Some(pool)
}

async fn app_state(pool: sqlx::PgPool) -> AppState {
    let ring = KeyRing::from_secret("test-auth-secret-phase41");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

/// Register via API, mark verified, mint access token with region claim.
async fn register_and_token(
    pool: &sqlx::PgPool,
    app: &axum::Router,
    ring: &KeyRing,
    email: &str,
    org_name: &str,
    region: &str,
) -> (String, String) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "reg")
                .body(Body::from(
                    json!({
                        "email": email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Phase41",
                        "org_name": org_name,
                        "region": region
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "register {email}");
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let org_public = body["org_id"].as_str().unwrap().to_string();
    let user_public = body["user_id"].as_str().unwrap().to_string();
    let org = OrgId::from_public(&org_public.parse::<PublicId>().unwrap()).unwrap();
    let user_id = user_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(user_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(pool)
    .await
    .unwrap();

    let membership_id: Uuid = {
        let mut tx = pool.begin().await.unwrap();
        companyos_tenancy::set_session_org_id(&mut tx, org)
            .await
            .unwrap();
        let id: Uuid =
            sqlx::query_as("SELECT id FROM membership WHERE org_id = $1 AND user_id = $2")
                .bind(org.as_uuid())
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .map(|(id,): (Uuid,)| id)
                .unwrap();
        tx.commit().await.unwrap();
        id
    };

    let mut tx = pool.begin().await.unwrap();
    let issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        ring,
        user_id,
        &user_public,
        org,
        membership_id,
        &["owner".into()],
        1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (org_public, issued.access_token)
}

#[tokio::test]
async fn region_set_at_register_and_immutable() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let ring = state.auth_keys.ring.clone();
    let app = build_router(state);

    let email = format!("eu-{}@test.local", new_uuid_v7());
    let (org_id, _) = register_and_token(&pool, &app, &ring, &email, "EU Co", "eu").await;

    let row: (String,) = sqlx::query_as("SELECT region FROM organization WHERE public_id = $1")
        .bind(&org_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, "eu");

    let err = sqlx::query("UPDATE organization SET region = 'us' WHERE public_id = $1")
        .bind(&org_id)
        .execute(&pool)
        .await;
    assert!(err.is_err(), "region must be immutable");
}

#[tokio::test]
async fn control_plane_failover_drill_us_ok_eu_denied() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let ring = state.auth_keys.ring.clone();
    let app = build_router(state);

    let us_email = format!("us-{}@test.local", new_uuid_v7());
    let eu_email = format!("eu2-{}@test.local", new_uuid_v7());
    let (us_org, us_token) = register_and_token(&pool, &app, &ring, &us_email, "US Co", "us").await;
    let (eu_org, eu_token) =
        register_and_token(&pool, &app, &ring, &eu_email, "EU Co 2", "eu").await;

    let us_drill = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/control-plane/failover-drill")
                .header("authorization", format!("Bearer {us_token}"))
                .header("content-type", "application/json")
                .header("x-request-id", "drill-us")
                .body(Body::from(
                    json!({
                        "org_id": us_org,
                        "fail_cell": "us-primary"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(us_drill.status(), StatusCode::OK);
    let us_body: Value =
        serde_json::from_slice(&us_drill.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(us_body["success"], true, "{us_body}");
    assert_eq!(us_body["serving_cell"], "us-dr");
    assert_eq!(us_body["within_budget"], true);

    let eu_drill = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/control-plane/failover-drill")
                .header("authorization", format!("Bearer {eu_token}"))
                .header("content-type", "application/json")
                .header("x-request-id", "drill-eu")
                .body(Body::from(
                    json!({
                        "org_id": eu_org,
                        "fail_cell": "eu-primary"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(eu_drill.status(), StatusCode::OK);
    let eu_body: Value =
        serde_json::from_slice(&eu_drill.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(eu_body["success"], false, "{eu_body}");
}

#[tokio::test]
async fn jwt_carries_region_claim() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no DATABASE_URL");
        return;
    };
    let state = app_state(pool.clone()).await;
    let ring = state.auth_keys.ring.clone();
    let app = build_router(state);

    let email = format!("ap-{}@test.local", new_uuid_v7());
    let (_, token) = register_and_token(&pool, &app, &ring, &email, "AP Co", "ap").await;
    let claims = companyos_auth_token::verify_access_token(&ring, &token).unwrap();
    assert_eq!(claims.region, "ap");
}

#[tokio::test]
async fn in_process_cell_simulation_two_orgs() {
    let mut plane = ControlPlane::new();
    plane.register_org("org_us_sim", RegionCode::Us);
    plane.register_org("org_eu_sim", RegionCode::Eu);

    assert!(plane
        .enforce_data_plane_access("org_us_sim", CellId::UsPrimary)
        .is_ok());
    assert!(plane
        .enforce_data_plane_access("org_eu_sim", CellId::UsPrimary)
        .is_err());
    assert!(plane
        .enforce_data_plane_access("org_eu_sim", CellId::EuPrimary)
        .is_ok());

    plane.set_cell_health(CellId::UsPrimary, CellHealth::Unhealthy);
    let us = run_failover_drill(
        &mut plane,
        "org_us_sim",
        CellId::UsPrimary,
        CI_FAILOVER_DRILL_BUDGET,
    );
    assert!(us.success && us.within_budget);
    assert_eq!(us.decision.unwrap().serving_cell, CellId::UsDr);

    let eu = run_failover_drill(
        &mut plane,
        "org_eu_sim",
        CellId::EuPrimary,
        CI_FAILOVER_DRILL_BUDGET,
    );
    assert!(!eu.success);
}
