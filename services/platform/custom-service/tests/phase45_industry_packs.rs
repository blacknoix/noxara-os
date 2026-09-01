//! Phase 4.5 — Industry packs + zero core branching + record conflict.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_custom::state::AppState;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
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
    companyos_outbox::migrate(&pool).await.ok()?;
    companyos_custom::migrate(&pool).await.ok()?;
    let _ = companyos_integration::migrate(&pool).await;
    Some(pool)
}

fn custom_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_custom::build_router(AppState::new(pool, ring))
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

async fn session_token(
    pool: &PgPool,
    ring: &KeyRing,
    org: OrgId,
    user_id: Uuid,
    role: &str,
) -> String {
    let mut tx = pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org).await.unwrap();
    let (mem_id, pol): (Uuid, i64) = sqlx::query_as(
        "SELECT id, policy_version FROM membership WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org.as_uuid())
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let tok = companyos_core::auth::sessions::create_session_with_tokens(
        &mut tx,
        ring,
        user_id,
        &PublicId::new(IdKind::User, user_id).as_str(),
        org,
        mem_id,
        &[role.into()],
        pol,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    tok.access_token
}

struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_id: Uuid,
    member_id: Uuid,
    owner_token: String,
    member_token: String,
}

impl Seeded {
    async fn refresh_tokens(&mut self) {
        self.owner_token =
            session_token(&self.pool, &self.ring, self.org, self.owner_id, "owner").await;
        self.member_token =
            session_token(&self.pool, &self.ring, self.org, self.member_id, "member").await;
    }
}

async fn seed(secret: &str, org_name: &str) -> Option<Seeded> {
    let pool = pool().await?;
    let ring = KeyRing::from_secret(secret);
    companyos_core::auth::ensure_bootstrap_key(&pool, &ring)
        .await
        .ok()?;
    companyos_core::workspace::sync_permission_catalogue(&pool)
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
                        "org_name": org_name
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

    let _ = companyos_integration::marketplace::seed::seed_first_party_catalogue(&pool, org).await;

    let member_id = insert_member_with_role(&pool, org, "member", "Member").await;
    let owner_token = session_token(&pool, &ring, org, owner_id, "owner").await;
    let member_token = session_token(&pool, &ring, org, member_id, "member").await;

    Some(Seeded {
        pool,
        ring,
        org,
        owner_id,
        member_id,
        owner_token,
        member_token,
    })
}

async fn json_req(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    let req = if let Some(b) = body {
        builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, val)
}

async fn json_req_headers(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let req = if let Some(b) = body {
        builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, val)
}

