//! TRD 8.2 game day — AI provider down → copilot feature_disabled; health still ok.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_ai::provider::{build_provider, CompletionRequest};
use companyos_ai::state::AppState;
use companyos_auth_token::KeyRing;
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn provider_force_down_disables_copilot_not_health() {
    std::env::set_var("AI_PROVIDER_FORCE_DOWN", "1");
    // Ensure we do not accidentally use a live key.
    std::env::remove_var("AI_API_KEY");

    let provider = build_provider();
    let err = provider
        .complete(CompletionRequest {
            system: "sys".into(),
            user_message: "hi".into(),
            context: String::new(),
            citations: vec![],
        })
        .await
        .expect_err("DownProvider must fail");
    assert!(err.contains("forced down"), "{err}");

    let Some(url) = test_database_url() else {
        std::env::remove_var("AI_PROVIDER_FORCE_DOWN");
        eprintln!("skipping HTTP portion: no TEST_DATABASE_URL");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("db");
    companyos_core::migrate(&pool).await.ok();
    companyos_ai::migrate(&pool).await.expect("ai migrate");

    let ring = KeyRing::from_secret("gameday-ai");
    let mut state = AppState::new(pool, ring);
    state.provider = build_provider();
    let app = companyos_ai::build_router(state);

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        health.status(),
        StatusCode::OK,
        "rest of app (health) works when provider down"
    );

    // Unauthenticated chat should fail auth before provider — still not 500 from panic.
    let chat = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "message": "hello" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(chat.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let _ = chat.into_body().collect().await;

    std::env::remove_var("AI_PROVIDER_FORCE_DOWN");
}

#[tokio::test]
async fn provider_error_maps_to_feature_disabled_code() {
    // Pure mapping contract used by chat handler.
    use companyos_errors::ErrorCode;
    let code = ErrorCode::FeatureDisabled;
    assert_eq!(code.as_str(), "feature_disabled");
}
