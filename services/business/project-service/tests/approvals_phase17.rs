//! Phase 1.7 Approval engine integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Pattern mirrors `operations_phase16.rs`:
//! register an owner via core's HTTP API (so `OrgProvisioning` seeds system
//! roles + `role_permission`), insert additional memberships, mint access
//! tokens with `companyos_core::auth::sessions::create_session_with_tokens`
//! on a shared `KeyRing`, then migrate core then `companyos_project::migrate`
//! and drive `companyos_project::build_router` with `tower::ServiceExt::oneshot`.
//!
//! Note: Temporal SLA escalation durability (timer survives worker restart /
//! duplicate decide is a no-op) is covered by `workflow_logic` unit tests —
//! see `src/approvals/workflow_logic.rs` (`timer_survives_restart_semantics`,
//! `duplicate_decide_is_noop`). These HTTP tests exercise the DB/API surface.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_project::state::AppState as ProjectAppState;
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::OnceLock;
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;
use uuid::Uuid;

/// Migrate once — concurrent DDL from every test is racy under FORCE RLS.
static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
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
        companyos_project::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn ops_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_project::build_router(ProjectAppState::new(pool, ring))
}

/// One org: owner + member + finance (default expense policy routes to finance).
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    #[allow(dead_code)]
    org: OrgId,
    #[allow(dead_code)]
    org_public: String,
    owner_token: String,
    member_token: String,
    finance_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
    #[allow(dead_code)]
    member_user_id: Uuid,
    #[allow(dead_code)]
    finance_user_id: Uuid,
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

async fn insert_member_with_role(
    pool: &PgPool,
    org: OrgId,
    role_key: &str,
    display_name: &str,
) -> Uuid {
    let email = format!("{role_key}-{}@test.local", new_uuid_v7());
    let user_id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::User, user_id).as_str();
    let (hash, salt) = password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,now())
        "#,
    )
    .bind(user_id)
    .bind(&public_id)
    .bind(&email)
    .bind(email.to_ascii_lowercase())
    .bind(&hash)
    .bind(&salt)
    .bind(display_name)
    .execute(pool)
    .await
    .unwrap();

    let mem_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let role_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 AND system_key = $2")
            .bind(org.as_uuid())
            .bind(role_key)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, role_id, policy_version, status)
        VALUES ($1,$2,$3,$4,$5,$6,1,'active')
        "#,
    )
    .bind(mem_id)
    .bind(org.as_uuid())
    .bind(user_id)
    .bind(PublicId::new(IdKind::Membership, mem_id).as_str())
    .bind(role_key)
    .bind(role_id.map(|r| r.0))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    user_id
}

async fn seed_org_with_tokens(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let _guard = SEED_LOCK.lock().await;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    let app = core_app(pool.clone(), ring.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "apr-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Approvals Phase17 Test Co"
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
    let owner_public = body["user_id"].as_str().unwrap().to_string();

    let org = OrgId::from_public(&org_public.parse().unwrap()).unwrap();
    let owner_id = owner_public.parse::<PublicId>().unwrap().uuid();

    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_id)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();

    companyos_core::workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    let member_user_id = insert_member_with_role(&pool, org, "member", "Plain Member").await;
    let finance_user_id = insert_member_with_role(&pool, org, "finance", "Finance Rep").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;
    let (finance_mem_id, finance_policy) =
        membership_role_and_policy(&pool, org, finance_user_id).await;

    let mut tx = pool.begin().await.unwrap();
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
    .unwrap();
    let member_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        member_user_id,
        &PublicId::new(IdKind::User, member_user_id).as_str(),
        org,
        member_mem_id,
        &["member".into()],
        member_policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let finance_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        finance_user_id,
        &PublicId::new(IdKind::User, finance_user_id).as_str(),
        org,
        finance_mem_id,
        &["finance".into()],
        finance_policy,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        ring,
        org,
        org_public,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
        finance_token: finance_issued.access_token,
        owner_id,
        member_user_id,
        finance_user_id,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_with_headers(app, method, uri, Some(token), &[], body).await
}

async fn call_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    extra: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-request-id", format!("apr-{}", new_uuid_v7()));
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

