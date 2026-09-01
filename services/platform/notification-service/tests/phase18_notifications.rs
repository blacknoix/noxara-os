//! Phase 1.8 notification integration tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_notification::state::AppState as NotifAppState;
use companyos_tenancy::{set_session_org_id, Actor, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    companyos_core::migrate(&pool).await.ok()?;
    companyos_notification::migrate(&pool).await.ok()?;
    Some(pool)
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn notif_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_notification::build_router(NotifAppState::new(pool, ring))
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

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    sales_user_id: Uuid,
    member_user_id: Uuid,
    owner_id: Uuid,
    sales_token: String,
    member_token: String,
}

async fn seed(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
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
                .header("x-request-id", "notif-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Notif Phase18 Test Co"
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

    let sales_user_id = insert_member_with_role(&pool, org, "sales", "Sales Rep").await;
    // Finance lacks sales.customer.read — used for the authz-deny notification test.
    let member_user_id = insert_member_with_role(&pool, org, "finance", "Finance User").await;

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;
    let (sales_mem_id, sales_policy) = membership_role_and_policy(&pool, org, sales_user_id).await;
    let (member_mem_id, member_policy) =
        membership_role_and_policy(&pool, org, member_user_id).await;

    let mut tx = pool.begin().await.unwrap();
    let owner_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        owner_id,
        &PublicId::new(IdKind::User, owner_id).as_str(),
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
    let sales_issued = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        sales_user_id,
        &PublicId::new(IdKind::User, sales_user_id).as_str(),
        org,
        sales_mem_id,
        &["sales".into()],
        sales_policy,
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
        &["finance".into()],
        member_policy,
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
        sales_user_id,
        member_user_id,
        owner_id,
        sales_token: sales_issued.access_token,
        member_token: member_issued.access_token,
    })
}

fn customer_created(org: OrgId, actor: Uuid) -> EventEnvelope {
    EventEnvelope::new(
        org,
        Context::Sales,
        "customer",
        "created",
        1,
        Actor::human(actor),
        json!({
            "customer_id": format!("cus_{}", new_uuid_v7().simple()),
            "summary": "Acme Corp created",
            "href": "/sales/customers/1"
        }),
    )
}

#[tokio::test]
async fn unauthorized_user_does_not_get_customer_created() {
    let Some(seeded) = seed("notif-authz-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = notif_app(seeded.pool.clone(), seeded.ring.clone());

    let mut env = customer_created(seeded.org, seeded.owner_id);
    env.idempotency_key = format!("idem-authz-{}", new_uuid_v7());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&env).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Sales has sales.customer.read → should see feed item.
    let sales_feed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/notifications/feed")
                .header("authorization", format!("Bearer {}", seeded.sales_token))
                .header("x-request-id", "feed-sales")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sales_feed.status(), StatusCode::OK);
    let sales_body: Value =
        serde_json::from_slice(&sales_feed.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert!(
        !sales_body["items"].as_array().unwrap().is_empty(),
        "sales user should be notified"
    );

    // Member role lacks sales.customer.read → empty feed.
    let member_feed = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/notifications/feed")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .header("x-request-id", "feed-member")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(member_feed.status(), StatusCode::OK);
    let member_body: Value =
        serde_json::from_slice(&member_feed.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert!(
        member_body["items"].as_array().unwrap().is_empty(),
        "member without sales.customer.read must not be notified"
    );
    let _ = seeded.sales_user_id;
    let _ = seeded.member_user_id;
}

#[tokio::test]
async fn tenant_isolation_on_feed() {
    let Some(a) = seed("notif-tenant-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed("notif-tenant-b").await else {
        return;
    };

    let app_a = notif_app(a.pool.clone(), a.ring.clone());
    let mut env = customer_created(a.org, a.owner_id);
    env.idempotency_key = format!("idem-tenant-{}", new_uuid_v7());
    let _ = app_a
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&env).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Org B token must not see org A's notifications.
    let app_b = notif_app(b.pool.clone(), b.ring.clone());
    let feed_b = app_b
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/notifications/feed")
                .header("authorization", format!("Bearer {}", b.owner_token))
                .header("x-request-id", "feed-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(feed_b.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&feed_b.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn idempotent_ingest_duplicate_delivery() {
    let Some(seeded) = seed("notif-idem-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = notif_app(seeded.pool.clone(), seeded.ring.clone());

    let mut env = customer_created(seeded.org, seeded.owner_id);
    env.idempotency_key = format!("idem-dup-{}", new_uuid_v7());

    let body = serde_json::to_vec(&env).unwrap();
    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::OK);
    let b1: Value =
        serde_json::from_slice(&res1.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b1["duplicate"], false);

    let res2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let b2: Value =
        serde_json::from_slice(&res2.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b2["duplicate"], true);
    assert_eq!(b2["notified"], 0);
}

#[tokio::test]
async fn push_device_register_upsert_no_duplicate() {
    let Some(seeded) = seed("notif-device-secret").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = notif_app(seeded.pool.clone(), seeded.ring.clone());

    let body = serde_json::to_vec(&json!({
        "platform": "fake",
        "push_token": "ci-fake-token-1",
        "device_label": "phase111-test"
    }))
    .unwrap();

    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/devices")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::CREATED);
    let b1: Value =
        serde_json::from_slice(&res1.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let id1 = b1["id"].as_str().unwrap().to_string();

    let res2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notifications/devices")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let b2: Value =
        serde_json::from_slice(&res2.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b2["id"].as_str().unwrap(), id1);

    let list = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/notifications/devices")
                .header("authorization", format!("Bearer {}", seeded.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let lb: Value =
        serde_json::from_slice(&list.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(lb["items"].as_array().unwrap().len(), 1);
}
