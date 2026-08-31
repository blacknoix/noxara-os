//! Phase 3.3 outbound webhook integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State as AxumState;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use companyos_core::webhook_crypto::WebhookEncryptor;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_integration::crypto::WebhookDecryptor;
use companyos_integration::dispatcher::{dispatch_once, DispatchOptions, AUTO_PAUSE_FAILURES};
use companyos_integration::enqueue::enqueue_event;
use companyos_integration::sign;
use companyos_integration::ssrf;
use companyos_integration::{build_router, AppState};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone, Default)]
struct EchoState {
    hits: Arc<Mutex<Vec<EchoHit>>>,
    /// When true, respond 500 (for retry tests).
    fail_next: Arc<Mutex<bool>>,
}

#[derive(Clone, Debug)]
struct EchoHit {
    signature: Option<String>,
    body: Vec<u8>,
}

async fn echo_handler(
    AxumState(state): AxumState<EchoState>,
    req: Request<Body>,
) -> (StatusCode, Json<Value>) {
    let signature = req
        .headers()
        .get("X-CompanyOS-Signature")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = req
        .into_body()
        .collect()
        .await
        .map(|c| c.to_bytes().to_vec())
        .unwrap_or_default();
    state
        .hits
        .lock()
        .expect("hits")
        .push(EchoHit { signature, body });
    let fail = {
        let mut f = state.fail_next.lock().expect("fail");
        let v = *f;
        *f = false;
        v
    };
    if fail {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false})),
        )
    } else {
        (StatusCode::OK, Json(json!({"ok": true})))
    }
}

