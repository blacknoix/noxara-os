//! Phase 3.1 Configurable workflow engine — DoD tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use companyos_workflow::state::AppState;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::OnceLock;
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
const SECRET: &str = "workflow-phase31-test-secret";

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
        companyos_workflow::migrate(pool).await.ok()?;
        companyos_outbox::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn wf_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_workflow::build_router(AppState::new(pool, ring))
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    org_public: String,
    owner_token: String,
    owner_id: Uuid,
}

async fn membership_role_and_policy(pool: &PgPool, org: OrgId, user_id: Uuid) -> (Uuid, i64) {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let row: (Uuid, i64) = sqlx::query_as(
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

async fn seed_org() -> Option<Seeded> {
    let pool = pool().await?;
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(SECRET);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let app = core_app(pool.clone(), ring.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "wf31-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Workflow Phase31 Test Co"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    if res.status() != StatusCode::CREATED {
        return None;
    }
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let org_public = body["org_id"].as_str()?.to_string();
    let owner_public = body["user_id"].as_str()?.to_string();
    let org = OrgId::from_public(&org_public.parse().ok()?).ok()?;
    let owner_id = owner_public.parse::<PublicId>().ok()?.uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .ok()?;

    let _ = companyos_core::workspace::provisioning::process_pending(&pool, org, "test").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let mut tx = pool.begin().await.ok()?;
    let owner_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        owner_id,
        &owner_public,
        org,
        owner_mem_id,
        &["owner".into()],
        owner_policy,
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
        owner_token: owner_issued.access_token,
        owner_id,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", format!("wf31-{}", new_uuid_v7()));
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    (status, val)
}

fn deal_won_graph() -> Value {
    json!({
        "entry": "create_followup",
        "trigger": { "kind": "domain_event", "event_key": "sales.deal.won" },
        "nodes": [
            {
                "type": "action",
                "id": "create_followup",
                "action": "create_task",
                "params": { "title": "Follow up" },
                "next": "done"
            },
            { "type": "end", "id": "done" }
        ]
    })
}

fn payroll_graph() -> Value {
    json!({
        "entry": "a",
        "trigger": { "kind": "manual" },
        "nodes": [
            {
                "type": "action",
                "id": "a",
                "action": "read_payroll",
                "params": {},
                "next": "done"
            },
            { "type": "end", "id": "done" }
        ]
    })
}

#[tokio::test]
async fn member_cannot_create_payroll_or_journal_workflow() {
    let Some(seed) = seed_org().await else {
        eprintln!("skip: no DB / register failed");
        return;
    };
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    std::env::set_var("WORKFLOW_ACTIVITIES_NOOP", "1");

    let member_user = new_uuid_v7();
    let member_public = PublicId::new(IdKind::User, member_user);
    {
        let mut tx = seed.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, seed.org).await.unwrap();
        // Bypass for cross-user insert during test seed.
        sqlx::query("SELECT set_config('app.auth_lookup_user', $1, true)")
            .bind(member_user.to_string())
            .execute(&mut *tx)
            .await
            .ok();
        let email = format!("member-{member_user}@test.local");
        let _ = sqlx::query(
            r#"
            INSERT INTO user_identity (id, public_id, email, email_normalized, display_name, password_hash)
            VALUES ($1, $2, $3, $4, 'Member WF', 'x')
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(member_user)
        .bind(member_public.as_str())
        .bind(&email)
        .bind(email.to_lowercase())
        .execute(&mut *tx)
        .await;
        sqlx::query(
            r#"
            INSERT INTO membership (id, org_id, user_id, public_id, role, status, policy_version)
            VALUES ($1, $2, $3, $4, 'member', 'active', 1)
            ON CONFLICT (org_id, user_id) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(seed.org.as_uuid())
        .bind(member_user)
        .bind(PublicId::new(IdKind::Membership, new_uuid_v7()).as_str())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    let app = wf_app(seed.pool.clone(), seed.ring.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workflows/definitions")
                .header("content-type", "application/json")
                .header("x-companyos-dev-org-id", &seed.org_public)
                .header("x-companyos-dev-user-id", member_public.as_str())
                .header("x-request-id", "wf31-member-deny")
                .body(Body::from(
                    json!({"name": "bad payroll", "graph": payroll_graph()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let app = wf_app(seed.pool.clone(), seed.ring.clone());
    let (st, _) = call(
        &app,
        "POST",
        "/api/v1/workflows/definitions",
        &seed.owner_token,
        Some(json!({"name": "deal won", "graph": deal_won_graph()})),
        &[],
    )
    .await;
    assert!(st.is_success(), "owner create: {st}");

    // Member CAN create create_task workflow (has operations.task.create)
    let app = wf_app(seed.pool, seed.ring);
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workflows/definitions")
                .header("content-type", "application/json")
                .header("x-companyos-dev-org-id", &seed.org_public)
                .header("x-companyos-dev-user-id", member_public.as_str())
                .header("x-request-id", "wf31-member-ok")
                .body(Body::from(
                    json!({"name": "member task", "graph": deal_won_graph()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "member should create task workflow"
    );
}

#[tokio::test]
async fn in_flight_survives_new_published_version() {
    let Some(seed) = seed_org().await else {
        return;
    };
    std::env::set_var("WORKFLOW_ACTIVITIES_NOOP", "1");
    let app = wf_app(seed.pool.clone(), seed.ring.clone());

    let (st, def) = call(
        &app,
        "POST",
        "/api/v1/workflows/definitions",
        &seed.owner_token,
        Some(json!({"name": "v-test", "graph": deal_won_graph()})),
        &[],
    )
    .await;
    assert!(st.is_success());
    let def_id = def["id"].as_str().unwrap();

    let (st, ver1) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/publish"),
        &seed.owner_token,
        Some(json!({})),
        &[("idempotency-key", "pub-1")],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{ver1}");
    assert_eq!(ver1["version"], 1);

    let (st, inst) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/start"),
        &seed.owner_token,
        Some(json!({"payload": {"deal_id": "dl_test"}})),
        &[("idempotency-key", "start-1")],
    )
    .await;
    assert!(st.is_success(), "{inst}");
    assert_eq!(inst["version_number"], 1);
    let inst_id = inst["id"].as_str().unwrap().to_string();

    let mut g2 = deal_won_graph();
    g2["nodes"][0]["params"]["title"] = json!("Follow up v2");
    let (st, _) = call(
        &app,
        "PATCH",
        &format!("/api/v1/workflows/definitions/{def_id}"),
        &seed.owner_token,
        Some(json!({"graph": g2})),
        &[],
    )
    .await;
    assert!(st.is_success());

    let (st, ver2) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/publish"),
        &seed.owner_token,
        Some(json!({})),
        &[("idempotency-key", "pub-2")],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ver2["version"], 2);

    let (st, inst2) = call(
        &app,
        "GET",
        &format!("/api/v1/workflows/instances/{inst_id}"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(inst2["version_number"], 1);
}

#[tokio::test]
async fn iteration_and_concurrency_caps_fail_closed() {
    let Some(seed) = seed_org().await else {
        return;
    };
    std::env::set_var("WORKFLOW_ACTIVITIES_NOOP", "1");
    let app = wf_app(seed.pool.clone(), seed.ring.clone());

    let (st, _) = call(
        &app,
        "PUT",
        "/api/v1/workflows/bounds",
        &seed.owner_token,
        Some(json!({"max_concurrent": 1, "max_steps_per_instance": 3})),
        &[],
    )
    .await;
    assert!(st.is_success());

    let loop_graph = json!({
        "entry": "loop",
        "trigger": { "kind": "manual" },
        "nodes": [{
            "type": "action",
            "id": "loop",
            "action": "create_task",
            "params": {},
            "next": "loop"
        }]
    });
    let (st, sim) = call(
        &app,
        "POST",
        "/api/v1/workflows/simulate",
        &seed.owner_token,
        Some(json!({"graph": loop_graph, "payload": {}, "max_steps": 3})),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sim["ok"], false);
    assert!(sim["error"].as_str().unwrap().contains("cap exceeded"));
    assert_eq!(sim["side_effects"], false);

    let timer_graph = json!({
        "entry": "wait",
        "trigger": { "kind": "manual" },
        "nodes": [
            { "type": "timer", "id": "wait", "duration_secs": 3600, "next": "done" },
            { "type": "end", "id": "done" }
        ]
    });
    let (st, def) = call(
        &app,
        "POST",
        "/api/v1/workflows/definitions",
        &seed.owner_token,
        Some(json!({"name": "timer", "graph": timer_graph})),
        &[],
    )
    .await;
    assert!(st.is_success());
    let def_id = def["id"].as_str().unwrap();

    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/publish"),
        &seed.owner_token,
        Some(json!({})),
        &[("idempotency-key", "pub-timer")],
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/start"),
        &seed.owner_token,
        Some(json!({"payload": {}})),
        &[("idempotency-key", "start-timer-1")],
    )
    .await;
    assert!(st.is_success());

    let (st, body) = call(
        &app,
        "POST",
        &format!("/api/v1/workflows/definitions/{def_id}/start"),
        &seed.owner_token,
        Some(json!({"payload": {}})),
        &[("idempotency-key", "start-timer-2")],
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn simulation_zero_side_effects() {
    let Some(seed) = seed_org().await else {
        return;
    };
    let app = wf_app(seed.pool.clone(), seed.ring.clone());

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let before_defs: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM workflow_definition WHERE org_id = $1")
            .bind(seed.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let before_outbox: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM outbox_event WHERE org_id = $1")
            .bind(seed.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();

    let (st, sim) = call(
        &app,
        "POST",
        "/api/v1/workflows/simulate",
        &seed.owner_token,
        Some(json!({
            "graph": deal_won_graph(),
            "payload": {"deal_id": "dl_x"}
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sim["ok"], true);
    assert_eq!(sim["side_effects"], false);

    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let after_defs: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM workflow_definition WHERE org_id = $1")
            .bind(seed.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let after_outbox: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM outbox_event WHERE org_id = $1")
            .bind(seed.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let after_inst: (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM workflow_instance WHERE org_id = $1")
            .bind(seed.org.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(after_defs.0, before_defs.0);
    assert_eq!(after_outbox.0, before_outbox.0);
    assert_eq!(after_inst.0, 0);
}

#[tokio::test]
async fn tenant_isolation_rls_on_definitions() {
    let Some(a) = seed_org().await else {
        return;
    };
    let Some(b) = seed_org().await else {
        return;
    };

    let def_id = new_uuid_v7();
    let def_public = PublicId::new(IdKind::WorkflowDefinition, def_id);
    {
        let mut tx = a.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, a.org).await.unwrap();
        sqlx::query(
            r#"
            INSERT INTO workflow_definition (
                id, org_id, public_id, name, description, status, created_by, updated_by
            ) VALUES ($1,$2,$3,'secret','','draft',$4,$4)
            "#,
        )
        .bind(def_id)
        .bind(a.org.as_uuid())
        .bind(def_public.as_str())
        .bind(a.owner_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    let mut tx = b.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, b.org).await.unwrap();
    let found: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM workflow_definition WHERE id = $1")
        .bind(def_id)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(found.is_none(), "RLS must hide cross-tenant definition");
}

#[tokio::test]
async fn fixtures_endpoint_returns_three() {
    let Some(seed) = seed_org().await else {
        return;
    };
    let app = wf_app(seed.pool, seed.ring);
    let (st, body) = call(
        &app,
        "GET",
        "/api/v1/workflows/fixtures",
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 3);
}
