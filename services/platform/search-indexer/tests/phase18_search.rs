//! Phase 1.8 search indexer tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_events::{Context, EventEnvelope};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_search::state::AppState as SearchAppState;
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
    companyos_search::migrate(&pool).await.ok()?;
    Some(pool)
}

fn search_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_search::build_router(SearchAppState::new(pool, ring))
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
    finance_token: String,
}

async fn seed(secret: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    let core = companyos_core::build_router(companyos_core::state::AppState::new(
        pool.clone(),
        ring.clone(),
    ));

    let owner_email = format!("owner-{}@test.local", new_uuid_v7());
    let res = core
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Search Phase18"
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
    let org = OrgId::from_public(&body["org_id"].as_str().unwrap().parse().unwrap()).unwrap();
    let owner_id = body["user_id"]
        .as_str()
        .unwrap()
        .parse::<PublicId>()
        .unwrap()
        .uuid();

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

    let finance_id = insert_member_with_role(&pool, org, "finance", "Finance").await;

    let mut tx = pool.begin().await.unwrap();
    let (owner_mem, owner_pol): (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(owner_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let (fin_mem, fin_pol): (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(finance_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();

    let owner_tok = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        owner_id,
        &PublicId::new(IdKind::User, owner_id).as_str(),
        org,
        owner_mem,
        &["owner".into()],
        owner_pol,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let fin_tok = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        &ring,
        finance_id,
        &PublicId::new(IdKind::User, finance_id).as_str(),
        org,
        fin_mem,
        &["finance".into()],
        fin_pol,
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
        owner_token: owner_tok.access_token,
        finance_token: fin_tok.access_token,
    })
}

#[tokio::test]
async fn query_without_org_id_fails() {
    let Some(seeded) = seed("search-orgid").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = search_app(seeded.pool, seeded.ring);
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/search/query?q=acme")
                .header("authorization", format!("Bearer {}", seeded.owner_token))
                .header("x-request-id", "q1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn results_rechecked_and_tenant_isolation() {
    let Some(a) = seed("search-authz-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed("search-authz-b").await else {
        return;
    };

    let app_a = search_app(a.pool.clone(), a.ring.clone());
    let cus_id = format!("cus_{}", new_uuid_v7().simple());
    let env = EventEnvelope::new(
        a.org,
        Context::Sales,
        "customer",
        "created",
        1,
        Actor::human(new_uuid_v7()),
        json!({
            "customer_id": cus_id,
            "name": "Acme Visible",
            "summary": "customer doc"
        }),
    );
    let _ = app_a
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/search/internal/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&env).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Finance lacks sales.customer.read → hit filtered out.
    let fin_q = app_a
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/search/query?q=Acme&org_id={}",
                    a.org.to_public()
                ))
                .header("authorization", format!("Bearer {}", a.finance_token))
                .header("x-request-id", "q-fin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fin_q.status(), StatusCode::OK);
    let fin_body: Value =
        serde_json::from_slice(&fin_q.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(fin_body["hits"].as_array().unwrap().is_empty());

    // Owner sees it.
    let own_q = app_a
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/search/query?q=Acme&org_id={}",
                    a.org.to_public()
                ))
                .header("authorization", format!("Bearer {}", a.owner_token))
                .header("x-request-id", "q-own")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(own_q.status(), StatusCode::OK);
    let own_body: Value =
        serde_json::from_slice(&own_q.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(own_body["hits"].as_array().unwrap().len(), 1);

    // Org B cannot query org A (forbidden).
    let app_b = search_app(b.pool, b.ring);
    let cross = app_b
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/search/query?q=Acme&org_id={}",
                    a.org.to_public()
                ))
                .header("authorization", format!("Bearer {}", b.owner_token))
                .header("x-request-id", "q-cross")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross.status(), StatusCode::FORBIDDEN);
}