async fn start_echo() -> (SocketAddr, EchoState) {
    let state = EchoState::default();
    let app = Router::new()
        .route("/hook", post(echo_handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    (addr, state)
}

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_outbox::migrate(&pool).await.ok()?;
    Some(pool)
}

async fn seed_org(pool: &PgPool) -> (OrgId, Uuid) {
    let org = OrgId::generate();
    let user = new_uuid_v7();
    sqlx::query(
        r#"
        INSERT INTO organization (id, public_id, name)
        VALUES ($1, $2, $3)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(org.as_uuid())
    .bind(org.to_public().as_str())
    .bind("Webhook Test Org")
    .execute(pool)
    .await
    .expect("org");
    (org, user)
}

async fn insert_endpoint(
    pool: &PgPool,
    org: OrgId,
    created_by: Uuid,
    url: &str,
    event_types: &[&str],
    secret: &str,
) -> (Uuid, String) {
    let encryptor = WebhookEncryptor::from_env().expect("encryptor");
    let ciphertext = encryptor.encrypt(secret.as_bytes()).expect("encrypt");
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::WebhookEndpoint, id);
    let events = serde_json::to_value(event_types).expect("json");
    let mut tx = pool.begin().await.expect("tx");
    set_session_org_id(&mut tx, org).await.expect("org");
    sqlx::query("SELECT set_config('app.webhook_dispatch', '1', true)")
        .execute(&mut *tx)
        .await
        .expect("dispatch");
    sqlx::query(
        r#"
        INSERT INTO webhook_endpoint (
            id, org_id, public_id, url, description, event_types,
            secret_ciphertext, secret_prefix, status, created_by
        ) VALUES ($1,$2,$3,$4,'test',$5,$6,$7,'active',$8)
        "#,
    )
    .bind(id)
    .bind(org.as_uuid())
    .bind(public_id.as_str())
    .bind(url)
    .bind(&events)
    .bind(&ciphertext)
    .bind(secret.chars().take(8).collect::<String>())
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .expect("insert endpoint");
    tx.commit().await.expect("commit");
    (id, public_id.as_str())
}

fn sample_event(org: OrgId) -> EventEnvelope {
    EventEnvelope::new(
        org,
        Context::Sales,
        "deal",
        "won",
        1,
        Actor::human(new_uuid_v7()),
        json!({ "deal_id": "dea_test" }),
    )
}

/// Allow loopback echo servers without racing on process-wide env vars.
const ALLOW_PRIVATE: DispatchOptions = DispatchOptions {
    allow_private: true,
};

fn integration_app(pool: PgPool) -> Router {
    let decryptor = WebhookDecryptor::from_env().expect("decryptor");
    build_router(AppState::with_dispatch_opts(pool, decryptor, ALLOW_PRIVATE))
}

#[tokio::test]
async fn healthz_ok() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = integration_app(pool);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn happy_delivery_and_signature() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let (addr, echo) = start_echo().await;
    let (org, user) = seed_org(&pool).await;
    let secret = "whsec_test_happy_secret_value";
    let hook_url = format!("http://{addr}/hook");
    insert_endpoint(&pool, org, user, &hook_url, &["sales.deal.won.v1"], secret).await;

    let envelope = sample_event(org);
    let app = integration_app(pool.clone());

    let enq = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/webhooks/enqueue")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&envelope).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(enq.status(), StatusCode::OK);
    let enq_body: Value =
        serde_json::from_slice(&enq.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(enq_body["inserted"], 1);

    let disp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/internal/webhooks/dispatch-once")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disp.status(), StatusCode::OK);
    let disp_body: Value =
        serde_json::from_slice(&disp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(disp_body["delivered"], 1);

    // Allow echo to record.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let hits = echo.hits.lock().expect("hits").clone();
    assert_eq!(hits.len(), 1);
    let sig = hits[0].signature.as_deref().expect("signature header");
    sign::verify(
        secret.as_bytes(),
        &hits[0].body,
        sig,
        chrono::Utc::now().timestamp(),
    )
    .expect("signature verifies");

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM webhook_delivery WHERE org_id = $1 LIMIT 1")
            .bind(org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status, "delivered");
}

#[tokio::test]
async fn ssrf_rejects_loopback_at_dispatch() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    assert_eq!(
        ssrf::assert_url_safe("http://127.0.0.1/hook"),
        Err(ssrf::SsrfError::BlockedAddress)
    );

    let (org, user) = seed_org(&pool).await;
    let secret = "whsec_ssrf_test_secret";
    insert_endpoint(
        &pool,
        org,
        user,
        "http://127.0.0.1:9/should-not-connect",
        &["*"],
        secret,
    )
    .await;

    let envelope = sample_event(org);
    enqueue_event(&pool, &envelope).await.expect("enqueue");

    let decryptor = WebhookDecryptor::from_env().unwrap();
    let stats = dispatch_once(&pool, &decryptor, 10, DispatchOptions::strict())
        .await
        .expect("dispatch");
    assert!(stats.failed >= 1 || stats.skipped_ssrf >= 1);
    assert!(stats.skipped_ssrf >= 1);

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (status, last_error): (String, Option<String>) = sqlx::query_as(
        "SELECT status, last_error FROM webhook_delivery WHERE org_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(status == "dead" || status == "failed", "status={status}");
    let err = last_error.unwrap_or_default();
    assert!(err.contains("ssrf"), "error={err}");
}

#[tokio::test]
async fn retry_backoff_then_success() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let (addr, echo) = start_echo().await;
    *echo.fail_next.lock().unwrap() = true;

    let (org, user) = seed_org(&pool).await;
    let secret = "whsec_retry_secret_value";
    insert_endpoint(
        &pool,
        org,
        user,
        &format!("http://{addr}/hook"),
        &["sales.deal.won.v1"],
        secret,
    )
    .await;

    let envelope = sample_event(org);
    enqueue_event(&pool, &envelope).await.unwrap();
    let decryptor = WebhookDecryptor::from_env().unwrap();

    let stats1 = dispatch_once(&pool, &decryptor, 5, ALLOW_PRIVATE)
        .await
        .unwrap();
    assert_eq!(stats1.failed, 1);
    assert_eq!(stats1.skipped_ssrf, 0);

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (status, attempt, next_retry): (String, i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT status, attempt, next_retry_at FROM webhook_delivery WHERE org_id = $1 LIMIT 1",
        )
        .bind(org.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    // Clear backoff so next dispatch runs immediately.
    sqlx::query(
        "UPDATE webhook_delivery SET next_retry_at = now() - interval '1 second' WHERE org_id = $1",
    )
    .bind(org.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status, "failed");
    assert_eq!(attempt, 1);
    assert!(next_retry.is_some());

    let stats2 = dispatch_once(&pool, &decryptor, 5, ALLOW_PRIVATE)
        .await
        .unwrap();
    assert_eq!(stats2.delivered, 1);
    assert_eq!(echo.hits.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn duplicate_event_id_at_least_once_single_row() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let (org, user) = seed_org(&pool).await;
    insert_endpoint(
        &pool,
        org,
        user,
        "https://8.8.8.8/unused",
        &["*"],
        "whsec_dedupe_secret",
    )
    .await;

    let envelope = sample_event(org);
    let r1 = enqueue_event(&pool, &envelope).await.unwrap();
    let r2 = enqueue_event(&pool, &envelope).await.unwrap();
    assert_eq!(r1.inserted, 1);
    assert_eq!(r2.inserted, 0);

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM webhook_delivery WHERE org_id = $1 AND event_id = $2",
    )
    .bind(org.as_uuid())
    .bind(envelope.event_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn replay_delivers_again_at_least_once() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let (addr, echo) = start_echo().await;
    let (org, user) = seed_org(&pool).await;
    let secret = "whsec_replay_secret_value";
    insert_endpoint(
        &pool,
        org,
        user,
        &format!("http://{addr}/hook"),
        &["*"],
        secret,
    )
    .await;

    let envelope = sample_event(org);
    enqueue_event(&pool, &envelope).await.unwrap();
    let decryptor = WebhookDecryptor::from_env().unwrap();
    assert_eq!(
        dispatch_once(&pool, &decryptor, 5, ALLOW_PRIVATE)
            .await
            .unwrap()
            .delivered,
        1
    );

    // Replay: reset to pending (same event_id — receiver must tolerate duplicate).
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    sqlx::query(
        r#"
        UPDATE webhook_delivery
        SET status = 'pending', next_retry_at = now(), attempt = attempt,
            delivered_at = NULL, status_code = NULL, updated_at = now()
        WHERE org_id = $1 AND event_id = $2
        "#,
    )
    .bind(org.as_uuid())
    .bind(envelope.event_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        dispatch_once(&pool, &decryptor, 5, ALLOW_PRIVATE)
            .await
            .unwrap()
            .delivered,
        1
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        echo.hits.lock().unwrap().len(),
        2,
        "at-least-once: receiver sees duplicate"
    );
}

#[tokio::test]
async fn auto_pause_after_consecutive_failures() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    // No echo — connection refused counts as failure. Allow private so SSRF doesn't short-circuit.
    let (org, user) = seed_org(&pool).await;
    let (_epid, _pub) = insert_endpoint(
        &pool,
        org,
        user,
        "http://127.0.0.1:1/nowhere",
        &["*"],
        "whsec_pause_secret",
    )
    .await;

    let decryptor = WebhookDecryptor::from_env().unwrap();
    for i in 0..AUTO_PAUSE_FAILURES {
        let mut env = sample_event(org);
        // unique event ids so each enqueue creates a new delivery
        env.event_id = new_uuid_v7();
        env.idempotency_key = env.event_id.to_string();
        enqueue_event(&pool, &env).await.unwrap();
        let _ = dispatch_once(&pool, &decryptor, 5, ALLOW_PRIVATE)
            .await
            .unwrap();
        // clear backoff on remaining pending so next can run... actually each is new.
        let _ = i;
    }

    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (status, failure_count): (String, i32) =
        sqlx::query_as("SELECT status, failure_count FROM webhook_endpoint WHERE org_id = $1")
            .bind(org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status, "paused");
    assert!(failure_count >= AUTO_PAUSE_FAILURES);
}
