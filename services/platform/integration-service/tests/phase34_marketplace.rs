//! Phase 3.4 marketplace tests.
//! Requires TEST_DATABASE_URL / DATABASE_URL (non-superuser).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use companyos_authz::{perms, Principal, Role};
use companyos_ids::new_uuid_v7;
use companyos_integration::crypto::WebhookDecryptor;
use companyos_integration::dispatcher::DispatchOptions;
use companyos_integration::marketplace::principal::enforce;
use companyos_integration::marketplace::review::default_checklist;
use companyos_integration::marketplace::seed::seed_first_party_catalogue;
use companyos_integration::{build_router, AppState};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALLOW_PRIVATE: DispatchOptions = DispatchOptions {
    allow_private: true,
};

async fn pool() -> Option<PgPool> {
    let url = test_database_url()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .ok()?;
    companyos_outbox::migrate(&pool).await.ok()?;
    companyos_integration::migrate(&pool).await.ok()?;
    Some(pool)
}

fn marketplace_app(pool: PgPool) -> Router {
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let decryptor = WebhookDecryptor::from_env().expect("decryptor");
    build_router(
        AppState::with_dispatch_opts(pool, decryptor, ALLOW_PRIVATE).with_allow_private_urls(true),
    )
}

/// A router that enforces SSRF checks on listing URLs (no private allowance).
fn strict_url_app(pool: PgPool) -> Router {
    std::env::set_var("COMPANYOS_LOCAL_AUTH", "1");
    let decryptor = WebhookDecryptor::from_env().expect("decryptor");
    build_router(
        AppState::with_dispatch_opts(pool, decryptor, ALLOW_PRIVATE).with_allow_private_urls(false),
    )
}

#[derive(Clone, Copy)]
struct Caller {
    org: OrgId,
    user: Uuid,
    role: &'static str,
}

impl Caller {
    fn owner() -> Self {
        Self {
            org: OrgId::generate(),
            user: new_uuid_v7(),
            role: "owner",
        }
    }

    fn as_role(self, role: &'static str) -> Self {
        Self { role, ..self }
    }
}

fn request(method: &str, uri: &str, caller: Caller, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-request-id", "phase34")
        .header("x-companyos-dev-org-id", caller.org.to_public().as_str())
        .header(
            "x-companyos-dev-user-id",
            companyos_ids::PublicId::new(companyos_ids::IdKind::User, caller.user).as_str(),
        )
        .header("x-companyos-dev-role", caller.role);
    match body {
        Some(value) => builder
            .body(Body::from(value.to_string()))
            .expect("request"),
        None => builder.body(Body::empty()).expect("request"),
    }
}

fn anon_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-request-id", "phase34")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn call(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.expect("response");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

struct PublishedApp {
    listing_id: String,
    client_id: String,
    client_secret: String,
}