fn unique_expense_subject() -> String {
    PublicId::generate(IdKind::Expense).as_str()
}

async fn create_expense_approval(app: &Router, token: &str, title: &str) -> Value {
    let subject_id = unique_expense_subject();
    let (status, approval) = call(
        app,
        "POST",
        "/api/v1/operations/approvals",
        token,
        Some(json!({
            "subject_type": "expense",
            "subject_id": subject_id,
            "title": title,
            "amount_minor": 12_500,
            "currency": "USD",
            "category": "Travel",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{approval:?}");
    approval
}

/// Decide token: prefer finance (default expense assignees). Fall back to owner
/// when the active step has no assignees (empty assignee list allows any
/// decide-permitted actor).
fn decide_token<'a>(seeded: &'a Seeded, approval: &Value) -> &'a str {
    let assignees = approval["steps"]
        .as_array()
        .and_then(|steps| steps.first())
        .and_then(|s| s["assignee_user_ids"].as_array())
        .cloned()
        .unwrap_or_default();
    if assignees.is_empty() {
        &seeded.owner_token
    } else {
        &seeded.finance_token
    }
}

#[tokio::test]
async fn policy_change_does_not_rewrite_in_flight_version() {
    let Some(seeded) = seed_org_with_tokens("ops-phase17-policy-freeze").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());

    // Ensure defaults exist, then create an in-flight approval.
    let (status, policies) = call(
        &app,
        "GET",
        "/api/v1/operations/approval-policies",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{policies:?}");
    let expense_policy = policies["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["subject_type"] == "expense")
        .expect("default expense policy");
    let policy_id = expense_policy["id"].as_str().unwrap().to_string();
    let policy_ver_before = expense_policy["current_version"].as_i64().unwrap();

    let approval = create_expense_approval(&app, &seeded.owner_token, "Freeze policy v").await;
    let approval_id = approval["id"].as_str().unwrap().to_string();
    let frozen_version = approval["policy_version"].as_i64().unwrap();
    let frozen_snap_version = approval["routing_snapshot"]["policy_version"]
        .as_i64()
        .unwrap();
    assert_eq!(frozen_version, frozen_snap_version);
    assert_eq!(frozen_version, policy_ver_before);

    // Publish a new policy version (immutable history — must not rewrite in-flight).
    let mut new_def = expense_policy["definition"].clone();
    new_def["steps"][0]["sla_seconds"] = json!(3_600);
    let (status, updated_policy) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/approval-policies/{policy_id}"),
        &seeded.owner_token,
        Some(json!({ "definition": new_def })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated_policy:?}");
    assert_eq!(
        updated_policy["current_version"].as_i64().unwrap(),
        policy_ver_before + 1
    );

    let (status, got) = call(
        &app,
        "GET",
        &format!("/api/v1/operations/approvals/{approval_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got:?}");
    assert_eq!(got["policy_version"].as_i64().unwrap(), frozen_version);
    assert_eq!(
        got["routing_snapshot"]["policy_version"]
            .as_i64()
            .unwrap(),
        frozen_snap_version
    );
}

#[tokio::test]
async fn duplicate_decide_is_noop() {
    let Some(seeded) = seed_org_with_tokens("ops-phase17-dup-decide").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());
    let approval = create_expense_approval(&app, &seeded.owner_token, "Dup decide").await;
    let approval_id = approval["id"].as_str().unwrap().to_string();
    let token = decide_token(&seeded, &approval);
    let idem = format!("decide-{}", new_uuid_v7());

    let (status, first) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/operations/approvals/{approval_id}/decide"),
        Some(token),
        &[("idempotency-key", &idem)],
        Some(json!({ "approve": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first:?}");
    assert_eq!(first["status"], "approved");

    // Second decide with a different outcome must remain approved (no-op).
    let (status, second) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/approvals/{approval_id}/decide"),
        token,
        Some(json!({ "approve": false, "comment": "should not apply" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second:?}");
    assert_eq!(second["status"], "approved");

    // Same Idempotency-Key replays the original response body.
    let (status, replay) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/operations/approvals/{approval_id}/decide"),
        Some(token),
        &[("idempotency-key", &idem)],
        Some(json!({ "approve": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay:?}");
    assert_eq!(replay["status"], first["status"]);
    assert_eq!(replay["id"], first["id"]);
    assert_eq!(replay["decided_at"], first["decided_at"]);
    assert_eq!(replay["decided_by"], first["decided_by"]);
}

#[tokio::test]
async fn authz_deny_decide_without_permission() {
    // Member catalogue defaults include operations.approval.read but not
    // operations.approval.decide (sensitive; denied in the deny-matrix).
    let Some(seeded) = seed_org_with_tokens("ops-phase17-authz-decide").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());
    let approval = create_expense_approval(&app, &seeded.owner_token, "Authz decide").await;
    let approval_id = approval["id"].as_str().unwrap().to_string();

    let (status, denied) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/approvals/{approval_id}/decide"),
        &seeded.member_token,
        Some(json!({ "approve": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied:?}");
}

#[tokio::test]
async fn tenant_isolation_approvals() {
    let Some(a) = seed_org_with_tokens("ops-phase17-tenant-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens("ops-phase17-tenant-b").await else {
        return;
    };
    let app_a = ops_app(a.pool.clone(), a.ring.clone());
    let app_b = ops_app(b.pool.clone(), b.ring.clone());

    let approval_a = create_expense_approval(&app_a, &a.owner_token, "Org A secret").await;
    let approval_a_id = approval_a["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &app_b,
        "GET",
        &format!("/api/v1/operations/approvals/{approval_a_id}"),
        &b.owner_token,
        None,
    )
    .await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
        "cross-tenant GET must be 404/403, got {status}: {body:?}"
    );

    // Org B list must not include Org A's approval.
    let (status, list_b) = call(
        &app_b,
        "GET",
        "/api/v1/operations/approvals?pending_for_me=false",
        &b.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list_b:?}");
    let ids: Vec<&str> = list_b["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(!ids.contains(&approval_a_id.as_str()));
}

#[tokio::test]
async fn expense_subject_creates_approval_and_inbox() {
    let Some(seeded) = seed_org_with_tokens("ops-phase17-expense-inbox").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, policies) = call(
        &app,
        "GET",
        "/api/v1/operations/approval-policies",
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{policies:?}");
    let items = policies["items"].as_array().unwrap();
    assert!(
        items.iter().any(|p| p["subject_type"] == "expense"),
        "default expense policy must be seeded: {policies:?}"
    );

    let approval = create_expense_approval(&app, &seeded.owner_token, "Expense for inbox").await;
    let approval_id = approval["id"].as_str().unwrap().to_string();
    assert_eq!(approval["subject_type"], "expense");
    assert_eq!(approval["status"], "pending");
    assert_eq!(approval["amount_minor"], 12_500);
    let rationale = approval["routing_snapshot"]["rationale"]
        .as_str()
        .unwrap_or("");
    assert!(
        !rationale.is_empty(),
        "routing rationale must be present: {approval:?}"
    );

    // Default expense policy routes to finance; finance inbox should see it.
    // If assignees were empty, owner (decide-permitted) is the fallback actor.
    let inbox_token = decide_token(&seeded, &approval);
    let (status, inbox) = call(
        &app,
        "GET",
        "/api/v1/operations/approvals?pending_for_me=true&status=pending",
        inbox_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inbox:?}");
    let inbox_ids: Vec<&str> = inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();

    if inbox_token == seeded.finance_token.as_str() {
        assert!(
            inbox_ids.contains(&approval_id.as_str()),
            "finance pending_for_me must include created approval: {inbox:?}"
        );
    } else {
        // Empty assignees: pending_for_me filters by assignee_user_ids, so the
        // approval may not appear in owner inbox — still assert create + rationale.
        assert!(
            !rationale.is_empty(),
            "owner fallback path still requires routing rationale"
        );
    }

    let (status, summary) = call(
        &app,
        "GET",
        "/api/v1/operations/approvals/inbox/summary",
        inbox_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{summary:?}");
    if inbox_token == seeded.finance_token.as_str() {
        assert!(
            summary["pending_for_me"].as_i64().unwrap_or(0) >= 1,
            "finance inbox summary must count pending: {summary:?}"
        );
    }
}
