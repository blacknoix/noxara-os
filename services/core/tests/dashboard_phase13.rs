//! Phase 1.3 dashboard BFF integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_core::state::AppState;
use companyos_core::{build_router, migrate, workspace};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn pool() -> Option<sqlx::PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()?;
    migrate(&pool).await.ok()?;
    Some(pool)
}

async fn app_state(pool: sqlx::PgPool) -> AppState {
    let ring = KeyRing::from_secret("test-auth-secret-phase13");
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .expect("jwks");
    AppState::new(pool, ring)
}

struct Seeded {
    pool: sqlx::PgPool,
    org: OrgId,
    #[allow(dead_code)]
    owner_id: Uuid,
    #[allow(dead_code)]
    owner_public: String,
    #[allow(dead_code)]
    member_id: Uuid,
    #[allow(dead_code)]
    member_public: String,
    owner_token: String,
    member_token: String,
}

async fn seed_org_with_tokens() -> Option<Seeded> {
    let pool = pool().await?;
    let state = app_state(pool.clone()).await;
    let app = build_router(state.clone());

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let member_email = format!("member-{}@test.local", new_uuid_v7());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-request-id", "reg-dash")
                .body(Body::from(
                    serde_json::json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Dashboard Test Co"
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

    workspace::provisioning::process_pending(&pool, org, "test")
        .await
        .ok();

    let member_id = new_uuid_v7();
    let member_public = PublicId::new(IdKind::User, member_id).as_str();
    let (hash, salt) = password::hash_password("correct-horse-battery-staple").unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_identity (
            id, public_id, email, email_normalized, password_hash, password_salt,
            display_name, email_verified_at
        ) VALUES ($1,$2,$3,$4,$5,$6,'Member',now())
        "#,
    )
    .bind(member_id)
    .bind(&member_public)
    .bind(&member_email)
    .bind(member_email.to_ascii_lowercase())
    .bind(&hash)
    .bind(&salt)
    .execute(&pool)
    .await
    .unwrap();

    let mem_id = new_uuid_v7();
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let role_id: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_role WHERE org_id = $1 AND system_key = 'member'")
            .bind(org.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO membership (id, org_id, user_id, public_id, role, role_id, policy_version, status)
        VALUES ($1,$2,$3,$4,'member',$5,1,'active')
        "#,
    )
    .bind(mem_id)
    .bind(org.as_uuid())
    .bind(member_id)
    .bind(PublicId::new(IdKind::Membership, mem_id).as_str())
    .bind(role_id.map(|r| r.0))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let owner_mem: (Uuid, i64) = {
        let mut tx = pool.begin().await.unwrap();
        set_session_org_id(&mut tx, org).await.unwrap();
        let row = sqlx::query_as(
            "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org.as_uuid())
        .bind(owner_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        row
    };
    let member_mem: (Uuid, i64) = {
        let mut tx = pool.begin().await.unwrap();
        set_session_org_id(&mut tx, org).await.unwrap();
        let row = sqlx::query_as(
            "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org.as_uuid())
        .bind(member_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        row
    };

    let mut tx = pool.begin().await.unwrap();
    let owner_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        owner_id,
        &owner_public,
        org,
        owner_mem.0,
        &["owner".into()],
        owner_mem.1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let member_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &state.auth_keys.ring,
        member_id,
        &member_public,
        org,
        member_mem.0,
        &["member".into()],
        member_mem.1,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        org,
        owner_id,
        owner_public,
        member_id,
        member_public,
        owner_token: owner_issued.access_token,
        member_token: member_issued.access_token,
    })
}

#[tokio::test]
async fn dashboard_requires_auth() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(pool).await;
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard")
                .header("x-request-id", "dash-401")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn dashboard_forbidden_without_dashboard_perm() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;

    // All system role keys include dashboard.read by default. Attach an explicit
    // deny on the member's org_role so PDP denies (explicit deny wins).
    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let role_id: Uuid =
        sqlx::query_scalar("SELECT role_id FROM membership WHERE org_id = $1 AND user_id = $2")
            .bind(seeded.org.as_uuid())
            .bind(seeded.member_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO role_permission (id, org_id, role_id, permission_id, effect, scope)
        VALUES ($1, $2, $3, 'workspace.dashboard.read', 'deny', 'organization')
        ON CONFLICT (role_id, permission_id, effect) DO NOTHING
        "#,
    )
    .bind(new_uuid_v7())
    .bind(seeded.org.as_uuid())
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("x-request-id", "dash-403")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn dashboard_happy_path_widgets_and_reason_codes() {
    let Some(seeded) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let state = app_state(seeded.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard?period=30d")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "dash-ok")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();

    assert_eq!(body["period"], "30d");
    assert_eq!(body["role_layout"], "owner");
    assert!(body["as_of"].as_str().unwrap().contains('T'));

    let widgets = body["widgets"].as_array().unwrap();
    let ids: Vec<&str> = widgets.iter().filter_map(|w| w["id"].as_str()).collect();
    for expected in [
        "setup_checklist",
        "my_work",
        "tasks",
        "inbox",
        "approvals",
        "pipeline",
        "revenue",
        "expenses",
        "cash",
        "receivables",
        "team_activity",
    ] {
        assert!(ids.contains(&expected), "missing widget {expected}");
    }

    let by_id = |id: &str| widgets.iter().find(|w| w["id"] == id).unwrap();

    let setup = by_id("setup_checklist");
    assert_eq!(setup["status"], "ready");
    assert_eq!(setup["kind"], "checklist");
    assert!(setup["reason_code"].is_null());
    assert_eq!(setup["stale"], false);
    let member_count = setup["payload"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "members")
        .unwrap()["member_count"]
        .as_i64()
        .unwrap();
    assert!(member_count >= 2, "seeded org has owner+member");

    let pipeline = by_id("pipeline");
    // Phase 1.4+: dashboard tries CRM over the network. Without CRM running in
    // this suite we expect an honest unavailable + crm_unreachable.
    assert_eq!(pipeline["status"], "unavailable");
    assert_eq!(pipeline["reason_code"], "crm_unreachable");
    assert_eq!(pipeline["stale"], false);
    assert_eq!(pipeline["kind"], "module_empty");
    assert_eq!(pipeline["payload"]["module"], "sales");

    // Phase 1.5: finance widgets fetch companyos-finance. Without it running,
    // expect finance_unreachable (not the old module_not_enabled stub).
    for id in ["revenue", "expenses", "cash", "receivables"] {
        let w = by_id(id);
        assert_eq!(w["status"], "unavailable", "{id}");
        assert_eq!(w["reason_code"], "finance_unreachable", "{id}");
        assert_eq!(w["kind"], "stat", "{id}");
        assert_eq!(w["payload"]["module"], "finance", "{id}");
    }

    // Phase 1.6: my_work + tasks fetch project-service summary. Without it,
    // expect operations_unreachable.
    for id in ["my_work", "tasks"] {
        let w = by_id(id);
        assert_eq!(w["status"], "unavailable", "{id}");
        assert_eq!(w["reason_code"], "operations_unreachable", "{id}");
        assert_eq!(w["kind"], "stat", "{id}");
        assert_eq!(w["payload"]["module"], "operations", "{id}");
    }

    let approvals = by_id("approvals");
    assert_eq!(approvals["status"], "empty");
    assert_eq!(approvals["reason_code"], "coming_in_later_phase");

    let inbox = by_id("inbox");
    assert_eq!(inbox["reason_code"], "no_data");

    let feed = by_id("team_activity");
    assert_eq!(feed["kind"], "feed");
    assert_eq!(feed["status"], "empty");

    // Member layout
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard?period=7d")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("x-request-id", "dash-member")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["role_layout"], "member");
    assert_eq!(body["period"], "7d");
}

#[tokio::test]
async fn dashboard_tenant_isolation_member_count() {
    let Some(a) = seed_org_with_tokens().await else {
        eprintln!("skipping: no database");
        return;
    };
    let Some(b) = seed_org_with_tokens().await else {
        return;
    };
    let state = app_state(a.pool.clone()).await;
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard")
                .header("authorization", format!("Bearer {}", a.owner_token))
                .header("x-request-id", "dash-iso")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let setup = body["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == "setup_checklist")
        .unwrap();
    let count = setup["payload"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "members")
        .unwrap()["member_count"]
        .as_i64()
        .unwrap();
    // Org A has exactly owner + member from seed; must not include org B members.
    assert_eq!(count, 2);
    let _ = b;
}