#[tokio::test]
async fn industry_packs_install_without_core_branching() {
    let Some(mut org_a) = seed("phase45-shared-secret-key!!", "Pack A Org").await else {
        eprintln!("skipping: no DATABASE_URL");
        return;
    };
    // Second org on the same key ring / DB so one router validates both tokens.
    let pool = org_a.pool.clone();
    let ring = org_a.ring.clone();
    companyos_core::workspace::sync_permission_catalogue(&pool)
        .await
        .ok();
    let core = companyos_core::build_router(companyos_core::state::AppState::new(
        pool.clone(),
        ring.clone(),
    ));
    let owner_email = format!("owner-b-{}@test.local", new_uuid_v7());
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
                        "display_name": "Owner B",
                        "org_name": "Pack B Org"
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
    let org_b_id = OrgId::from_public(&body["org_id"].as_str().unwrap().parse().unwrap()).unwrap();
    let owner_b = body["user_id"]
        .as_str()
        .unwrap()
        .parse::<PublicId>()
        .unwrap()
        .uuid();
    sqlx::query(
        "UPDATE user_identity SET email_verified_at = now(), mfa_enabled_at = now(), mfa_totp_secret_encrypted = $2 WHERE id = $1",
    )
    .bind(owner_b)
    .bind(companyos_core::auth::mfa::generate_totp_secret())
    .execute(&pool)
    .await
    .unwrap();
    companyos_core::workspace::provisioning::process_pending(&pool, org_b_id, "test")
        .await
        .ok();
    let _ =
        companyos_integration::marketplace::seed::seed_first_party_catalogue(&pool, org_b_id).await;
    let member_b = insert_member_with_role(&pool, org_b_id, "member", "Member B").await;
    let mut org_b = Seeded {
        pool: pool.clone(),
        ring: ring.clone(),
        org: org_b_id,
        owner_id: owner_b,
        member_id: member_b,
        owner_token: session_token(&pool, &ring, org_b_id, owner_b, "owner").await,
        member_token: session_token(&pool, &ring, org_b_id, member_b, "member").await,
    };

    let router = custom_app(org_a.pool.clone(), org_a.ring.clone());

    let (st, cat) = json_req(
        &router,
        "GET",
        "/api/v1/custom/industry-packs",
        &org_a.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cat}");
    assert_eq!(cat["items"].as_array().unwrap().len(), 4);

    let (st, _) = json_req(
        &router,
        "POST",
        "/api/v1/custom/industry-packs/professional-services/install",
        &org_a.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, inst_a) = json_req(
        &router,
        "POST",
        "/api/v1/custom/industry-packs/professional-services/install",
        &org_a.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{inst_a}");
    assert!(inst_a["package"]["entities_imported"].as_u64().unwrap() >= 2);
    assert_eq!(
        inst_a["marketplace_connector_key"],
        "industry.professional-services"
    );
    org_a.refresh_tokens().await;

    let (st, inst_b) = json_req(
        &router,
        "POST",
        "/api/v1/custom/industry-packs/retail/install",
        &org_b.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{inst_b}");
    org_b.refresh_tokens().await;

    let (st, eng) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/engagement",
        &org_a.owner_token,
        Some(json!({
            "values": {
                "title": "Acme retainers",
                "status": "active",
                "retainer_amount": { "amount_minor": 150000, "currency": "USD" }
            }
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{eng}");

    let (st, sku) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/product_sku",
        &org_b.owner_token,
        Some(json!({
            "values": {
                "sku": "TEE-001",
                "name": "Logo Tee",
                "list_price": { "amount_minor": 2500, "currency": "USD" },
                "active": true,
                "category": "apparel"
            }
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{sku}");

    let mut tx = org_a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, org_a.org).await.unwrap();
    let stages_a: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM workspace_seed_pipeline_stage WHERE org_id = $1 ORDER BY position",
    )
    .bind(org_a.org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        stages_a.iter().any(|(n,)| n == "Engaged"),
        "professional-services seed missing: {stages_a:?}"
    );

    let (st, un) = json_req(
        &router,
        "POST",
        "/api/v1/custom/industry-packs/professional-services/uninstall",
        &org_a.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{un}");
    let (st, still) = json_req(
        &router,
        "GET",
        "/api/v1/custom/records/engagement",
        &org_a.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{still}");
    assert_eq!(still["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn custom_record_conflict_is_user_visible_and_deterministic() {
    let Some(mut org) = seed("phase45-conflict-secret-key!", "Conflict Org").await else {
        eprintln!("skipping: no DATABASE_URL");
        return;
    };
    let router = custom_app(org.pool.clone(), org.ring.clone());

    let (st, _) = json_req(
        &router,
        "POST",
        "/api/v1/custom/industry-packs/professional-services/install",
        &org.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    org.refresh_tokens().await;

    let (st, rec) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/engagement",
        &org.owner_token,
        Some(json!({ "values": { "title": "v1", "status": "draft" } })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{rec}");
    let id = rec["id"].as_str().unwrap();
    assert_eq!(rec["version"], 1);

    let (st, win) = json_req_headers(
        &router,
        "PATCH",
        &format!("/api/v1/custom/records/engagement/{id}"),
        &org.owner_token,
        &[("if-match", "1")],
        Some(json!({ "values": { "title": "winner" } })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{win}");
    assert_eq!(win["values"]["title"], "winner");
    assert_eq!(win["version"], 2);

    let (st, lose) = json_req_headers(
        &router,
        "PATCH",
        &format!("/api/v1/custom/records/engagement/{id}"),
        &org.owner_token,
        &[("if-match", "1")],
        Some(json!({ "values": { "title": "loser-silent?" } })),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{lose}");
    assert!(
        lose.to_string().contains("version mismatch"),
        "conflict must be explicit: {lose}"
    );

    let (st, got) = json_req(
        &router,
        "GET",
        &format!("/api/v1/custom/records/engagement/{id}"),
        &org.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(got["values"]["title"], "winner");
    assert_eq!(got["version"], 2);
}

#[test]
fn business_services_have_no_industry_pack_branches() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../business");
    assert!(root.is_dir(), "expected {}", root.display());
    let mut offenders = Vec::new();
    for path in walkdir_rs(&root) {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        for (idx, line) in src.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            let hit = (lower.contains("match ") && lower.contains("industry"))
                || lower.contains("if industry ==")
                || lower.contains("if pack ==")
                || (lower.contains("match pack") && lower.contains('{'));
            if hit && !line.trim_start().starts_with("//") {
                offenders.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "industry/pack branching in business services (packs must be data-only):\n{}",
        offenders.join("\n")
    );
}

fn walkdir_rs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
    }
    walk(dir, &mut out);
    out
}
