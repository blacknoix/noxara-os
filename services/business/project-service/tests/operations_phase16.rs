//! Phase 1.6 Operations (projects/tasks) integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Pattern mirrors `services/business/finance-service/tests/finance_phase15.rs`:
//! register an owner via core's HTTP API (so `OrgProvisioning` seeds system
//! roles + `role_permission`), insert additional memberships, mint access
//! tokens with `companyos_core::auth::sessions::create_session_with_tokens`
//! on a shared `KeyRing`, then migrate core then `companyos_project::migrate`
//! and drive `companyos_project::build_router` with `tower::ServiceExt::oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_project::state::AppState as ProjectAppState;
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
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

/// One org: owner + member (+ finance for mention authz). Optional second org
/// is seeded separately for tenant isolation.
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    #[allow(dead_code)]
    org_public: String,
    owner_token: String,
    member_token: String,
    #[allow(dead_code)]
    finance_token: String,
    owner_id: Uuid,
    member_user_id: Uuid,
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
                .header("x-request-id", "ops-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Ops Phase16 Test Co"
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
        .header("x-request-id", format!("ops-{}", new_uuid_v7()));
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

async fn create_project(app: &Router, token: &str, name: &str) -> Value {
    let (status, proj) = call(
        app,
        "POST",
        "/api/v1/operations/projects",
        token,
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{proj:?}");
    proj
}

async fn create_task(app: &Router, token: &str, project_id: &str, title: &str) -> Value {
    let (status, task) = call(
        app,
        "POST",
        "/api/v1/operations/tasks",
        token,
        Some(json!({
            "project_id": project_id,
            "title": title,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task:?}");
    task
}

/// Grant member role `operations.task.update` at scope=own only (remove any
/// organization-scoped row for that permission).
async fn grant_member_task_update_own(pool: &PgPool, org: OrgId) {
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
        DELETE FROM role_permission
        WHERE role_id = $1 AND permission_id = 'operations.task.update' AND effect = 'allow'
        "#,
    )
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO role_permission (id, org_id, role_id, permission_id, effect, scope)
        VALUES ($1, $2, $3, 'operations.task.update', 'allow', 'own')
        "#,
    )
    .bind(new_uuid_v7())
    .bind(org.as_uuid())
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    // Do not bump membership.policy_version — access JWTs embed it and
    // AuthCtx rejects mismatched versions. PDP reloads role_permission each request.
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn tenant_isolation_projects_and_tasks() {
    let Some(a) = seed_org_with_tokens("ops-phase16-tenant-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens("ops-phase16-tenant-b").await else {
        return;
    };
    let app_a = ops_app(a.pool.clone(), a.ring.clone());
    let app_b = ops_app(b.pool.clone(), b.ring.clone());

    let proj_b = create_project(&app_b, &b.owner_token, "Org B Secret Project").await;
    let proj_b_id = proj_b["id"].as_str().unwrap().to_string();
    let task_b = create_task(&app_b, &b.owner_token, &proj_b_id, "Secret task").await;
    let task_b_id = task_b["id"].as_str().unwrap().to_string();

    let (status, _) = call(
        &app_a,
        "GET",
        &format!("/api/v1/operations/projects/{proj_b_id}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, list_a) = call(
        &app_a,
        "GET",
        "/api/v1/operations/projects",
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&proj_b_id.as_str()));

    let (status, _) = call(
        &app_a,
        "GET",
        &format!("/api/v1/operations/tasks/{task_b_id}"),
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, tasks_a) = call(
        &app_a,
        "GET",
        "/api/v1/operations/tasks",
        &a.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let task_ids: Vec<&str> = tasks_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert!(!task_ids.contains(&task_b_id.as_str()));

    // RLS under org A session must return empty for B's ids.
    let mut tx = a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, a.org).await.unwrap();
    let foreign_proj: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM operations_project WHERE public_id = $1")
            .bind(&proj_b_id)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(
        foreign_proj.is_empty(),
        "RLS must hide org B operations_project from org A"
    );
    let foreign_task: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM operations_task WHERE public_id = $1")
            .bind(&task_b_id)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(
        foreign_task.is_empty(),
        "RLS must hide org B operations_task from org A"
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn authz_deny_task_update_wrong_scope() {
    let Some(seeded) = seed_org_with_tokens("ops-phase16-authz-scope").await else {
        eprintln!("skipping: no database");
        return;
    };
    grant_member_task_update_own(&seeded.pool, seeded.org).await;
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());

    let proj = create_project(&app, &seeded.owner_token, "Authz Scope Project").await;
    let project_id = proj["id"].as_str().unwrap();

    let owner_task = create_task(&app, &seeded.owner_token, project_id, "Owner-owned").await;
    let owner_task_id = owner_task["id"].as_str().unwrap();
    let owner_ver = owner_task["version"].as_i64().unwrap();

    let (status, denied) = call_with_headers(
        &app,
        "PATCH",
        &format!("/api/v1/operations/tasks/{owner_task_id}"),
        Some(&seeded.member_token),
        &[("if-match", &owner_ver.to_string())],
        Some(json!({ "title": "hacked" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied:?}");

    let member_task = create_task(&app, &seeded.member_token, project_id, "Member-owned").await;
    let member_task_id = member_task["id"].as_str().unwrap();
    let member_ver = member_task["version"].as_i64().unwrap();

    let (status, updated) = call_with_headers(
        &app,
        "PATCH",
        &format!("/api/v1/operations/tasks/{member_task_id}"),
        Some(&seeded.member_token),
        &[("if-match", &member_ver.to_string())],
        Some(json!({ "title": "member updated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated["title"], "member updated");
}

#[tokio::test]
async fn mention_without_permission_does_not_notify() {
    // Finance role defaults have no operations.* (including task.read). Member
    // retains operations.task.read, so only the member should be notified.
    let Some(seeded) = seed_org_with_tokens("ops-phase16-mention").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());

    let proj = create_project(&app, &seeded.owner_token, "Mention Project").await;
    let task = create_task(
        &app,
        &seeded.owner_token,
        proj["id"].as_str().unwrap(),
        "Mention target",
    )
    .await;
    let task_id = task["id"].as_str().unwrap();

    let member_pub = PublicId::new(IdKind::User, seeded.member_user_id).as_str();
    let finance_pub = PublicId::new(IdKind::User, seeded.finance_user_id).as_str();
    let body = format!("Hey @{member_pub} and @{finance_pub} please review");

    let (status, comment) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/tasks/{task_id}/comments"),
        &seeded.owner_token,
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{comment:?}");

    let mentioned = comment["mentioned_user_ids"].as_array().unwrap();
    let mentioned_strs: Vec<&str> = mentioned.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        mentioned_strs.contains(&member_pub.as_str()),
        "member with task.read must appear: {mentioned_strs:?}"
    );
    assert!(
        !mentioned_strs.contains(&finance_pub.as_str()),
        "finance without task.read must be excluded: {mentioned_strs:?}"
    );

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let recipients: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT recipient_user_id
        FROM operations_notification_intent
        WHERE org_id = $1 AND kind = 'mention'
        "#,
    )
    .bind(seeded.org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert!(
        recipients.iter().any(|(u,)| *u == seeded.member_user_id),
        "member must receive mention intent"
    );
    assert!(
        !recipients.iter().any(|(u,)| *u == seeded.finance_user_id),
        "finance must not receive mention intent"
    );
}

#[tokio::test]
async fn board_move_conflict_returns_409() {
    let Some(seeded) = seed_org_with_tokens("ops-phase16-board-409").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());
    let proj = create_project(&app, &seeded.owner_token, "Board Project").await;
    let task = create_task(
        &app,
        &seeded.owner_token,
        proj["id"].as_str().unwrap(),
        "Movable",
    )
    .await;
    let task_id = task["id"].as_str().unwrap();
    let version = task["version"].as_i64().unwrap();

    let (status, conflict) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/operations/tasks/{task_id}/move"),
        Some(&seeded.owner_token),
        &[("if-match", &(version + 99).to_string())],
        Some(json!({ "status": "todo" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict:?}");

    let (status, moved) = call_with_headers(
        &app,
        "POST",
        &format!("/api/v1/operations/tasks/{task_id}/move"),
        Some(&seeded.owner_token),
        &[("if-match", &version.to_string())],
        Some(json!({ "status": "todo" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved:?}");
    assert_eq!(moved["status"], "todo");
}

#[tokio::test]
async fn my_work_index_exists() {
    // Do NOT load 50k rows to prove the plan — assert the composite index
    // `operations_task_my_work_idx` exists and covers org_id + assignee_id
    // (plus status, due_at) for the My Work access path.
    let Some(pool) = pool().await else {
        eprintln!("skipping: no database");
        return;
    };

    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT indexname, indexdef
        FROM pg_indexes
        WHERE indexname = 'operations_task_my_work_idx'
        "#,
    )
    .fetch_optional(&pool)
    .await
    .unwrap();

    let Some((name, def)) = row else {
        // Fallback via pg_class for environments that only expose catalog that way.
        let exists: Option<(String,)> = sqlx::query_as(
            "SELECT relname FROM pg_class WHERE relname = 'operations_task_my_work_idx'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            exists.is_some(),
            "operations_task_my_work_idx must exist (pg_class)"
        );
        return;
    };

    assert_eq!(name, "operations_task_my_work_idx");
    let def_l = def.to_lowercase();
    assert!(
        def_l.contains("org_id") && def_l.contains("assignee_id"),
        "indexdef must include org_id and assignee_id: {def}"
    );
    assert!(
        def_l.contains("status") && def_l.contains("due_at"),
        "indexdef should include status and due_at: {def}"
    );
}

#[tokio::test]
async fn project_created_from_deal_won() {
    let Some(seeded) = seed_org_with_tokens("ops-phase16-deal-won").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());

    let deal_id = PublicId::generate(IdKind::Deal).as_str();
    let customer_id = PublicId::generate(IdKind::Customer).as_str();
    let envelope = EventEnvelope::new(
        seeded.org,
        Context::Sales,
        "deal",
        "won",
        1,
        Actor::human(seeded.owner_id),
        json!({
            "id": deal_id,
            "name": "Won Deal Project",
            "customer_id": customer_id,
        }),
    );

    let (status, first) = call(
        &app,
        "POST",
        "/api/v1/operations/events/sales/apply",
        &seeded.owner_token,
        Some(json!({ "envelope": envelope })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first:?}");
    assert_eq!(first["applied"], true);
    let project_id = first["project_id"].as_str().unwrap().to_string();

    let (status, get) = call(
        &app,
        "GET",
        &format!("/api/v1/operations/projects/{project_id}"),
        &seeded.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{get:?}");
    assert_eq!(get["deal_id"], deal_id);

    // Second apply is idempotent — same project.
    let envelope2 = EventEnvelope::new(
        seeded.org,
        Context::Sales,
        "deal",
        "won",
        1,
        Actor::human(seeded.owner_id),
        json!({
            "id": deal_id,
            "name": "Won Deal Project",
            "customer_id": customer_id,
        }),
    );
    let (status, second) = call(
        &app,
        "POST",
        "/api/v1/operations/events/sales/apply",
        &seeded.owner_token,
        Some(json!({ "envelope": envelope2 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second:?}");
    assert_eq!(second["applied"], true);
    assert_eq!(second["project_id"], project_id);

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM operations_project WHERE org_id = $1 AND deal_public_id = $2 AND deleted_at IS NULL",
    )
    .bind(seeded.org.as_uuid())
    .bind(&deal_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn task_events_outbox() {
    let Some(seeded) = seed_org_with_tokens("ops-phase16-outbox").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(seeded.pool.clone(), seeded.ring.clone());
    let proj = create_project(&app, &seeded.owner_token, "Outbox Project").await;
    let project_id = proj["id"].as_str().unwrap();

    let task = create_task(&app, &seeded.owner_token, project_id, "Outbox task").await;
    let task_id = task["id"].as_str().unwrap();
    let version = task["version"].as_i64().unwrap();
    let member_pub = PublicId::new(IdKind::User, seeded.member_user_id).as_str();

    // Assign → operations.task.assigned.v1
    let (status, assigned) = call_with_headers(
        &app,
        "PATCH",
        &format!("/api/v1/operations/tasks/{task_id}"),
        Some(&seeded.owner_token),
        &[("if-match", &version.to_string())],
        Some(json!({ "assignee_id": member_pub })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{assigned:?}");
    let version2 = assigned["version"].as_i64().unwrap();

    // Complete → operations.task.completed.v1
    let (status, done) = call_with_headers(
        &app,
        "PATCH",
        &format!("/api/v1/operations/tasks/{task_id}"),
        Some(&seeded.owner_token),
        &[("if-match", &version2.to_string())],
        Some(json!({ "status": "done" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done:?}");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let subjects: Vec<(String,)> =
        sqlx::query_as("SELECT subject FROM outbox_event WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();

    let joined: Vec<&str> = subjects.iter().map(|(s,)| s.as_str()).collect();
    assert!(
        joined
            .iter()
            .any(|s| s.contains("operations.task.created.v1")),
        "missing created: {joined:?}"
    );
    assert!(
        joined
            .iter()
            .any(|s| s.contains("operations.task.completed.v1")),
        "missing completed: {joined:?}"
    );
    assert!(
        joined
            .iter()
            .any(|s| s.contains("operations.task.assigned.v1")),
        "missing assigned: {joined:?}"
    );
}
