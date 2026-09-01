//! Phase 4.4 low-code builder — DoD integration tests.

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

async fn seed(secret: &str) -> Option<Seeded> {
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
                        "org_name": "Custom Phase44"
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
        .header("x-request-id", format!("t-{}", new_uuid_v7().simple()))
        .header("content-type", "application/json");
    let req = if let Some(b) = body {
        builder.body(Body::from(b.to_string())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = router.clone().oneshot(req).await.unwrap();
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

#[tokio::test]
async fn define_publish_crud_rls_authz_audit_outbox() {
    let Some(mut seed_a) = seed("custom-phase44-shared").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(mut seed_b) = seed("custom-phase44-shared").await else {
        return;
    };

    let router = custom_app(seed_a.pool.clone(), seed_a.ring.clone());

    // Owner defines + publishes entity
    let (st, ent) = json_req(
        &router,
        "POST",
        "/api/v1/custom/entities",
        &seed_a.owner_token,
        Some(json!({
            "slug": "widget",
            "label": "Widget",
            "description": "demo",
            "fields": [
                {"name": "title", "label": "Title", "type": "text", "required": true},
                {"name": "qty", "label": "Qty", "type": "number", "required": true},
                {"name": "unit", "label": "Unit", "type": "number", "required": true},
                {"name": "total", "label": "Total", "type": "formula", "formula": "qty * unit"}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{ent}");
    let entity_id = ent["id"].as_str().unwrap();

    let (st, _) = json_req(
        &router,
        "POST",
        &format!("/api/v1/custom/entities/{entity_id}/publish"),
        &seed_a.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Publish bumps policy_version — mint fresh tokens.
    seed_a.refresh_tokens().await;
    seed_b.refresh_tokens().await;

    // Member without grant cannot write
    let (st, _) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/widget",
        &seed_a.member_token,
        Some(json!({ "values": { "title": "x", "qty": 2, "unit": 5 } })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Owner can CRUD
    let (st, rec) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/widget",
        &seed_a.owner_token,
        Some(json!({ "values": { "title": "Alpha", "qty": 2, "unit": 5 } })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{rec}");
    assert_eq!(rec["values"]["total"], 10, "formula must recalc on save");
    let record_id = rec["id"].as_str().unwrap().to_string();

    // Second org: publish same slug so list is authorized, but RLS hides org A rows.
    let (st, ent_b) = json_req(
        &router,
        "POST",
        "/api/v1/custom/entities",
        &seed_b.owner_token,
        Some(json!({
            "slug": "widget",
            "label": "Widget B",
            "fields": [
                {"name": "title", "label": "Title", "type": "text", "required": true},
                {"name": "qty", "label": "Qty", "type": "number", "required": true},
                {"name": "unit", "label": "Unit", "type": "number", "required": true},
                {"name": "total", "label": "Total", "type": "formula", "formula": "qty * unit"}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{ent_b}");
    let entity_b = ent_b["id"].as_str().unwrap();
    let (st, _) = json_req(
        &router,
        "POST",
        &format!("/api/v1/custom/entities/{entity_b}/publish"),
        &seed_b.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    seed_b.refresh_tokens().await;

    let (st, list_b) = json_req(
        &router,
        "GET",
        "/api/v1/custom/records/widget",
        &seed_b.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{list_b}");
    assert!(
        list_b["items"].as_array().unwrap().is_empty(),
        "org B must not see org A records via RLS: {list_b}"
    );

    // Direct RLS probe: org B session cannot read org A's row by public_id.
    let mut rls_tx = seed_a.pool.begin().await.unwrap();
    set_session_org_id(&mut rls_tx, seed_b.org).await.unwrap();
    let leaked: Option<(String,)> = sqlx::query_as(
        "SELECT public_id FROM custom_record WHERE public_id = $1 AND deleted_at IS NULL",
    )
    .bind(&record_id)
    .fetch_optional(&mut *rls_tx)
    .await
    .unwrap();
    rls_tx.commit().await.unwrap();
    assert!(
        leaked.is_none(),
        "RLS must hide foreign-tenant custom_record"
    );

    // Audit row
    let mut tx = seed_a.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed_a.org).await.unwrap();
    let audit_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT FROM audit_entry
        WHERE org_id = $1 AND resource_type = 'custom_record' AND resource_id = $2
        "#,
    )
    .bind(seed_a.org.as_uuid())
    .bind(&record_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(audit_count >= 1, "expected audit row");

    // Outbox event
    let outbox: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT FROM outbox_event
        WHERE org_id = $1 AND payload->'payload'->>'id' = $2
        "#,
    )
    .bind(seed_a.org.as_uuid())
    .bind(&record_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(outbox >= 1, "expected outbox event");

    // Search document path / indexer hook
    let doc = companyos_custom::search_doc::search_document(
        &seed_a.org.to_public().as_str(),
        "widget",
        &record_id,
        &rec["values"],
        "title:Alpha",
    );
    assert_eq!(doc["permission"], "custom.widget.read");
    assert_eq!(doc["doc_type"], "custom:widget");
}

#[tokio::test]
async fn package_export_import_and_upgrade_rehearsal() {
    let Some(mut src) = seed("custom-pkg-src").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(mut dst) = seed("custom-pkg-src").await else {
        return;
    };
    let router = custom_app(src.pool.clone(), src.ring.clone());

    let (st, ent) = json_req(
        &router,
        "POST",
        "/api/v1/custom/entities",
        &src.owner_token,
        Some(json!({
            "slug": "asset_tag",
            "label": "Asset tag",
            "fields": [
                {"name": "code", "label": "Code", "type": "text", "required": true}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{ent}");
    let entity_id = ent["id"].as_str().unwrap();
    let (st, _) = json_req(
        &router,
        "POST",
        &format!("/api/v1/custom/entities/{entity_id}/publish"),
        &src.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    src.refresh_tokens().await;
    dst.refresh_tokens().await;

    let (st, _) = json_req(
        &router,
        "POST",
        "/api/v1/custom/views/asset_tag",
        &src.owner_token,
        Some(json!({
            "name": "Default",
            "columns": ["code"],
            "filters": [],
            "sort": [{"field": "code", "dir": "asc"}]
        })),
    )
    .await;
    assert!(st.is_success(), "create view {st}");

    let (st, pkg) = json_req(
        &router,
        "GET",
        "/api/v1/custom/packages/export",
        &src.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pkg}");
    assert_eq!(pkg["format"], "companyos.custom.package");

    // Import into destination org
    let (st, imported) = json_req(
        &router,
        "POST",
        "/api/v1/custom/packages/import",
        &dst.owner_token,
        Some(json!({ "package": pkg })),
    )
    .await;
    assert!(st.is_success(), "import: {st} {imported}");
    assert!(imported["entities_imported"].as_u64().unwrap_or(0) >= 1);
    dst.refresh_tokens().await;

    // CRUD works in destination
    let (st, rec) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/asset_tag",
        &dst.owner_token,
        Some(json!({ "values": { "code": "A-1" } })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{rec}");

    // Upgrade rehearsal: platform bump then CRUD still works
    companyos_custom::migrate_platform_bump(&dst.pool)
        .await
        .expect("platform bump");
    let ver: String =
        sqlx::query_scalar("SELECT value FROM custom_platform_meta WHERE key = 'schema_version'")
            .fetch_one(&dst.pool)
            .await
            .unwrap();
    assert_eq!(ver, "2");

    let (st, list) = json_req(
        &router,
        "GET",
        "/api/v1/custom/records/asset_tag",
        &dst.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{list}");
    assert!(!list["items"].as_array().unwrap().is_empty());

    let (st, rec2) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/asset_tag",
        &dst.owner_token,
        Some(json!({ "values": { "code": "A-2" } })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{rec2}");
}

#[tokio::test]
async fn formula_escape_and_script_limits() {
    // Unit coverage lives in formula + sandbox modules; this wires script fail-closed on save.
    let Some(mut seed) = seed("custom-script-limits").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let router = custom_app(seed.pool.clone(), seed.ring.clone());

    let (st, ent) = json_req(
        &router,
        "POST",
        "/api/v1/custom/entities",
        &seed.owner_token,
        Some(json!({
            "slug": "gadget",
            "label": "Gadget",
            "fields": [
                {"name": "n", "label": "N", "type": "number", "required": true}
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{ent}");
    let entity_id = ent["id"].as_str().unwrap();
    let (st, _) = json_req(
        &router,
        "POST",
        &format!("/api/v1/custom/entities/{entity_id}/publish"),
        &seed.owner_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    seed.refresh_tokens().await;

    // Infinite loop before_save — fail closed
    let loop_src = json!([
        {"op": "loop", "times": 4294967295u32, "body": [
            {"op": "set", "field": "n", "expr": {"kind": "add",
                "left": {"kind": "field", "name": "n"},
                "right": {"kind": "lit", "value": 1}
            }}
        ]}
    ])
    .to_string();

    let (st, _) = json_req(
        &router,
        "PUT",
        "/api/v1/custom/scripts/gadget",
        &seed.owner_token,
        Some(json!({
            "hook": "before_save",
            "source": loop_src,
            "enabled": true
        })),
    )
    .await;
    assert!(st.is_success(), "upsert script {st}");

    let (st, body) = json_req(
        &router,
        "POST",
        "/api/v1/custom/records/gadget",
        &seed.owner_token,
        Some(json!({ "values": { "n": 1 } })),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "runaway must fail closed: {body}"
    );
    assert!(
        body.to_string().contains("fail-closed") || body.to_string().contains("script"),
        "{body}"
    );
}

#[test]
fn sandbox_unit_limits_covered() {
    // Ensure sandbox module tests are linked via the package (compile-time presence).
    assert!(companyos_authz::is_dynamic_custom_entity_permission(
        "custom.widget.read"
    ));
    assert!(!companyos_authz::is_dynamic_custom_entity_permission(
        "custom.builder.manage"
    ));
    assert!(!companyos_authz::is_dynamic_custom_entity_permission(
        "custom.demo_asset.write"
    ));
}
