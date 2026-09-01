//! Phase 3.5 timesheets + capacity integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

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

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    #[allow(dead_code)]
    org_public: String,
    owner_token: String,
    member_token: String,
    manager_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
    member_user_id: Uuid,
    #[allow(dead_code)]
    manager_user_id: Uuid,
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
                .header("x-request-id", "p35-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Phase35 Timesheets Co"
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
    let manager_user_id = insert_member_with_role(&pool, org, "manager", "Team Manager").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;
    let (manager_mem_id, manager_policy) =
        membership_role_and_policy(&pool, org, manager_user_id).await;

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
    let manager_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        manager_user_id,
        &PublicId::new(IdKind::User, manager_user_id).as_str(),
        org,
        manager_mem_id,
        &["manager".into()],
        manager_policy,
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
        manager_token: manager_issued.access_token,
        owner_id,
        member_user_id,
        manager_user_id,
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-request-id", format!("p35-{}", new_uuid_v7()));
    req = req.header("authorization", format!("Bearer {token}"));
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

/// Monday of a known week for deterministic tests.
fn week_start() -> &'static str {
    "2026-03-02" // Monday
}

#[tokio::test]
async fn timesheet_create_entries_submit_approve() {
    let Some(s) = seed_org_with_tokens("phase35-happy").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(s.pool.clone(), s.ring.clone());
    let proj = create_project(&app, &s.owner_token, "Billable Work").await;

    let (status, sheet) = call(
        &app,
        "POST",
        "/api/v1/operations/timesheets",
        &s.member_token,
        Some(json!({ "week_start": week_start() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sheet:?}");
    assert_eq!(sheet["week_start"], week_start());
    assert_eq!(sheet["status"], "draft");
    let sheet_id = sheet["id"].as_str().unwrap();

    let (status, entry) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.member_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-03",
            "minutes": 480,
            "billable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{entry:?}");
    assert_eq!(entry["minutes"], 480);

    let (status, entry2) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.member_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-04",
            "minutes": 240,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{entry2:?}");

    let (status, got) = call(
        &app,
        "GET",
        &format!("/api/v1/operations/timesheets/{sheet_id}"),
        &s.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got:?}");
    assert_eq!(got["entries"].as_array().unwrap().len(), 2);

    let (status, submitted) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/submit"),
        &s.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted:?}");
    assert_eq!(submitted["status"], "submitted");

    let (status, approved) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/approve"),
        &s.manager_token,
        Some(json!({ "note": "lgtm" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved:?}");
    assert_eq!(approved["status"], "approved");
}

#[tokio::test]
async fn member_cannot_approve_others_timesheet() {
    let Some(s) = seed_org_with_tokens("phase35-deny-approve").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(s.pool.clone(), s.ring.clone());
    let proj = create_project(&app, &s.owner_token, "Owner Project").await;

    // Owner creates + submits a sheet; member tries to approve.
    let (status, sheet) = call(
        &app,
        "POST",
        "/api/v1/operations/timesheets",
        &s.owner_token,
        Some(json!({ "week_start": week_start() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sheet:?}");
    let sheet_id = sheet["id"].as_str().unwrap();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.owner_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-02",
            "minutes": 60,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/submit"),
        &s.owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/approve"),
        &s.member_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body:?}");
}

#[tokio::test]
async fn cannot_edit_another_members_submitted_sheet_without_approve() {
    let Some(s) = seed_org_with_tokens("phase35-edit-submitted").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(s.pool.clone(), s.ring.clone());
    let proj = create_project(&app, &s.owner_token, "Member Project").await;
    let member_pub = PublicId::new(IdKind::User, s.member_user_id).as_str();

    let (status, sheet) = call(
        &app,
        "POST",
        "/api/v1/operations/timesheets",
        &s.member_token,
        Some(json!({ "week_start": week_start() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sheet:?}");
    let sheet_id = sheet["id"].as_str().unwrap();

    let (status, entry) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.member_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-02",
            "minutes": 120,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{entry:?}");
    let entry_id = entry["id"].as_str().unwrap();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/submit"),
        &s.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Insert a second member without approve — they should not edit submitted sheet.
    let other_id = insert_member_with_role(&s.pool, s.org, "member", "Other Member").await;
    let (other_mem, other_pol) = membership_role_and_policy(&s.pool, s.org, other_id).await;
    let mut tx = s.pool.begin().await.unwrap();
    let other_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &s.ring,
        other_id,
        &PublicId::new(IdKind::User, other_id).as_str(),
        s.org,
        other_mem,
        &["member".into()],
        other_pol,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &other_issued.access_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-03",
            "minutes": 30,
        })),
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::CONFLICT,
        "expected forbid/conflict editing submitted sheet, got {status}: {body:?}"
    );

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries/{entry_id}"),
        &other_issued.access_token,
        Some(json!({ "minutes": 999 })),
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::CONFLICT,
        "expected forbid/conflict patching submitted entry, got {status}: {body:?}"
    );

    // Manager with approve can still mutate when needed (policy allows).
    let _ = member_pub;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.manager_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-05",
            "minutes": 15,
        })),
    )
    .await;
    // Manager has approve → assert_can_edit_draft allows non-draft edits.
    assert_eq!(status, StatusCode::CREATED, "{body:?}");
}

