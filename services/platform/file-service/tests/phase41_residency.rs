//! Phase 4.1 file residency — object keys are region-prefixed; wrong-cell denied.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_file::state::AppState;
use companyos_tenancy::{object_key, OrgId, RegionCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup() -> Option<(sqlx::PgPool, KeyRing, OrgId, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_file::migrate(&pool).await.ok()?;

    let org = OrgId::generate();
    let user = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO organization (id, public_id, name, region) VALUES ($1,$2,$3,'eu') ON CONFLICT DO NOTHING",
    )
    .bind(org.as_uuid())
    .bind(org.to_public().as_str())
    .bind("EU Files")
    .execute(&pool)
    .await
    .ok()?;

    // Local auth path uses headers; still need membership for non-bypass in some paths.
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    std::env::set_var("COMPANYOS_CELL_REGION", "eu");

    let ring = KeyRing::from_secret("file-region-test");
    Some((pool, ring, org, format!("usr_{user}")))
}

#[tokio::test]
async fn object_key_includes_region_prefix() {
    let key = object_key(RegionCode::Eu, OrgId::generate(), Uuid::nil(), "a.pdf");
    assert!(key.starts_with("eu/org/"));
    assert!(companyos_tenancy::enforce_object_key_region(&key, RegionCode::Eu).is_ok());
    assert!(companyos_tenancy::enforce_object_key_region(&key, RegionCode::Us).is_err());
}

#[tokio::test]
async fn wrong_cell_get_denied() {
    let Some((pool, ring, org, user_public)) = setup().await else {
        eprintln!("skip: DATABASE_URL");
        return;
    };

    // Insert a file row with EU key, then flip cell to US and attempt get.
    let file_id = Uuid::now_v7();
    let public_id = format!("fil_{}", file_id.simple());
    let key = object_key(RegionCode::Eu, org, file_id, "secret.pdf");

    let mut tx = pool.begin().await.unwrap();
    companyos_tenancy::set_session_org_id(&mut tx, org)
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO file_object
            (id, org_id, public_id, bucket, object_key, content_type, size_bytes, created_by, status)
        VALUES ($1,$2,$3,'companyos-files',$4,'application/pdf',10,$5,'ready')
        "#,
    )
    .bind(file_id)
    .bind(org.as_uuid())
    .bind(&public_id)
    .bind(&key)
    .bind(Uuid::nil())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    std::env::set_var("COMPANYOS_CELL_REGION", "us");
    let state = AppState::new(pool, ring);
    assert_eq!(state.cell_region, RegionCode::Us);
    let app = companyos_file::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/files/{public_id}"))
                .header("x-companyos-dev-org-id", org.to_public().as_str())
                .header("x-companyos-dev-user-id", &user_public)
                .header("x-companyos-region", "eu")
                .header("x-request-id", "file-cross")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS);
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["code"], "residency_violation");
}
