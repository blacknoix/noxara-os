//! Phase 2.5 Inventory & Procurement integration tests (DoD).
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).
//!
//! Pattern mirrors `hr-service/tests/hr_phase23.rs`: register an owner via
//! core's HTTP API (so `OrgProvisioning` seeds system roles +
//! `role_permission`), mint an access token with
//! `companyos_core::auth::sessions::create_session_with_tokens` on a shared
//! `KeyRing`, migrate core / inventory / finance / project, and drive
//! `companyos_inventory::build_router` (+ finance / project routers) with
//! `tower::ServiceExt::oneshot`. Finance and project are additionally
//! spawned as real HTTP servers on random ports (`FINANCE_SERVICE_URL` /
//! `PROJECT_SERVICE_URL`) so inventory-service's own `finance_client` /
//! `approvals_client` HTTP calls (goods-receipt journals, vendor bills,
//! purchase-request approval routing) have somewhere to land.
//!
//! Purchase-request approval: `submit_purchase_request` best-effort-calls
//! `companyos-project`'s approvals API, but the resulting Temporal workflow
//! is not driven in this test environment. Tests use inventory's own
//! `/purchase-requests/{id}/decide` callback endpoint directly to finalize
//! the approve/reject decision, exactly as project-service would after a
//! human decides (see `purchase_requests::decide_purchase_request`).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::{Datelike, Utc};
use companyos_auth_token::KeyRing;
use companyos_finance::state::AppState as FinanceAppState;
use companyos_ids::{new_uuid_v7, PublicId};
use companyos_inventory::state::AppState as InventoryAppState;
use companyos_project::state::AppState as ProjectAppState;
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tower::ServiceExt;
use uuid::Uuid;

/// Migrate once — concurrent DDL from every test is racy under FORCE RLS.
static MIGRATED: OnceLock<()> = OnceLock::new();
static SEED_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
static FINANCE_SPAWN_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
static PROJECT_SPAWN_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

// ---------------------------------------------------------------------------
// Setup / seeding plumbing
// ---------------------------------------------------------------------------

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
        companyos_inventory::migrate(pool).await.ok()?;
        companyos_finance::migrate(pool).await.ok()?;
        companyos_project::migrate(pool).await.ok()?;
        let _ = MIGRATED.set(());
    }
    Some(())
}

fn core_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_core::build_router(companyos_core::state::AppState::new(pool, ring))
}

fn inventory_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_inventory::build_router(InventoryAppState::new(pool, ring))
}

fn finance_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_finance::build_router(FinanceAppState::new(pool, ring))
}

fn project_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_project::build_router(ProjectAppState::new(pool, ring))
}

/// One org with an owner token — the "owner" role carries every permission
/// (see `companyos_authz::Role::permissions`), so a single token covers
/// every endpoint under test without needing per-role fixtures.
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_token: String,
    #[allow(dead_code)]
    owner_id: Uuid,
}

/// A finance-service / project-service instance spawned on a random local
/// port for inventory-service's outbound HTTP calls to land on. Aborted on
/// drop.
struct SpawnedServer {
    #[allow(dead_code)]
    url: String,
    handle: JoinHandle<()>,
}

impl Drop for SpawnedServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn_finance_server(pool: PgPool, ring: KeyRing) -> SpawnedServer {
    let _guard = FINANCE_SPAWN_LOCK.lock().await;
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind finance");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");
    std::env::set_var("FINANCE_SERVICE_URL", &url);
    let app = finance_app(pool, ring);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("finance test server error: {e}");
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    SpawnedServer { url, handle }
}