/// Draft → submit → complete every checklist item → publish.
async fn publish_listing(
    app: &Router,
    publisher: Caller,
    scopes: &[&str],
    redirect_uris: &[&str],
) -> PublishedApp {
    let slug = format!("app-{}", new_uuid_v7());
    let (status, created) = call(
        app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "Test App",
                "slug": slug,
                "description": "phase 3.4 fixture",
                "listing_kind": "third_party",
                "requested_scopes": scopes,
                "redirect_uris": redirect_uris,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create listing: {created}");

    let listing_id = created["listing"]["id"]
        .as_str()
        .expect("listing id")
        .to_string();
    let client_id = created["oauth_client_id"]
        .as_str()
        .expect("client id")
        .to_string();
    let client_secret = created["client_secret"]
        .as_str()
        .expect("client secret")
        .to_string();

    let (status, body) = call(
        app,
        request(
            "POST",
            &format!("/api/v1/marketplace/listings/{listing_id}/submit"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "submit: {body}");

    let all_items: Vec<String> = default_checklist().into_iter().map(|i| i.id).collect();
    let (status, body) = call(
        app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/checklist"),
            publisher,
            Some(json!({ "completed_item_ids": all_items })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "checklist: {body}");

    let (status, body) = call(
        app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/publish"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "publish: {body}");

    PublishedApp {
        listing_id,
        client_id,
        client_secret,
    }
}

async fn install(
    app: &Router,
    installer: Caller,
    listing_id: &str,
    consented: &[&str],
) -> (StatusCode, Value) {
    call(
        app,
        request(
            "POST",
            "/api/v1/marketplace/installs",
            installer,
            Some(json!({ "listing_id": listing_id, "consented_scopes": consented })),
        ),
    )
    .await
}

async fn authorize_permission(app: &Router, access_token: &str, permission: &str) -> StatusCode {
    call(
        app,
        anon_request(
            "POST",
            "/api/v1/marketplace/oauth/authorize-permission",
            json!({ "access_token": access_token, "permission": permission }),
        ),
    )
    .await
    .0
}

// ---------------------------------------------------------------------------
// 1. Consent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn install_consent_limits_token_authority() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let publisher = Caller::owner();
    let installer = Caller::owner();

    let published = publish_listing(
        &app,
        publisher,
        &[
            "sales.customer.read",
            "sales.deal.read",
            "finance.invoice.read",
        ],
        &["https://8.8.8.8/callback"],
    )
    .await;

    let (status, body) = install(
        &app,
        installer,
        &published.listing_id,
        &["sales.customer.read", "sales.deal.read"],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "install: {body}");
    let access_token = body["access_token"].as_str().expect("access token");
    assert_eq!(
        body["install"]["consented_scopes"],
        json!(["sales.customer.read", "sales.deal.read"])
    );

    assert_eq!(
        authorize_permission(&app, access_token, "sales.customer.read").await,
        StatusCode::OK
    );
    // Requested by the listing but never consented to.
    assert_eq!(
        authorize_permission(&app, access_token, "finance.invoice.read").await,
        StatusCode::FORBIDDEN
    );
    // Never requested by the listing at all.
    assert_eq!(
        authorize_permission(&app, access_token, "hr.payroll.run").await,
        StatusCode::FORBIDDEN
    );

    // Consent cannot exceed what the listing requested.
    let (status, body) = install(
        &app,
        Caller::owner(),
        &published.listing_id,
        &["hr.payroll.run"],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn reconsent_rotates_tokens_to_the_new_scope_set() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let installer = Caller::owner();
    let published = publish_listing(
        &app,
        Caller::owner(),
        &["sales.customer.read", "sales.deal.read"],
        &[],
    )
    .await;

    let (_, body) = install(
        &app,
        installer,
        &published.listing_id,
        &["sales.customer.read"],
    )
    .await;
    let old_token = body["access_token"].as_str().expect("token").to_string();
    let install_id = body["install"]["id"]
        .as_str()
        .expect("install id")
        .to_string();
    assert_eq!(
        authorize_permission(&app, &old_token, "sales.deal.read").await,
        StatusCode::FORBIDDEN
    );

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/installs/{install_id}/reconsent"),
            installer,
            Some(json!({ "consented_scopes": ["sales.customer.read", "sales.deal.read"] })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "reconsent: {body}");
    let new_token = body["access_token"].as_str().expect("token").to_string();

    // Widening consent invalidates the old snapshot and mints a new one.
    assert_eq!(
        authorize_permission(&app, &old_token, "sales.customer.read").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        authorize_permission(&app, &new_token, "sales.deal.read").await,
        StatusCode::OK
    );
}

// ---------------------------------------------------------------------------
// 2. Uninstall revokes everything
// ---------------------------------------------------------------------------

#[tokio::test]
async fn uninstall_revokes_tokens_and_blocks_refresh() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool.clone());
    let installer = Caller::owner();
    let published = publish_listing(&app, Caller::owner(), &["sales.customer.read"], &[]).await;

    let (_, body) = install(
        &app,
        installer,
        &published.listing_id,
        &["sales.customer.read"],
    )
    .await;
    let access_token = body["access_token"].as_str().expect("access").to_string();
    let refresh_token = body["refresh_token"].as_str().expect("refresh").to_string();
    let install_id = body["install"]["id"]
        .as_str()
        .expect("install id")
        .to_string();
    assert_eq!(
        authorize_permission(&app, &access_token, "sales.customer.read").await,
        StatusCode::OK
    );

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/installs/{install_id}/uninstall"),
            installer,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "uninstall: {body}");
    assert_eq!(body["status"], "revoked");
    assert_eq!(body["outbound_enabled"], false);
    assert_eq!(body["inbound_enabled"], false);

    assert_eq!(
        authorize_permission(&app, &access_token, "sales.customer.read").await,
        StatusCode::UNAUTHORIZED
    );

    let (status, body) = call(
        &app,
        anon_request(
            "POST",
            "/api/v1/marketplace/oauth/token",
            json!({
                "grant_type": "refresh_token",
                "client_id": published.client_id,
                "client_secret": published.client_secret,
                "refresh_token": refresh_token,
            }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "refresh after uninstall: {body}"
    );

    // Every token row for the install is revoked, access and refresh alike.
    let mut tx = pool.begin().await.expect("tx");
    set_session_org_id(&mut tx, installer.org)
        .await
        .expect("org");
    let (live,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM marketplace_app_token \
         WHERE org_id = $1 AND revoked_at IS NULL",
    )
    .bind(installer.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("count");
    tx.commit().await.expect("commit");
    assert_eq!(live, 0);
}

// ---------------------------------------------------------------------------
// 3. Publish gate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publish_blocked_until_security_checklist_complete() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let publisher = Caller::owner();

    let slug = format!("gate-{}", new_uuid_v7());
    let (status, created) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "Gated App",
                "slug": slug,
                "requested_scopes": ["sales.customer.read"],
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let listing_id = created["listing"]["id"].as_str().expect("id").to_string();

    call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/listings/{listing_id}/submit"),
            publisher,
            None,
        ),
    )
    .await;

    // Nothing completed yet.
    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/publish"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Complete every required item except the security ones.
    let non_security: Vec<String> = default_checklist()
        .into_iter()
        .filter(|i| !i.id.starts_with("security_"))
        .map(|i| i.id)
        .collect();
    let (status, review) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/checklist"),
            publisher,
            Some(json!({ "completed_item_ids": non_security })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["security_review_completed"], false);

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/publish"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "security review must gate publication: {body}"
    );

    // Now complete the security items too.
    let security: Vec<String> = default_checklist()
        .into_iter()
        .filter(|i| i.id.starts_with("security_"))
        .map(|i| i.id)
        .collect();
    assert!(security.len() >= 2);
    let (_, review) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/checklist"),
            publisher,
            Some(json!({ "completed_item_ids": security })),
        ),
    )
    .await;
    assert_eq!(review["security_review_completed"], true);

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/publish"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "published");
}

