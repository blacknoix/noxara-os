//! Phase 1.8 file service tests — tenant isolation; reject bad content type.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_file::state::AppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::OrgId;
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
    companyos_file::migrate(&pool).await.ok()?;
    Some(pool)
}

fn app(pool: PgPool) -> Router {
    companyos_file::build_router(AppState::new(pool, KeyRing::from_secret("file-test")))
}

fn local_headers(org: &OrgId, user: &str) -> (String, String) {
    (org.to_public().as_str(), user.to_string())
}

#[tokio::test]
async fn reject_bad_content_type_and_tenant_isolation() {
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let router = app(pool);
    let org_a = OrgId::generate();
    let org_b = OrgId::generate();
    let user_a = PublicId::new(IdKind::User, new_uuid_v7()).as_str();
    let user_b = PublicId::new(IdKind::User, new_uuid_v7()).as_str();
    let (org_a_pub, _) = local_headers(&org_a, &user_a);

    // Bad content type rejected.
    let bad = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/files/presign-upload")
                .header("content-type", "application/json")
                .header("x-companyos-dev-org-id", &org_a_pub)
                .header("x-companyos-dev-user-id", &user_a)
                .header("x-request-id", "bad-ct")
                .body(Body::from(
                    json!({
                        "filename": "x.exe",
                        "content_type": "application/x-msdownload",
                        "size_bytes": 100
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    // Good upload for org A.
    let ok = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/files/presign-upload")
                .header("content-type", "application/json")
                .header("x-companyos-dev-org-id", &org_a_pub)
                .header("x-companyos-dev-user-id", &user_a)
                .header("x-request-id", "ok")
                .body(Body::from(
                    json!({
                        "filename": "doc.pdf",
                        "content_type": "application/pdf",
                        "size_bytes": 2048
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&ok.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let file_id = body["file_id"].as_str().unwrap().to_string();

    // Org B cannot read org A's file (RLS / session).
    let cross = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/files/{file_id}"))
                .header(
                    "x-companyos-dev-org-id",
                    org_b.to_public().as_str(),
                )
                .header("x-companyos-dev-user-id", &user_b)
                .header("x-request-id", "cross")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross.status(), StatusCode::NOT_FOUND);
}
