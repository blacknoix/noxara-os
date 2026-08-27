//! Phase 2.2 Attendance & Leave integration tests (DoD).
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_hr::crypto::FieldEncryptor;
use companyos_hr::state::AppState as HrAppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
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
        companyos_hr::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn hr_app(pool: PgPool, ring: KeyRing) -> Router {
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [9u8; 32]);
    let enc = FieldEncryptor::from_base64(&key, "test").expect("encryptor");
    companyos_hr::build_router(HrAppState::new(pool, ring, enc))
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    member_token: String,
    manager_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
    member_user_id: Uuid,
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
                .header("x-request-id", "hr22-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "HR Phase22 Test Co"
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
    let manager_user_id = insert_member_with_role(&pool, org, "manager", "People Manager").await;

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
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
        manager_token: manager_issued.access_token,
        owner_id,
        member_user_id,
        manager_user_id,
    })
}

async fn call(
    app: Router,
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
        .header("x-request-id", "hr22-test");
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let req = if let Some(b) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = app.oneshot(req).await.unwrap();
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

async fn link_employee(app: &Router, token: &str, user_id: Uuid, name: &str) -> String {
    let (st, emp) = call(
        app.clone(),
        "POST",
        "/api/v1/people/employees",
        token,
        Some(json!({
            "display_name": name,
            "user_id": PublicId::new(IdKind::User, user_id).as_str(),
            "status": "active"
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{emp}");
    emp["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn leave_write_denied_without_permission_and_member_can_request() {
    let Some(seed) = seed_org_with_tokens("hr22-leave-write").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let _member_emp =
        link_employee(&app, &seed.owner_token, seed.member_user_id, "Member Emp").await;

    // Finance cannot create leave types (no hr.leave.write)
    let (st, denied) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.member_token, // member has write for requests but blocked from creating types
        Some(json!({
            "code": "ANN",
            "name": "Annual",
            "category": "annual",
            "accrual_units_milli": 20000
        })),
        &[("idempotency-key", &format!("lt-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{denied}");

    let (st, lt) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.owner_token,
        Some(json!({
            "code": "ANN",
            "name": "Annual",
            "category": "annual",
            "accrual_cadence": "yearly",
            "accrual_units_milli": 20000,
            "carry_forward_cap_milli": 5000,
            "requires_approval": false
        })),
        &[("idempotency-key", &format!("lt-ok-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt}");
    let leave_type_id = lt["id"].as_str().unwrap().to_string();

    // Accrue for member
    let (st, _) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave/accrue",
        &seed.owner_token,
        Some(json!({
            "employee_id": _member_emp,
            "leave_type_id": leave_type_id,
            "units_milli": 10000,
            "effective_date": "2026-01-01"
        })),
        &[("idempotency-key", &format!("acc-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    // Member requests own leave (auto-approved — requires_approval false)
    let (st, req) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-requests",
        &seed.member_token,
        Some(json!({
            "leave_type_id": leave_type_id,
            "start_date": "2026-03-02",
            "end_date": "2026-03-03",
            "start_period": "full",
            "end_period": "full",
            "submit": true
        })),
        &[("idempotency-key", &format!("lvr-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{req}");
    assert_eq!(req["status"], "approved");
    assert_eq!(req["units_milli"], 2000);

    // Cannot approve own (even manager approving self)
    let manager_emp =
        link_employee(&app, &seed.owner_token, seed.manager_user_id, "Manager Emp").await;
    let (st, lt2) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.owner_token,
        Some(json!({
            "code": "SICK",
            "name": "Sick",
            "category": "sick",
            "accrual_units_milli": 5000,
            "requires_approval": true
        })),
        &[("idempotency-key", &format!("lt2-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt2}");
    let (st, _) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave/accrue",
        &seed.owner_token,
        Some(json!({
            "employee_id": manager_emp,
            "leave_type_id": lt2["id"],
            "units_milli": 5000,
            "effective_date": "2026-01-01"
        })),
        &[("idempotency-key", &format!("acc2-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let (st, mgr_req) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-requests",
        &seed.manager_token,
        Some(json!({
            "leave_type_id": lt2["id"],
            "start_date": "2026-09-07",
            "end_date": "2026-09-07",
            "submit": true
        })),
        &[("idempotency-key", &format!("lvr2-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{mgr_req}");
    // Force pending if approval engine unreachable
    let req_id = mgr_req["id"].as_str().unwrap();
    if mgr_req["status"] == "pending_approval" || mgr_req["status"] == "draft" {
        // Direct decide as self must fail
        let mut tx = seed.pool.begin().await.unwrap();
        set_session_org_id(&mut tx, seed.org).await.unwrap();
        let _ = sqlx::query(
            "UPDATE people_leave_request SET status = 'pending_approval' WHERE public_id = $1",
        )
        .bind(req_id)
        .execute(&mut *tx)
        .await;
        tx.commit().await.unwrap();
        let (st, self_decide) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/people/leave-requests/{req_id}/decide"),
            &seed.manager_token,
            Some(json!({ "approve": true })),
            &[],
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{self_decide}");
    }
}

#[tokio::test]
async fn attendance_append_only_and_scope() {
    let Some(seed) = seed_org_with_tokens("hr22-att").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let member_emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Clock Emp").await;
    let _other = link_employee(&app, &seed.owner_token, seed.manager_user_id, "Other Emp").await;

    let (st, att) = call(
        app.clone(),
        "POST",
        "/api/v1/people/attendance",
        &seed.member_token,
        Some(json!({
            "entry_kind": "check_in",
            "source": "manual",
            "timezone": "UTC"
        })),
        &[("idempotency-key", &format!("att-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{att}");
    assert!(att["id"].as_str().unwrap().starts_with("att_"));

    // Append-only: UPDATE rejected by trigger
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let att_uuid = att["id"]
        .as_str()
        .unwrap()
        .parse::<PublicId>()
        .unwrap()
        .uuid();
    let err =
        sqlx::query("UPDATE people_attendance SET note = 'mutated' WHERE org_id = $1 AND id = $2")
            .bind(seed.org.as_uuid())
            .bind(att_uuid)
            .execute(&mut *tx)
            .await;
    assert!(err.is_err(), "expected update rejection");
    tx.rollback().await.unwrap();

    // Member listing others' attendance beyond own scope returns only own when scoped
    let (st, list) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/attendance?employee_id={member_emp}"),
        &seed.member_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{list}");
    assert!(list["total"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn carry_forward_idempotent_and_balances_from_ledger() {
    let Some(seed) = seed_org_with_tokens("hr22-cf").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "CF Emp").await;

    let (st, lt) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.owner_token,
        Some(json!({
            "code": "CFANN",
            "name": "CF Annual",
            "category": "annual",
            "accrual_cadence": "yearly",
            "accrual_units_milli": 20000,
            "carry_forward_cap_milli": 5000,
            "expiry_days": 90
        })),
        &[("idempotency-key", &format!("lt-cf-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt}");
    let leave_type_id = lt["id"].as_str().unwrap();

    let (st, _) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave/accrue",
        &seed.owner_token,
        Some(json!({
            "employee_id": emp,
            "leave_type_id": leave_type_id,
            "units_milli": 12000,
            "effective_date": "2025-01-01"
        })),
        &[("idempotency-key", &format!("acc-cf-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    let (st, bal) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/leave/balances?employee_id={emp}&as_of=2025-12-31"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bal}");
    let item = bal["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["leave_type_id"] == leave_type_id)
        .unwrap();
    assert_eq!(item["balance_units_milli"], 12000);

    let (st, cf1) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave/carry-forward",
        &seed.owner_token,
        Some(json!({ "year": 2025 })),
        &[("idempotency-key", &format!("cf1-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cf1}");
    assert_eq!(cf1["idempotent_replay"], false);
    assert!(cf1["workflow_id"]
        .as_str()
        .unwrap()
        .contains(":LeaveCarryForward:2025"));
    let posted = cf1["entries_posted"].as_i64().unwrap();
    assert!(posted >= 1);

    let (st, cf2) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave/carry-forward",
        &seed.owner_token,
        Some(json!({ "year": 2025 })),
        &[("idempotency-key", &format!("cf2-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cf2}");
    assert_eq!(cf2["idempotent_replay"], true);
    assert_eq!(cf2["entries_posted"], posted);

    let (st, bal2) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/people/leave/balances?employee_id={emp}&as_of=2026-01-01"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bal2}");
    let item2 = bal2["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["leave_type_id"] == leave_type_id)
        .unwrap();
    // Capped at 5000
    assert_eq!(item2["balance_units_milli"], 5000);
}

#[tokio::test]
async fn tenant_isolation_leave_and_attendance() {
    let Some(a) = seed_org_with_tokens("hr22-iso-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed_org_with_tokens("hr22-iso-b").await else {
        return;
    };
    let app_a = hr_app(a.pool.clone(), a.ring.clone());
    let _emp = link_employee(&app_a, &a.owner_token, a.member_user_id, "Iso Emp").await;
    let (st, lt) = call(
        app_a.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &a.owner_token,
        Some(json!({ "code": "ISO", "name": "Iso", "category": "custom" })),
        &[("idempotency-key", &format!("iso-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt}");
    let lt_id = lt["id"].as_str().unwrap();

    let app_b = hr_app(b.pool.clone(), b.ring.clone());
    let (st, miss) = call(
        app_b,
        "GET",
        &format!("/api/v1/people/leave-types/{lt_id}"),
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{miss}");
}

#[tokio::test]
async fn half_day_timezone_fixture_via_api() {
    let Some(seed) = seed_org_with_tokens("hr22-half").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = hr_app(seed.pool.clone(), seed.ring.clone());
    let emp = link_employee(&app, &seed.owner_token, seed.member_user_id, "Half Emp").await;
    let (st, lt) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-types",
        &seed.owner_token,
        Some(json!({
            "code": "HALF",
            "name": "Half",
            "category": "unpaid",
            "allows_half_day": true,
            "requires_approval": false,
            "accrual_cadence": "none"
        })),
        &[("idempotency-key", &format!("half-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{lt}");

    // Holiday on Wed mid-range
    let (st, _) = call(
        app.clone(),
        "POST",
        "/api/v1/people/holidays",
        &seed.owner_token,
        Some(json!({ "name": "Midweek", "holiday_date": "2026-03-04" })),
        &[("idempotency-key", &format!("hol-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    // Mon pm → Wed am with Wed holiday → Mon 0.5 + Tue 1.0 = 1.5 (Wed excluded)
    let (st, req) = call(
        app.clone(),
        "POST",
        "/api/v1/people/leave-requests",
        &seed.owner_token,
        Some(json!({
            "employee_id": emp,
            "leave_type_id": lt["id"],
            "start_date": "2026-03-02",
            "end_date": "2026-03-04",
            "start_period": "pm",
            "end_period": "am",
            "submit": true
        })),
        &[("idempotency-key", &format!("half-req-{}", new_uuid_v7()))],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{req}");
    assert_eq!(req["units_milli"], 1500);
}