#[tokio::test]
async fn review_queue_lists_submitted_listings() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let publisher = Caller::owner();

    let slug = format!("queued-{}", new_uuid_v7());
    let (_, created) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "Queued App",
                "slug": slug,
                "requested_scopes": ["sales.customer.read"],
            })),
        ),
    )
    .await;
    let listing_id = created["listing"]["id"].as_str().expect("id").to_string();
    call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/listings/{listing_id}/submit"),
            publisher,
            None,
        ),
    )
    .await;

    let (status, body) = call(
        &app,
        request("GET", "/api/v1/marketplace/reviews/queue", publisher, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["listing_id"], listing_id);
    assert_eq!(items[0]["security_review_completed"], false);

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/reviews/{listing_id}/reject"),
            publisher,
            Some(json!({ "reason": "insufficient scope justification" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "rejected");
}

/// The web app addresses review and uninstall through alias routes; they must
/// behave identically to the canonical ones.
#[tokio::test]
async fn web_alias_routes_match_the_canonical_ones() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let publisher = Caller::owner();

    let slug = format!("alias-{}", new_uuid_v7());
    let (_, created) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "Alias App",
                "slug": slug,
                "requested_scopes": ["sales.customer.read"],
                "redirect_uris": [""],
            })),
        ),
    )
    .await;
    let listing_id = created["listing"]["id"].as_str().expect("id").to_string();
    assert_eq!(
        created["listing"]["redirect_uris"],
        json!([]),
        "blank redirect_uri entries are dropped"
    );
    call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/listings/{listing_id}/submit"),
            publisher,
            None,
        ),
    )
    .await;

    let (status, body) = call(
        &app,
        request("GET", "/api/v1/marketplace/review", publisher, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"][0]["listing_id"], listing_id);
    assert_eq!(body["items"][0]["listing_name"], "Alias App");

    let all_items: Vec<String> = default_checklist().into_iter().map(|i| i.id).collect();
    let (status, body) = call(
        &app,
        request(
            "PATCH",
            &format!("/api/v1/marketplace/listings/{listing_id}/review"),
            publisher,
            Some(json!({ "completed_item_ids": all_items })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["security_review_completed"], true);

    let (status, body) = call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/listings/{listing_id}/publish"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        &app,
        request(
            "GET",
            &format!("/api/v1/marketplace/catalogue/{listing_id}"),
            publisher,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], listing_id);

    let installer = Caller::owner();
    let (_, body) = install(&app, installer, &listing_id, &["sales.customer.read"]).await;
    let install_id = body["install"]["id"].as_str().expect("id").to_string();
    let (status, body) = call(
        &app,
        request(
            "DELETE",
            &format!("/api/v1/marketplace/installs/{install_id}"),
            installer,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "revoked");
}

// ---------------------------------------------------------------------------
// 4. First-party connectors take the same path as third-party apps
// ---------------------------------------------------------------------------

#[tokio::test]
async fn first_party_connectors_install_through_the_same_path() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool.clone());
    let publisher = Caller::owner();
    seed_first_party_catalogue(&pool, publisher.org)
        .await
        .expect("seed catalogue");

    let installer = Caller::owner();
    let connectors = ["email.google", "calendar.microsoft", "payments.stripe"];
    let mut install_ids = Vec::new();

    for connector in connectors {
        let (status, body) = call(
            &app,
            request(
                "POST",
                &format!("/api/v1/integrations/{connector}/connect"),
                installer,
                Some(json!({})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "connect {connector}: {body}");
        assert_eq!(body["install"]["connector_key"], connector);
        assert_eq!(body["install"]["listing_kind"], "first_party");
        assert_eq!(body["install"]["status"], "active");
        assert!(body["access_token"].as_str().is_some());
        install_ids.push(body["install"]["id"].as_str().expect("id").to_string());
    }

    // A third-party install lands in exactly the same table with the same shape.
    let published = publish_listing(&app, Caller::owner(), &["sales.customer.read"], &[]).await;
    let (_, third) = install(
        &app,
        installer,
        &published.listing_id,
        &["sales.customer.read"],
    )
    .await;
    install_ids.push(third["install"]["id"].as_str().expect("id").to_string());

    let mut tx = pool.begin().await.expect("tx");
    set_session_org_id(&mut tx, installer.org)
        .await
        .expect("org");
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT public_id, status, connector_key FROM marketplace_install WHERE org_id = $1",
    )
    .bind(installer.org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("installs");
    let token_counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT i.public_id, COUNT(t.id)::bigint FROM marketplace_install i \
         JOIN marketplace_app_token t ON t.install_id = i.id \
         WHERE i.org_id = $1 GROUP BY i.public_id",
    )
    .bind(installer.org.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("tokens");
    tx.commit().await.expect("commit");

    assert_eq!(rows.len(), 4, "all installs share marketplace_install");
    assert!(rows.iter().all(|(_, status, _)| status == "active"));
    assert_eq!(token_counts.len(), 4, "every install minted tokens");
    assert!(
        token_counts.iter().all(|(_, count)| *count == 2),
        "each install gets one access + one refresh token: {token_counts:?}"
    );

    // The integrations alias only surfaces connector-backed installs.
    let (status, body) = call(
        &app,
        request("GET", "/api/v1/integrations", installer, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 3);

    // Disconnect uses the same revoke path as uninstall.
    let (status, body) = call(
        &app,
        request(
            "POST",
            "/api/v1/integrations/email.google/disconnect",
            installer,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "revoked");
    assert_eq!(body["outbound_enabled"], false);
}

#[tokio::test]
async fn catalogue_exposes_published_listings_across_orgs() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool.clone());
    let publisher = Caller::owner();
    seed_first_party_catalogue(&pool, publisher.org)
        .await
        .expect("seed");

    let browser = Caller::owner();
    let (status, body) = call(
        &app,
        request("GET", "/api/v1/marketplace/catalogue", browser, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let keys: Vec<&str> = body["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|i| i["connector_key"].as_str())
        .collect();
    for connector in [
        "email.google",
        "calendar.microsoft",
        "payments.stripe",
        "storage.s3",
        "chat.slack",
    ] {
        assert!(keys.contains(&connector), "catalogue missing {connector}");
    }
}

// ---------------------------------------------------------------------------
// 5. Tenant isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn org_b_cannot_see_org_a_installs() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool.clone());
    let published = publish_listing(&app, Caller::owner(), &["sales.customer.read"], &[]).await;

    let org_a = Caller::owner();
    let org_b = Caller::owner();
    let (_, body) = install(&app, org_a, &published.listing_id, &["sales.customer.read"]).await;
    let install_public_id = body["install"]["id"].as_str().expect("id").to_string();

    let (status, body) = call(
        &app,
        request("GET", "/api/v1/marketplace/installs", org_b, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().expect("items").len(), 0);

    let (status, _) = call(
        &app,
        request(
            "GET",
            &format!("/api/v1/marketplace/installs/{install_public_id}"),
            org_b,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Planted cross-tenant query: RLS must hide org A's rows from org B.
    let mut tx = pool.begin().await.expect("tx");
    set_session_org_id(&mut tx, org_b.org).await.expect("org");
    let leaked: Vec<(String,)> = sqlx::query_as("SELECT public_id FROM marketplace_install")
        .fetch_all(&mut *tx)
        .await
        .expect("select");
    let leaked_tokens: Vec<(String,)> =
        sqlx::query_as("SELECT public_id FROM marketplace_app_token")
            .fetch_all(&mut *tx)
            .await
            .expect("select");
    tx.commit().await.expect("commit");
    assert!(
        !leaked.iter().any(|(id,)| id == &install_public_id),
        "TENANT ISOLATION FAILURE: org B read org A install"
    );
    assert!(leaked_tokens.is_empty(), "org B saw foreign app tokens");
}

// ---------------------------------------------------------------------------
// 6. Role boundaries
// ---------------------------------------------------------------------------

#[test]
fn member_principal_cannot_review_write_or_install() {
    let member = Principal::with_roles(vec![Role::Member]);
    assert!(enforce(&member, perms::admin_marketplace_read(), "t").is_ok());
    for denied in [
        perms::admin_marketplace_write(),
        perms::admin_marketplace_review(),
        perms::admin_marketplace_install(),
        perms::admin_marketplace_uninstall(),
    ] {
        assert!(
            enforce(&member, denied.clone(), "t").is_err(),
            "member must not hold {}",
            denied.as_str()
        );
    }
}

#[tokio::test]
async fn member_cannot_review_or_publish_over_http() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let publisher = Caller::owner();
    let member = publisher.as_role("member");

    let slug = format!("member-{}", new_uuid_v7());
    let (_, created) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "Member Denied",
                "slug": slug,
                "requested_scopes": ["sales.customer.read"],
            })),
        ),
    )
    .await;
    let listing_id = created["listing"]["id"].as_str().expect("id").to_string();

    for (method, uri, body) in [
        (
            "POST",
            format!("/api/v1/marketplace/reviews/{listing_id}/checklist"),
            Some(json!({ "completed_item_ids": [] })),
        ),
        (
            "POST",
            format!("/api/v1/marketplace/reviews/{listing_id}/publish"),
            None,
        ),
        ("GET", "/api/v1/marketplace/reviews/queue".to_string(), None),
        (
            "POST",
            "/api/v1/marketplace/listings".to_string(),
            Some(json!({ "name": "x", "slug": "x", "requested_scopes": [] })),
        ),
    ] {
        let (status, payload) = call(&app, request(method, &uri, member, body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {payload}");
    }

    // Read-only catalogue access still works for a member.
    let (status, _) = call(
        &app,
        request("GET", "/api/v1/marketplace/catalogue", member, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 7. OAuth code exchange
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oauth_code_exchange_installs_a_third_party_app() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool);
    let redirect_uri = "https://8.8.8.8/oauth/callback";
    let published = publish_listing(
        &app,
        Caller::owner(),
        &["sales.customer.read", "sales.deal.read"],
        &[redirect_uri],
    )
    .await;

    let installer = Caller::owner();
    let verifier = "phase34-pkce-verifier-with-enough-entropy-0123456789";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let (status, body) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/oauth/authorize",
            installer,
            Some(json!({
                "listing_id": published.listing_id,
                "redirect_uri": redirect_uri,
                "consented_scopes": ["sales.customer.read"],
                "code_challenge": challenge,
                "code_challenge_method": "S256",
                "state": "xyz",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "authorize: {body}");
    let code = body["code"].as_str().expect("code").to_string();
    assert_eq!(body["state"], "xyz");

    // Wrong PKCE verifier is rejected.
    let (status, body) = call(
        &app,
        anon_request(
            "POST",
            "/api/v1/marketplace/oauth/token",
            json!({
                "grant_type": "authorization_code",
                "client_id": published.client_id,
                "client_secret": published.client_secret,
                "code": code,
                "code_verifier": "not-the-verifier",
                "redirect_uri": redirect_uri,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "bad pkce: {body}");

    // Wrong client secret is rejected.
    let (status, _) = call(
        &app,
        anon_request(
            "POST",
            "/api/v1/marketplace/oauth/token",
            json!({
                "grant_type": "authorization_code",
                "client_id": published.client_id,
                "client_secret": "wrong-secret",
                "code": code,
                "code_verifier": verifier,
                "redirect_uri": redirect_uri,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let exchange = json!({
        "grant_type": "authorization_code",
        "client_id": published.client_id,
        "client_secret": published.client_secret,
        "code": code,
        "code_verifier": verifier,
        "redirect_uri": redirect_uri,
    });
    let (status, body) = call(
        &app,
        anon_request("POST", "/api/v1/marketplace/oauth/token", exchange.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "exchange: {body}");
    let access_token = body["access_token"].as_str().expect("access").to_string();
    let refresh_token = body["refresh_token"].as_str().expect("refresh").to_string();
    assert_eq!(body["scope"], json!(["sales.customer.read"]));

    assert_eq!(
        authorize_permission(&app, &access_token, "sales.customer.read").await,
        StatusCode::OK
    );
    assert_eq!(
        authorize_permission(&app, &access_token, "sales.deal.read").await,
        StatusCode::FORBIDDEN
    );

    // Codes are single-use.
    let (status, _) = call(
        &app,
        anon_request("POST", "/api/v1/marketplace/oauth/token", exchange),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Refresh rotates and keeps the consented scope set.
    let (status, body) = call(
        &app,
        anon_request(
            "POST",
            "/api/v1/marketplace/oauth/token",
            json!({
                "grant_type": "refresh_token",
                "client_id": published.client_id,
                "client_secret": published.client_secret,
                "refresh_token": refresh_token,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "refresh: {body}");
    assert_eq!(body["scope"], json!(["sales.customer.read"]));
    let rotated = body["access_token"].as_str().expect("access").to_string();
    assert_ne!(rotated, access_token);
    assert_eq!(
        authorize_permission(&app, &rotated, "sales.customer.read").await,
        StatusCode::OK
    );

    // The install created by the exchange is an ordinary marketplace install.
    let (status, body) = call(
        &app,
        request("GET", "/api/v1/marketplace/installs", installer, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["listing_kind"], "third_party");
    assert_eq!(items[0]["status"], "active");
}

// ---------------------------------------------------------------------------
// SSRF
// ---------------------------------------------------------------------------

#[tokio::test]
async fn listing_redirect_uris_are_ssrf_checked() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = strict_url_app(pool);
    let publisher = Caller::owner();

    for bad in [
        "http://127.0.0.1/callback",
        "http://169.254.169.254/latest/meta-data",
        "http://10.0.0.5/callback",
        "ftp://8.8.8.8/callback",
    ] {
        let (status, body) = call(
            &app,
            request(
                "POST",
                "/api/v1/marketplace/listings",
                publisher,
                Some(json!({
                    "name": "SSRF probe",
                    "slug": format!("ssrf-{}", new_uuid_v7()),
                    "requested_scopes": ["sales.customer.read"],
                    "redirect_uris": [bad],
                })),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{bad} was accepted: {body}"
        );
    }

    let (status, body) = call(
        &app,
        request(
            "POST",
            "/api/v1/marketplace/listings",
            publisher,
            Some(json!({
                "name": "SSRF probe ok",
                "slug": format!("ssrf-ok-{}", new_uuid_v7()),
                "requested_scopes": ["sales.customer.read"],
                "redirect_uris": ["https://8.8.8.8/callback"],
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_emits_marketplace_outbox_events() {
    let Some(pool) = pool().await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = marketplace_app(pool.clone());
    let publisher = Caller::owner();
    let installer = Caller::owner();

    let published = publish_listing(&app, publisher, &["sales.customer.read"], &[]).await;
    let (_, body) = install(
        &app,
        installer,
        &published.listing_id,
        &["sales.customer.read"],
    )
    .await;
    let install_id = body["install"]["id"].as_str().expect("id").to_string();
    call(
        &app,
        request(
            "POST",
            &format!("/api/v1/marketplace/installs/{install_id}/uninstall"),
            installer,
            None,
        ),
    )
    .await;

    for (org, expected) in [
        (
            publisher.org,
            vec![
                "listing_created",
                "oauth_client_created",
                "listing_submitted",
                "listing_published",
            ],
        ),
        (
            installer.org,
            vec!["install_created", "oauth_token_issued", "install_revoked"],
        ),
    ] {
        let mut tx = pool.begin().await.expect("tx");
        set_session_org_id(&mut tx, org).await.expect("org");
        let subjects: Vec<(String,)> =
            sqlx::query_as("SELECT subject FROM outbox_event WHERE org_id = $1")
                .bind(org.as_uuid())
                .fetch_all(&mut *tx)
                .await
                .expect("events");
        tx.commit().await.expect("commit");
        for event in expected {
            assert!(
                subjects
                    .iter()
                    .any(|(s,)| s.contains(&format!(".admin.marketplace.{event}."))),
                "missing {event} in {subjects:?}"
            );
        }
    }
}