async fn spawn_project_server(pool: PgPool, ring: KeyRing) -> SpawnedServer {
    let _guard = PROJECT_SPAWN_LOCK.lock().await;
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind project");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");
    std::env::set_var("PROJECT_SERVICE_URL", &url);
    let app = project_app(pool, ring);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("project test server error: {e}");
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    SpawnedServer { url, handle }
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
                .header("x-request-id", "inv25-reg")
                .body(Body::from(
                    json!({
                        "email": owner_email,
                        "password": "correct-horse-battery-staple",
                        "display_name": "Owner",
                        "org_name": "Inventory Phase25 Test Co"
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

    let (owner_mem_id, owner_policy) = membership_role_and_policy(&pool, org, owner_id).await;

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
    tx.commit().await.unwrap();

    Some(Seeded {
        pool,
        ring,
        org,
        owner_token: owner_issued.access_token,
        owner_id,
    })
}

// ---------------------------------------------------------------------------
// HTTP call helper
// ---------------------------------------------------------------------------

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
        .header("x-request-id", format!("inv25-{}", new_uuid_v7()));
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

fn public_id_uuid(s: &str) -> Uuid {
    s.parse::<PublicId>().unwrap().uuid()
}

// ---------------------------------------------------------------------------
// Domain helpers — inventory
// ---------------------------------------------------------------------------

async fn create_warehouse(app: &Router, token: &str, code: &str, name: &str) -> Value {
    let (st, wh) = call(
        app,
        "POST",
        "/api/v1/inventory/warehouses",
        token,
        Some(json!({ "code": code, "name": name })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{wh}");
    wh
}

async fn create_item(
    app: &Router,
    token: &str,
    sku: &str,
    name: &str,
    currency: &str,
    allow_negative_stock: bool,
) -> Value {
    let (st, item) = call(
        app,
        "POST",
        "/api/v1/inventory/items",
        token,
        Some(json!({
            "sku": sku,
            "name": name,
            "currency": currency,
            "allow_negative_stock": allow_negative_stock,
        })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{item}");
    item
}

async fn create_supplier(app: &Router, token: &str, name: &str, currency: &str) -> Value {
    let (st, sup) = call(
        app,
        "POST",
        "/api/v1/inventory/suppliers",
        token,
        Some(json!({ "name": name, "currency": currency })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{sup}");
    sup
}

async fn post_movement(
    app: &Router,
    token: &str,
    warehouse_id: &str,
    item_id: &str,
    qty_delta: i64,
    unit_cost_minor: i64,
    movement_type: &str,
) -> (StatusCode, Value) {
    let idem = format!("mv-{}", new_uuid_v7());
    call(
        app,
        "POST",
        "/api/v1/inventory/movements",
        token,
        Some(json!({
            "warehouse_id": warehouse_id,
            "item_id": item_id,
            "qty_delta": qty_delta,
            "unit_cost_minor": unit_cost_minor,
            "movement_type": movement_type,
        })),
        &[("idempotency-key", idem.as_str())],
    )
    .await
}

async fn reconcile(
    app: &Router,
    token: &str,
    warehouse_id: Option<&str>,
    item_id: Option<&str>,
) -> Value {
    let mut body = json!({});
    if let Some(w) = warehouse_id {
        body["warehouse_id"] = json!(w);
    }
    if let Some(i) = item_id {
        body["item_id"] = json!(i);
    }
    let (st, res) = call(
        app,
        "POST",
        "/api/v1/inventory/stock/reconcile",
        token,
        Some(body),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{res}");
    res
}

async fn item_stock_level(app: &Router, token: &str, item_id: &str, warehouse_id: &str) -> Value {
    let (st, res) = call(
        app,
        "GET",
        &format!("/api/v1/inventory/items/{item_id}/stock"),
        token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{res}");
    res["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["warehouse_id"] == warehouse_id)
        .cloned()
        .unwrap_or_else(|| json!({ "qty_on_hand": 0, "avg_unit_cost_minor": 0 }))
}

async fn create_purchase_request(
    app: &Router,
    token: &str,
    currency: &str,
    lines: &[(&str, i64, i64)],
) -> Value {
    let line_json: Vec<Value> = lines
        .iter()
        .map(|(item_id, qty, unit_cost)| {
            json!({ "item_id": item_id, "qty": qty, "unit_cost_estimate_minor": unit_cost })
        })
        .collect();
    let (st, pr) = call(
        app,
        "POST",
        "/api/v1/inventory/purchase-requests",
        token,
        Some(json!({ "currency": currency, "lines": line_json })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{pr}");
    pr
}

async fn submit_purchase_request(app: &Router, token: &str, pr_id: &str) -> Value {
    let (st, pr) = call(
        app,
        "POST",
        &format!("/api/v1/inventory/purchase-requests/{pr_id}/submit"),
        token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pr}");
    pr
}

async fn decide_purchase_request(app: &Router, token: &str, pr_id: &str, approve: bool) -> Value {
    let (st, pr) = call(
        app,
        "POST",
        &format!("/api/v1/inventory/purchase-requests/{pr_id}/decide"),
        token,
        Some(json!({ "approve": approve })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pr}");
    pr
}

async fn create_purchase_order(
    app: &Router,
    token: &str,
    supplier_id: &str,
    currency: &str,
    lines: &[(&str, &str, i64, i64)],
    purchase_request_id: Option<&str>,
) -> Value {
    let line_json: Vec<Value> = lines
        .iter()
        .map(|(item_id, warehouse_id, qty, cost)| {
            json!({
                "item_id": item_id,
                "warehouse_id": warehouse_id,
                "qty_ordered": qty,
                "unit_cost_minor": cost,
            })
        })
        .collect();
    let mut body = json!({ "supplier_id": supplier_id, "currency": currency, "lines": line_json });
    if let Some(pr) = purchase_request_id {
        body["purchase_request_id"] = json!(pr);
    }
    let (st, po) = call(
        app,
        "POST",
        "/api/v1/inventory/purchase-orders",
        token,
        Some(body),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{po}");
    po
}

async fn issue_purchase_order(app: &Router, token: &str, po_id: &str) -> Value {
    let idem = format!("issue-{}", new_uuid_v7());
    let (st, po) = call(
        app,
        "POST",
        &format!("/api/v1/inventory/purchase-orders/{po_id}/issue"),
        token,
        None,
        &[("idempotency-key", idem.as_str())],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{po}");
    po
}

async fn get_purchase_order(app: &Router, token: &str, po_id: &str) -> Value {
    let (st, po) = call(
        app,
        "GET",
        &format!("/api/v1/inventory/purchase-orders/{po_id}"),
        token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{po}");
    po
}

async fn create_goods_receipt(
    app: &Router,
    token: &str,
    po_id: &str,
    lines: &[(&str, i64)],
) -> Value {
    let line_json: Vec<Value> = lines
        .iter()
        .map(|(po_line_id, qty)| json!({ "po_line_id": po_line_id, "qty_received": qty }))
        .collect();
    let (st, grn) = call(
        app,
        "POST",
        "/api/v1/inventory/goods-receipts",
        token,
        Some(json!({ "purchase_order_id": po_id, "lines": line_json })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{grn}");
    grn
}

async fn post_goods_receipt(app: &Router, token: &str, grn_id: &str) -> (StatusCode, Value) {
    let idem = format!("grnpost-{}", new_uuid_v7());
    call(
        app,
        "POST",
        &format!("/api/v1/inventory/goods-receipts/{grn_id}/post"),
        token,
        None,
        &[("idempotency-key", idem.as_str())],
    )
    .await
}

async fn create_vendor_bill(app: &Router, token: &str, grn_id: &str, supplier_ref: &str) -> Value {
    let (st, bill) = call(
        app,
        "POST",
        "/api/v1/inventory/procure-to-pay/vendor-bill",
        token,
        Some(json!({ "goods_receipt_id": grn_id, "supplier_ref": supplier_ref })),
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{bill}");
    bill
}

async fn pay_vendor_bill(app: &Router, token: &str, bill_id: &str) -> Value {
    let idem = format!("pay-{}", new_uuid_v7());
    let (st, bill) = call(
        app,
        "POST",
        &format!("/api/v1/inventory/procure-to-pay/vendor-bill/{bill_id}/pay"),
        token,
        Some(json!({})),
        &[("idempotency-key", idem.as_str())],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bill}");
    bill
}

// ---------------------------------------------------------------------------
// Domain helpers — finance (periods / journals)
// ---------------------------------------------------------------------------

async fn list_periods(app: &Router, token: &str) -> Value {
    let (st, res) = call(app, "GET", "/api/v1/finance/periods", token, None, &[]).await;
    assert_eq!(st, StatusCode::OK, "{res}");
    res
}

async fn close_period(app: &Router, token: &str, period_id: &str) -> Value {
    let idem = format!("close-{}", new_uuid_v7());
    let (st, res) = call(
        app,
        "POST",
        &format!("/api/v1/finance/periods/{period_id}/close"),
        token,
        Some(json!({})),
        &[("idempotency-key", idem.as_str())],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{res}");
    res
}

async fn list_journals(app: &Router, token: &str, source_type: &str) -> Value {
    let (st, res) = call(
        app,
        "GET",
        &format!("/api/v1/finance/journals?source_type={source_type}"),
        token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{res}");
    res
}

fn current_month_code() -> String {
    let today = Utc::now().date_naive();
    format!("{:04}-{:02}", today.year(), today.month())
}

async fn find_open_period_for_today(app: &Router, token: &str) -> Value {
    let code = current_month_code();
    let periods = list_periods(app, token).await;
    periods["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["code"] == code)
        .cloned()
        .unwrap_or_else(|| {
            panic!("expected auto-created fiscal period for current month {code}: {periods}")
        })
}

fn assert_journal_balanced(entry: &Value) {
    let lines = entry["lines"]
        .as_array()
        .unwrap_or_else(|| panic!("{entry}"));
    let debit: i64 = lines
        .iter()
        .map(|l| l["debit_minor"].as_i64().unwrap())
        .sum();
    let credit: i64 = lines
        .iter()
        .map(|l| l["credit_minor"].as_i64().unwrap())
        .sum();
    assert_eq!(debit, credit, "journal entry not balanced: {entry}");
    assert!(debit > 0, "journal entry has zero value: {entry}");
}

// ---------------------------------------------------------------------------
// 1. Stock on hand == SUM(qty_delta) over the movement ledger
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stock_on_hand_equals_sum_of_movements() {
    let Some(seed) = seed_org_with_tokens("inv25-stock-sum").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-SUM", "Sum Warehouse").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(&app, &seed.owner_token, "SKU-SUM", "Sum Item", "USD", false).await;
    let item_id = item["id"].as_str().unwrap().to_string();

    let (st, receipt) = post_movement(
        &app,
        &seed.owner_token,
        &warehouse_id,
        &item_id,
        100,
        500,
        "receipt",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{receipt}");
    assert_eq!(receipt["qty_on_hand_after"], 100);

    let (st, issue) = post_movement(
        &app,
        &seed.owner_token,
        &warehouse_id,
        &item_id,
        -40,
        0,
        "issue",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{issue}");
    assert_eq!(issue["qty_on_hand_after"], 60);

    let level = item_stock_level(&app, &seed.owner_token, &item_id, &warehouse_id).await;
    assert_eq!(level["qty_on_hand"], 60, "{level}");

    // Cross-check directly against the append-only ledger.
    let warehouse_uuid = public_id_uuid(&warehouse_id);
    let item_uuid = public_id_uuid(&item_id);
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let sum: (Option<i64>,) = sqlx::query_as(
        "SELECT SUM(qty_delta)::bigint FROM inventory_stock_movement WHERE org_id = $1 AND warehouse_id = $2 AND item_id = $3",
    )
    .bind(seed.org.as_uuid())
    .bind(warehouse_uuid)
    .bind(item_uuid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(sum.0.unwrap_or(0), 60);
    assert_eq!(level["qty_on_hand"].as_i64().unwrap(), sum.0.unwrap_or(0));
}

// ---------------------------------------------------------------------------
// 2. Drift reconciliation raises an alert but never silently "fixes" the cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn drift_alert_never_silent_correction() {
    let Some(seed) = seed_org_with_tokens("inv25-drift").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-DRIFT", "Drift Warehouse").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(
        &app,
        &seed.owner_token,
        "SKU-DRIFT",
        "Drift Item",
        "USD",
        false,
    )
    .await;
    let item_id = item["id"].as_str().unwrap().to_string();

    let (st, receipt) = post_movement(
        &app,
        &seed.owner_token,
        &warehouse_id,
        &item_id,
        50,
        200,
        "receipt",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{receipt}");

    let warehouse_uuid = public_id_uuid(&warehouse_id);
    let item_uuid = public_id_uuid(&item_id);

    // Corrupt the cache directly — the ledger still sums to 50.
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    sqlx::query(
        "UPDATE inventory_stock_level SET qty_on_hand = 999 WHERE org_id = $1 AND warehouse_id = $2 AND item_id = $3",
    )
    .bind(seed.org.as_uuid())
    .bind(warehouse_uuid)
    .bind(item_uuid)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let recon = reconcile(&app, &seed.owner_token, Some(&warehouse_id), Some(&item_id)).await;
    assert_eq!(recon["drift_count"], 1, "{recon}");
    let alert = &recon["alerts"][0];
    assert_eq!(alert["cached_qty"], 999, "{recon}");
    assert_eq!(alert["movement_sum_qty"], 50, "{recon}");

    // The drift_alert row must exist in the DB — reconciliation is
    // alert-only, never a silent fix.
    let mut tx = seed.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seed.org).await.unwrap();
    let alerts: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT cached_qty, movement_sum_qty FROM inventory_drift_alert WHERE org_id = $1 AND warehouse_id = $2 AND item_id = $3",
    )
    .bind(seed.org.as_uuid())
    .bind(warehouse_uuid)
    .bind(item_uuid)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(alerts.len(), 1, "expected exactly one drift alert row");
    assert_eq!(alerts[0], (999, 50));

    // The cache must NOT have been silently corrected back to the ledger sum.
    let level = item_stock_level(&app, &seed.owner_token, &item_id, &warehouse_id).await;
    assert_eq!(
        level["qty_on_hand"], 999,
        "cache must remain uncorrected until an operator/adjustment resolves it: {level}"
    );

    // Reconciling again still finds the same ongoing (uncorrected) drift.
    let recon2 = reconcile(&app, &seed.owner_token, Some(&warehouse_id), Some(&item_id)).await;
    assert_eq!(recon2["drift_count"], 1, "{recon2}");
}

// ---------------------------------------------------------------------------
// 3. Negative-stock policy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn negative_stock_blocked_by_default() {
    let Some(seed) = seed_org_with_tokens("inv25-neg-block").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-NEG1", "Neg Warehouse 1").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(
        &app,
        &seed.owner_token,
        "SKU-NEG1",
        "Neg Item 1",
        "USD",
        false,
    )
    .await;
    let item_id = item["id"].as_str().unwrap().to_string();

    let (st, res) = post_movement(
        &app,
        &seed.owner_token,
        &warehouse_id,
        &item_id,
        -5,
        0,
        "issue",
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{res}");
}

#[tokio::test]
async fn negative_stock_allowed_when_policy_set() {
    let Some(seed) = seed_org_with_tokens("inv25-neg-allow").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-NEG2", "Neg Warehouse 2").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(
        &app,
        &seed.owner_token,
        "SKU-NEG2",
        "Neg Item 2",
        "USD",
        true,
    )
    .await;
    let item_id = item["id"].as_str().unwrap().to_string();

    let (st, res) = post_movement(
        &app,
        &seed.owner_token,
        &warehouse_id,
        &item_id,
        -10,
        0,
        "issue",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{res}");
    assert_eq!(res["qty_on_hand_after"], -10);

    let level = item_stock_level(&app, &seed.owner_token, &item_id, &warehouse_id).await;
    assert_eq!(level["qty_on_hand"], -10, "{level}");
}

// ---------------------------------------------------------------------------
// 4. Partial GRN then a second receipt completes the PO
// ---------------------------------------------------------------------------

#[tokio::test]
async fn partial_grn_then_second_receipt_completes() {
    let Some(seed) = seed_org_with_tokens("inv25-partial-grn").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let _finance = spawn_finance_server(seed.pool.clone(), seed.ring.clone()).await;
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-PARTIAL", "Partial Warehouse").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(
        &app,
        &seed.owner_token,
        "SKU-PARTIAL",
        "Partial Item",
        "USD",
        false,
    )
    .await;
    let item_id = item["id"].as_str().unwrap().to_string();
    let supplier = create_supplier(&app, &seed.owner_token, "Partial Supplier", "USD").await;
    let supplier_id = supplier["id"].as_str().unwrap().to_string();

    let po = create_purchase_order(
        &app,
        &seed.owner_token,
        &supplier_id,
        "USD",
        &[(item_id.as_str(), warehouse_id.as_str(), 10, 1_000)],
        None,
    )
    .await;
    let po_id = po["id"].as_str().unwrap().to_string();
    let po_line_id = po["lines"][0]["id"].as_str().unwrap().to_string();
    issue_purchase_order(&app, &seed.owner_token, &po_id).await;

    let grn1 =
        create_goods_receipt(&app, &seed.owner_token, &po_id, &[(po_line_id.as_str(), 4)]).await;
    let grn1_id = grn1["id"].as_str().unwrap().to_string();
    let (st, posted1) = post_goods_receipt(&app, &seed.owner_token, &grn1_id).await;
    assert_eq!(st, StatusCode::OK, "{posted1}");

    let po_after_1 = get_purchase_order(&app, &seed.owner_token, &po_id).await;
    assert_eq!(po_after_1["status"], "partially_received", "{po_after_1}");
    assert_eq!(po_after_1["lines"][0]["qty_received"], 4, "{po_after_1}");

    let grn2 =
        create_goods_receipt(&app, &seed.owner_token, &po_id, &[(po_line_id.as_str(), 6)]).await;
    let grn2_id = grn2["id"].as_str().unwrap().to_string();
    let (st, posted2) = post_goods_receipt(&app, &seed.owner_token, &grn2_id).await;
    assert_eq!(st, StatusCode::OK, "{posted2}");

    let po_after_2 = get_purchase_order(&app, &seed.owner_token, &po_id).await;
    assert_eq!(po_after_2["status"], "received", "{po_after_2}");
    assert_eq!(po_after_2["lines"][0]["qty_received"], 10, "{po_after_2}");

    let level = item_stock_level(&app, &seed.owner_token, &item_id, &warehouse_id).await;
    assert_eq!(level["qty_on_hand"], 10, "{level}");
}

// ---------------------------------------------------------------------------
// 5. Procure-to-pay end-to-end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn procure_to_pay_e2e() {
    let Some(seed) = seed_org_with_tokens("inv25-p2p-e2e").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let _finance = spawn_finance_server(seed.pool.clone(), seed.ring.clone()).await;
    let _project = spawn_project_server(seed.pool.clone(), seed.ring.clone()).await;
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());
    let finance = finance_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-P2P", "P2P Warehouse").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(&app, &seed.owner_token, "SKU-P2P", "P2P Item", "USD", false).await;
    let item_id = item["id"].as_str().unwrap().to_string();
    let supplier = create_supplier(&app, &seed.owner_token, "P2P Supplier", "USD").await;
    let supplier_id = supplier["id"].as_str().unwrap().to_string();

    // Purchase request -> submit (best-effort routes to companyos-project,
    // spawned above) -> decide (inventory's own callback, bypassing Temporal).
    let pr = create_purchase_request(
        &app,
        &seed.owner_token,
        "USD",
        &[(item_id.as_str(), 6, 1_000)],
    )
    .await;
    let pr_id = pr["id"].as_str().unwrap().to_string();

    let submitted = submit_purchase_request(&app, &seed.owner_token, &pr_id).await;
    assert_eq!(submitted["status"], "pending_approval", "{submitted}");

    let decided = decide_purchase_request(&app, &seed.owner_token, &pr_id, true).await;
    assert_eq!(decided["status"], "approved", "{decided}");

    // Purchase order created from the approved PR -> issue.
    let po = create_purchase_order(
        &app,
        &seed.owner_token,
        &supplier_id,
        "USD",
        &[(item_id.as_str(), warehouse_id.as_str(), 6, 1_000)],
        Some(pr_id.as_str()),
    )
    .await;
    let po_id = po["id"].as_str().unwrap().to_string();
    let po_line_id = po["lines"][0]["id"].as_str().unwrap().to_string();
    issue_purchase_order(&app, &seed.owner_token, &po_id).await;

    let (st, pr_after) = call(
        &app,
        "GET",
        &format!("/api/v1/inventory/purchase-requests/{pr_id}"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pr_after}");
    assert_eq!(pr_after["status"], "converted", "{pr_after}");

    // Partial goods receipt: 4 of the 6 ordered units.
    let grn =
        create_goods_receipt(&app, &seed.owner_token, &po_id, &[(po_line_id.as_str(), 4)]).await;
    let grn_id = grn["id"].as_str().unwrap().to_string();
    let (st, posted) = post_goods_receipt(&app, &seed.owner_token, &grn_id).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let journal_public_id = posted["journal_public_id"].as_str().unwrap().to_string();
    assert!(journal_public_id.starts_with("jrn_"), "{posted}");

    let po_after = get_purchase_order(&app, &seed.owner_token, &po_id).await;
    assert_eq!(po_after["status"], "partially_received", "{po_after}");

    // Vendor bill for exactly this receipt's value (4 * 1_000 = 4_000), then pay it.
    let bill = create_vendor_bill(&app, &seed.owner_token, &grn_id, "ACME-SUPPLIER-001").await;
    let bill_id = bill["id"].as_str().unwrap().to_string();
    assert_eq!(bill["amount_minor"], 4_000, "{bill}");
    assert_eq!(bill["status"], "open", "{bill}");

    let paid = pay_vendor_bill(&app, &seed.owner_token, &bill_id).await;
    assert_eq!(paid["status"], "paid", "{paid}");
    assert_eq!(paid["amount_paid_minor"], 4_000, "{paid}");
    assert!(paid["payment_journal_public_id"]
        .as_str()
        .map(|s| s.starts_with("jrn_"))
        .unwrap_or(false));

    // Journals exist for the receipt and balance.
    let journals = list_journals(&finance, &seed.owner_token, "inventory_receipt").await;
    let grn_uuid_str = public_id_uuid(&grn_id).to_string();
    let receipt_entry = journals["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["source_id"] == grn_uuid_str)
        .unwrap_or_else(|| {
            panic!("expected an inventory_receipt journal for GRN {grn_id}: {journals}")
        });
    assert_journal_balanced(receipt_entry);
}

// ---------------------------------------------------------------------------
// 6. Valuation journals balance; closed fiscal periods reject new postings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn valuation_journal_balanced_and_closed_period_rejects() {
    let Some(seed) = seed_org_with_tokens("inv25-period-close").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let _finance = spawn_finance_server(seed.pool.clone(), seed.ring.clone()).await;
    let app = inventory_app(seed.pool.clone(), seed.ring.clone());
    let finance = finance_app(seed.pool.clone(), seed.ring.clone());

    let wh = create_warehouse(&app, &seed.owner_token, "WH-CLOSE", "Close Warehouse").await;
    let warehouse_id = wh["id"].as_str().unwrap().to_string();
    let item = create_item(
        &app,
        &seed.owner_token,
        "SKU-CLOSE",
        "Close Item",
        "USD",
        false,
    )
    .await;
    let item_id = item["id"].as_str().unwrap().to_string();
    let supplier = create_supplier(&app, &seed.owner_token, "Close Supplier", "USD").await;
    let supplier_id = supplier["id"].as_str().unwrap().to_string();

    // First receipt succeeds — its journal posts into (and auto-opens, if
    // missing) the fiscal period covering today.
    let po1 = create_purchase_order(
        &app,
        &seed.owner_token,
        &supplier_id,
        "USD",
        &[(item_id.as_str(), warehouse_id.as_str(), 5, 1_000)],
        None,
    )
    .await;
    let po1_id = po1["id"].as_str().unwrap().to_string();
    let po1_line_id = po1["lines"][0]["id"].as_str().unwrap().to_string();
    issue_purchase_order(&app, &seed.owner_token, &po1_id).await;

    let grn1 = create_goods_receipt(
        &app,
        &seed.owner_token,
        &po1_id,
        &[(po1_line_id.as_str(), 5)],
    )
    .await;
    let grn1_id = grn1["id"].as_str().unwrap().to_string();
    let (st, posted1) = post_goods_receipt(&app, &seed.owner_token, &grn1_id).await;
    assert_eq!(st, StatusCode::OK, "{posted1}");
    let journal_id = posted1["journal_public_id"].as_str().unwrap().to_string();

    let (st, journal_entry) = call(
        &finance,
        "GET",
        &format!("/api/v1/finance/journals/{journal_id}"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{journal_entry}");
    assert_journal_balanced(&journal_entry);

    // Close the fiscal period covering today.
    let period = find_open_period_for_today(&finance, &seed.owner_token).await;
    let period_id = period["id"].as_str().unwrap().to_string();
    let closed = close_period(&finance, &seed.owner_token, &period_id).await;
    assert_eq!(closed["status"], "closed", "{closed}");

    // A second PO/GRN whose receipt journal would post (today) into the now
    // -closed period must be rejected with 409 — and the whole post rolls
    // back (no partial stock movement, no partial PO update).
    let po2 = create_purchase_order(
        &app,
        &seed.owner_token,
        &supplier_id,
        "USD",
        &[(item_id.as_str(), warehouse_id.as_str(), 3, 1_000)],
        None,
    )
    .await;
    let po2_id = po2["id"].as_str().unwrap().to_string();
    let po2_line_id = po2["lines"][0]["id"].as_str().unwrap().to_string();
    issue_purchase_order(&app, &seed.owner_token, &po2_id).await;

    let grn2 = create_goods_receipt(
        &app,
        &seed.owner_token,
        &po2_id,
        &[(po2_line_id.as_str(), 3)],
    )
    .await;
    let grn2_id = grn2["id"].as_str().unwrap().to_string();
    let (st, rejected) = post_goods_receipt(&app, &seed.owner_token, &grn2_id).await;
    assert_eq!(st, StatusCode::CONFLICT, "{rejected}");

    let (st, grn2_after) = call(
        &app,
        "GET",
        &format!("/api/v1/inventory/goods-receipts/{grn2_id}"),
        &seed.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{grn2_after}");
    assert_eq!(
        grn2_after["status"], "draft",
        "rejected post must roll back the GRN status transition: {grn2_after}"
    );

    let po2_after = get_purchase_order(&app, &seed.owner_token, &po2_id).await;
    assert_eq!(
        po2_after["status"], "issued",
        "rejected post must roll back the PO status/qty_received update: {po2_after}"
    );
    assert_eq!(po2_after["lines"][0]["qty_received"], 0, "{po2_after}");
}

// ---------------------------------------------------------------------------
// 7. Tenant isolation via RLS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tenant_isolation_rls() {
    let Some(a) = seed_org_with_tokens("inv25-tenant-a").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let Some(b) = seed_org_with_tokens("inv25-tenant-b").await else {
        return;
    };
    let app_a = inventory_app(a.pool.clone(), a.ring.clone());
    let app_b = inventory_app(b.pool.clone(), b.ring.clone());

    let wh_a = create_warehouse(&app_a, &a.owner_token, "WH-TENANT-A", "Tenant A Warehouse").await;
    let warehouse_a_id = wh_a["id"].as_str().unwrap().to_string();
    let item_a = create_item(
        &app_a,
        &a.owner_token,
        "SKU-TENANT-A",
        "Tenant A Item",
        "USD",
        false,
    )
    .await;
    let item_a_id = item_a["id"].as_str().unwrap().to_string();

    let wh_b = create_warehouse(&app_b, &b.owner_token, "WH-TENANT-B", "Tenant B Warehouse").await;
    let warehouse_b_id = wh_b["id"].as_str().unwrap().to_string();
    let item_b = create_item(
        &app_b,
        &b.owner_token,
        "SKU-TENANT-B",
        "Tenant B Item",
        "USD",
        false,
    )
    .await;
    let item_b_id = item_b["id"].as_str().unwrap().to_string();

    // Org B cannot fetch org A's item or warehouse by id.
    let (st, missing_item) = call(
        &app_b,
        "GET",
        &format!("/api/v1/inventory/items/{item_a_id}"),
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{missing_item}");

    let (st, missing_wh) = call(
        &app_b,
        "GET",
        &format!("/api/v1/inventory/warehouses/{warehouse_a_id}"),
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{missing_wh}");

    // Org A cannot fetch org B's item or warehouse by id either.
    let (st, missing_item_from_a) = call(
        &app_a,
        "GET",
        &format!("/api/v1/inventory/items/{item_b_id}"),
        &a.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{missing_item_from_a}");

    let (st, missing_wh_from_a) = call(
        &app_a,
        "GET",
        &format!("/api/v1/inventory/warehouses/{warehouse_b_id}"),
        &a.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{missing_wh_from_a}");

    // Each org's list views only ever contain their own rows.
    let (st, items_b) = call(
        &app_b,
        "GET",
        "/api/v1/inventory/items",
        &b.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{items_b}");
    let item_ids_b: Vec<&str> = items_b["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(!item_ids_b.contains(&item_a_id.as_str()), "{items_b}");
    assert!(item_ids_b.contains(&item_b_id.as_str()), "{items_b}");

    let (st, warehouses_a) = call(
        &app_a,
        "GET",
        "/api/v1/inventory/warehouses",
        &a.owner_token,
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{warehouses_a}");
    let wh_ids_a: Vec<&str> = warehouses_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["id"].as_str())
        .collect();
    assert!(
        !wh_ids_a.contains(&warehouse_b_id.as_str()),
        "{warehouses_a}"
    );
    assert!(
        wh_ids_a.contains(&warehouse_a_id.as_str()),
        "{warehouses_a}"
    );
}

// ---------------------------------------------------------------------------
// Property tests — pure valuation math (no DB required, always run).
// See also the unit tests in `src/valuation.rs` for edge cases (rounding,
// backorder coverage, depreciation caps).
// ---------------------------------------------------------------------------

mod valuation_properties {
    use companyos_inventory::valuation::{issue_cost_minor, weighted_average_receipt};
    use proptest::prelude::*;

    proptest! {
        /// Blending a receipt priced at exactly the current average cost
        /// must leave the average unchanged, for any quantities.
        #[test]
        fn receipt_at_same_cost_preserves_average(
            on_hand_qty in 0i64..1_000_000,
            avg in 0i64..1_000_000,
            receipt_qty in 1i64..1_000_000,
        ) {
            let (new_qty, new_avg) =
                weighted_average_receipt(on_hand_qty, avg, receipt_qty, avg).unwrap();
            prop_assert_eq!(new_qty, on_hand_qty + receipt_qty);
            prop_assert_eq!(new_avg, avg);
        }

        /// Issuing never changes the average — COGS is always exactly
        /// `qty * avg_unit_cost_minor`.
        #[test]
        fn issue_cost_is_exact_multiple_of_average(
            qty in 1i64..1_000_000,
            avg in 0i64..1_000_000,
        ) {
            let cogs = issue_cost_minor(qty, avg).unwrap();
            prop_assert_eq!(cogs, qty * avg);
        }
    }
}
