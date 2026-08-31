//! Phase 3.2 analytics and reporting definition-of-done tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_analytics::state::AppState;
use companyos_auth_token::KeyRing;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::OnceLock;
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
const SECRET: &str = "analytics-phase32-test-secret";

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    ensure_migrated(&pool).await?;
    Some(pool)
}

async fn ensure_migrated(pool: &PgPool) -> Option<()> {
    if MIGRATED.get().is_some() {
        return Some(());
    }
    let _guard = SEED_LOCK.lock().await;
    if MIGRATED.get().is_none() {
        companyos_core::migrate(pool).await.ok()?;
        companyos_outbox::migrate(pool).await.ok()?;
        companyos_analytics::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn analytics_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_analytics::build_router(AppState::new(pool, ring))
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    org_public: String,
    owner_token: String,
    member_token: String,
}

async fn membership_role_and_policy(pool: &PgPool, org: OrgId, user_id: Uuid) -> (Uuid, i64) {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let row = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    row
}

async fn insert_member(pool: &PgPool, org: OrgId) -> Uuid {
    let user_id = new_uuid_v7();
    let user_public = PublicId::new(IdKind::User, user_id).as_str();
    let email = format!("analytics-member-{user_id}@test.local");
    let (hash, salt) =
        companyos_core::auth::password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,'Analytics Member',now())
        "#,
    )
    .bind(user_id)
    .bind(user_public)
    .bind(&email)
    .bind(email.to_ascii_lowercase())
    .bind(hash)
    .bind(salt)
    .execute(pool)
    .await
    .unwrap();

    let membership_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let role_id: Uuid =
        sqlx::query_scalar("SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'member'")
            .bind(org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (
            id, org_id, user_id, public_id, role, role_id, policy_version, status
        ) VALUES ($1,$2,$3,$4,'member',$5,1,'active')
        "#,
    )
    .bind(membership_id)
    .bind(org.as_uuid())
    .bind(user_id)
    .bind(PublicId::new(IdKind::Membership, membership_id).as_str())
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    user_id
}