#[tokio::test]
async fn capacity_overload_view() {
    let Some(s) = seed_org_with_tokens("phase35-overload").await else {
        eprintln!("skipping: no database");
        return;
    };
    let app = ops_app(s.pool.clone(), s.ring.clone());
    let proj = create_project(&app, &s.owner_token, "Capacity Proj").await;
    let member_pub = PublicId::new(IdKind::User, s.member_user_id).as_str();

    // Capacity 400 minutes for the week; book 480 submitted → overload 80.
    let (status, alloc) = call(
        &app,
        "POST",
        "/api/v1/operations/capacity/allocations",
        &s.manager_token,
        Some(json!({
            "membership_user_id": member_pub,
            "period_start": "2026-03-02",
            "period_end": "2026-03-08",
            "capacity_minutes": 400,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{alloc:?}");
    assert_eq!(alloc["capacity_minutes"], 400);

    let (status, sheet) = call(
        &app,
        "POST",
        "/api/v1/operations/timesheets",
        &s.member_token,
        Some(json!({ "week_start": week_start() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sheet:?}");
    let sheet_id = sheet["id"].as_str().unwrap();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &s.member_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-03",
            "minutes": 480,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/submit"),
        &s.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, overload) = call(
        &app,
        "GET",
        "/api/v1/operations/capacity/overload?from=2026-03-02&to=2026-03-08",
        &s.manager_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overload:?}");
    let items = overload["items"].as_array().unwrap();
    let row = items
        .iter()
        .find(|i| i["member_id"] == member_pub)
        .expect("member overload row");
    assert_eq!(row["capacity_minutes"], 400);
    assert_eq!(row["booked_minutes"], 480);
    assert_eq!(row["overload_minutes"], 80);
}

#[tokio::test]
async fn timesheet_rls_second_org() {
    let Some(a) = seed_org_with_tokens("phase35-rls-a").await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens("phase35-rls-b").await else {
        return;
    };
    let app_a = ops_app(a.pool.clone(), a.ring.clone());
    let app_b = ops_app(b.pool.clone(), b.ring.clone());

    let proj = create_project(&app_a, &a.owner_token, "Org A Project").await;
    let (status, sheet) = call(
        &app_a,
        "POST",
        "/api/v1/operations/timesheets",
        &a.member_token,
        Some(json!({ "week_start": week_start() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sheet:?}");
    let sheet_id = sheet["id"].as_str().unwrap();

    let (status, _) = call(
        &app_a,
        "POST",
        &format!("/api/v1/operations/timesheets/{sheet_id}/entries"),
        &a.member_token,
        Some(json!({
            "project_id": proj["id"],
            "entry_date": "2026-03-02",
            "minutes": 60,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Org B cannot see Org A timesheet.
    let (status, body) = call(
        &app_b,
        "GET",
        &format!("/api/v1/operations/timesheets/{sheet_id}"),
        &b.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:?}");

    let (status, list) = call(
        &app_b,
        "GET",
        "/api/v1/operations/timesheets",
        &b.member_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list:?}");
    let items = list["items"].as_array().unwrap();
    assert!(
        items.iter().all(|i| i["id"] != sheet_id),
        "org B must not list org A timesheet"
    );
}