async fn seed_org() -> Option<Seeded> {
    let pool = pool().await?;
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(SECRET);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let app = core_app(pool.clone(), ring.clone());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header(
                    "x-request-id",
                    format!("analytics-register-{}", new_uuid_v7()),
                )
                .body(Body::from(
                    json!({
                        "email": format!("analytics-owner-{}@test.local", new_uuid_v7()),
                        "password": "correct-horse-battery-staple",
                        "display_name": "Analytics Owner",
                        "org_name": "Analytics Phase 3.2 Test"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .ok()?;
    if response.status() != StatusCode::CREATED {
        return None;
    }
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.ok()?.to_bytes()).ok()?;
    let org_public = body["org_id"].as_str()?.to_string();
    let owner_public = body["user_id"].as_str()?.to_string();
    let org = OrgId::from_public(&org_public.parse().ok()?).ok()?;
    let owner_id = owner_public.parse::<PublicId>().ok()?.uuid();

    // Match the established service-test MFA-verified owner setup.
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), \
         mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .ok()?;
    companyos_core::workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok()?;

    let member_id = insert_member(&pool, org).await;
    let (owner_membership, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (member_membership, member_policy) =
        membership_role_and_policy(&pool, org, member_id).await;

    let mut tx = pool.begin().await.ok()?;
    let owner_tokens = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        owner_id,
        &owner_public,
        org,
        owner_membership,
        &["owner".into()],
        owner_policy,
        None,
        None,
        None,
    )
    .await
    .ok()?;
    let member_tokens = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        member_id,
        &PublicId::new(IdKind::User, member_id).as_str(),
        org,
        member_membership,
        &["member".into()],
        member_policy,
        None,
        None,
        None,
    )
    .await
    .ok()?;
    tx.commit().await.ok()?;

    Some(Seeded {
        pool,
        ring,
        org,
        org_public,
        owner_token: owner_tokens.access_token,
        member_token: member_tokens.access_token,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", format!("analytics-{}", new_uuid_v7()));
    let body = match body {
        Some(value) => {
            request = request.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

async fn ingest(app: &Router, envelope: EventEnvelope) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/analytics/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&envelope).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

fn report_definition(org_id: &str) -> Value {
    json!({
        "org_id": org_id,
        "metric": "revenue_issued",
        "dimensions": ["currency"],
        "filters": [],
        "group_by": ["currency"],
        "visualization": "bar"
    })
}

async fn create_revenue_report(app: &Router, seed: &Seeded) -> Value {
    let (status, report) = call(
        app,
        "POST",
        "/api/v1/analytics/reports",
        &seed.owner_token,
        Some(json!({
            "name": "Issued revenue",
            "description": "Revenue grouped by currency",
            "definition": report_definition(&seed.org_public)
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    report
}

async fn fact_count(pool: &PgPool, org: OrgId, table: &str) -> i64 {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let sql = format!("SELECT COUNT(*)::bigint FROM {table} WHERE org_id = $1");
    let count: (i64,) = sqlx::query_as(&sql)
        .bind(org.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    count.0
}

#[test]
fn metric_catalogue_golden_is_unique_without_database() {
    let metrics = companyos_analytics::metrics::list_metrics();
    let golden: Vec<companyos_analytics::metrics::MetricDefinition> =
        serde_json::from_str(&companyos_analytics::metrics::catalogue_golden_json()).unwrap();
    assert_eq!(metrics, golden);
    assert!(!metrics.is_empty());
    let names: HashSet<_> = metrics.iter().map(|metric| metric.name.as_str()).collect();
    assert_eq!(names.len(), metrics.len());
}

#[tokio::test]
async fn reporting_permissions_simulation_forecast_schedule_and_export() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = analytics_app(seed.pool.clone(), seed.ring.clone());

    let (metrics_status, metrics) = call(
        &app,
        "GET",
        "/api/v1/analytics/metrics",
        &seed.owner_token,
        None,
    )
    .await;
    let (golden_status, golden) = call(
        &app,
        "GET",
        "/api/v1/analytics/metrics/golden",
        &seed.owner_token,
        None,
    )
    .await;
    assert_eq!(metrics_status, StatusCode::OK);
    assert_eq!(golden_status, StatusCode::OK);
    assert_eq!(metrics["metrics"], golden);

    let (guard_status, guard_error) = call(
        &app,
        "POST",
        "/api/v1/analytics/query/run",
        &seed.owner_token,
        Some(json!({
            "metric": "revenue_issued",
            "dimensions": [],
            "filters": [],
            "group_by": [],
            "visualization": "table"
        })),
    )
    .await;
    assert_eq!(guard_status, StatusCode::BAD_REQUEST, "{guard_error}");

    let invoice = EventEnvelope::new(
        seed.org,
        Context::Finance,
        "invoice",
        "issued",
        1,
        Actor::human(new_uuid_v7()),
        json!({
            "invoice_id": "inv_phase32_a",
            "amount_minor": 2500,
            "currency": "USD"
        }),
    );
    let deal = EventEnvelope::new(
        seed.org,
        Context::Sales,
        "deal",
        "stage_changed",
        1,
        Actor::human(new_uuid_v7()),
        json!({
            "deal_id": "deal_phase32_a",
            "from_stage": "qualified",
            "to_stage": "proposal",
            "amount_minor": 9000,
            "currency": "USD"
        }),
    );
    assert_eq!(
        ingest(&app, invoice).await["fact"],
        "fact_invoice_lifecycle"
    );
    assert_eq!(ingest(&app, deal).await["fact"], "fact_deal_stage_change");

    let report = create_revenue_report(&app, &seed).await;
    let report_id = report["id"].as_str().unwrap();
    let run_uri = format!("/api/v1/analytics/reports/{report_id}/run");
    let (owner_status, owner_run) =
        call(&app, "POST", &run_uri, &seed.owner_token, Some(json!({}))).await;
    assert_eq!(owner_status, StatusCode::OK, "{owner_run}");
    assert_eq!(owner_run["report_id"], report_id);
    assert!(
        owner_run["result"]["rows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "{owner_run}"
    );
    assert_eq!(owner_run["result"]["rows"][0]["value"], 2500);

    let (member_status, member_run) =
        call(&app, "POST", &run_uri, &seed.member_token, Some(json!({}))).await;
    assert_eq!(member_status, StatusCode::OK, "{member_run}");
    assert_eq!(member_run["report_id"], report_id);
    assert_eq!(member_run["result"]["rows"], json!([]));
    assert_eq!(member_run["result"]["permission_denied_empty"], true);

    let before = fact_count(&seed.pool, seed.org, "analytics_fact_invoice_lifecycle").await;
    let (simulate_status, simulation) = call(
        &app,
        "POST",
        "/api/v1/analytics/reports/simulate",
        &seed.owner_token,
        Some(json!({"definition": report_definition(&seed.org_public)})),
    )
    .await;
    assert_eq!(simulate_status, StatusCode::OK, "{simulation}");
    assert_eq!(simulation["dry_run"], true);
    let after = fact_count(&seed.pool, seed.org, "analytics_fact_invoice_lifecycle").await;
    assert_eq!(before, after);

    let (forecast_status, forecast) = call(
        &app,
        "POST",
        "/api/v1/analytics/forecasts",
        &seed.owner_token,
        Some(json!({
            "org_id": seed.org_public,
            "series": "revenue",
            "horizon_periods": 2,
            "history_periods": 7,
            "method": "linear_trend"
        })),
    )
    .await;
    assert_eq!(forecast_status, StatusCode::OK, "{forecast}");
    assert_eq!(forecast["method"], "linear_trend");
    assert!(forecast["inputs"].is_object());
    assert_eq!(forecast["inputs"]["horizon_periods"], 2);
    assert_eq!(forecast["forecast"].as_array().unwrap().len(), 2);

    let export_uri = format!("/api/v1/analytics/reports/{report_id}/export");
    let (export_status, export) = call(
        &app,
        "POST",
        &export_uri,
        &seed.owner_token,
        Some(json!({"format": "csv"})),
    )
    .await;
    assert_eq!(export_status, StatusCode::OK, "{export}");
    assert_eq!(export["content_type"], "text/csv");
    assert!(export["content"]
        .as_str()
        .unwrap()
        .starts_with("metric,dimension,value,record_ids,drill_links\n"));
    assert!(export["content"]
        .as_str()
        .unwrap()
        .contains("revenue_issued"));

    let (schedule_status, schedule) = call(
        &app,
        "POST",
        "/api/v1/analytics/schedules",
        &seed.owner_token,
        Some(json!({
            "report_id": report_id,
            "cron": "0 9 * * *",
            "timezone": "UTC",
            "channel": "notification",
            "recipients": ["owner@test.local"],
            "export_format": "csv",
            "enabled": true
        })),
    )
    .await;
    assert_eq!(schedule_status, StatusCode::CREATED, "{schedule}");
    let schedule_id = schedule["id"].as_str().unwrap();
    let (fire_status, fired) = call(
        &app,
        "POST",
        &format!("/api/v1/analytics/schedules/{schedule_id}/fire"),
        &seed.owner_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(fire_status, StatusCode::OK, "{fired}");
    assert_eq!(fired["state"], "completed");
    assert_eq!(fired["workflow_type"], "ScheduledReportDelivery");
    assert_eq!(fired["export"]["content_type"], "text/csv");

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let run_status: String =
        sqlx::query_scalar("SELECT status FROM analytics_run WHERE org_id = $1 AND public_id = $2")
            .bind(seed.org.as_uuid())
            .bind(fired["run_id"].as_str().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(run_status, "completed");
}

#[tokio::test]
async fn reports_and_facts_are_tenant_isolated_by_rls() {
    let Some(first) = seed_org().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(second) = seed_org().await else {
        eprintln!("skip: second org could not be seeded");
        return;
    };
    let app = analytics_app(first.pool.clone(), first.ring.clone());
    let invoice = EventEnvelope::new(
        first.org,
        Context::Finance,
        "invoice",
        "issued",
        1,
        Actor::human(new_uuid_v7()),
        json!({
            "invoice_id": "inv_first_org_only",
            "amount_minor": 7700,
            "currency": "USD"
        }),
    );
    ingest(&app, invoice).await;
    let report = create_revenue_report(&app, &first).await;
    let report_id = report["id"].as_str().unwrap();
    let report_uuid = report_id.parse::<PublicId>().unwrap().uuid();

    let second_app = analytics_app(second.pool.clone(), second.ring.clone());
    let (list_status, list) = call(
        &second_app,
        "GET",
        "/api/v1/analytics/reports",
        &second.owner_token,
        None,
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert!(!list["reports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == report_id));
    let (get_status, _) = call(
        &second_app,
        "GET",
        &format!("/api/v1/analytics/reports/{report_id}"),
        &second.owner_token,
        None,
    )
    .await;
    assert_eq!(get_status, StatusCode::NOT_FOUND);

    let (query_status, query) = call(
        &second_app,
        "POST",
        "/api/v1/analytics/query/run",
        &second.owner_token,
        Some(report_definition(&second.org_public)),
    )
    .await;
    assert_eq!(query_status, StatusCode::OK, "{query}");
    assert!(query["result"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["value"] == 0 && row["record_ids"] == json!([])));

    let (cross_org_status, _) = call(
        &second_app,
        "POST",
        "/api/v1/analytics/query/run",
        &second.owner_token,
        Some(report_definition(&first.org_public)),
    )
    .await;
    assert_eq!(cross_org_status, StatusCode::FORBIDDEN);

    let mut tx = second.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, second.org).await.unwrap();
    let hidden_report: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM analytics_report WHERE id = $1")
            .bind(report_uuid)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    let hidden_fact: Option<(Uuid,)> = sqlx::query_as(
        "SELECT event_id FROM analytics_fact_invoice_lifecycle WHERE invoice_id = $1",
    )
    .bind("inv_first_org_only")
    .fetch_optional(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(hidden_report.is_none());
    assert!(hidden_fact.is_none());
}
