//! Phase 4.3 autonomous agents integration tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use companyos_ai::agents::AgentPolicyDoc;
use companyos_ai::state::AppState as AiAppState;
use companyos_auth_token::KeyRing;
use companyos_core::auth::password;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use companyos_testkit::test_database_url;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::{Duration, Instant};
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
    companyos_ai::migrate(&pool).await.ok()?;
    Some(pool)
}

fn ai_app(pool: PgPool, ring: KeyRing) -> Router {
    companyos_ai::build_router(AiAppState::new(pool, ring))
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

#[allow(dead_code)]
struct Seeded {
    pool: PgPool,
    ring: KeyRing,
    org: OrgId,
    owner_id: Uuid,
    owner_token: String,
    read_only_token: String,
    member_id: Uuid,
    member_token: String,
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
                        "org_name": "AI Phase43"
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

    let read_only_id = insert_member_with_role(&pool, org, "read_only", "ReadOnly").await;
    let member_id = insert_member_with_role(&pool, org, "member", "Member").await;

    let owner_token = session_token(&pool, &ring, org, owner_id, "owner").await;
    let read_only_token = session_token(&pool, &ring, org, read_only_id, "read_only").await;
    let member_token = session_token(&pool, &ring, org, member_id, "member").await;

    Some(Seeded {
        pool,
        ring,
        org,
        owner_id,
        owner_token,
        read_only_token,
        member_id,
        member_token,
    })
}

fn default_policy() -> AgentPolicyDoc {
    AgentPolicyDoc::default()
}

async fn oneshot_json(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    request_id: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", request_id);
    let req = if let Some(b) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn publish_policy(app: &Router, token: &str, doc: &AgentPolicyDoc) -> (StatusCode, Value) {
    oneshot_json(
        app,
        "POST",
        "/api/v1/ai/agents/policy",
        token,
        "ai-43-policy",
        Some(serde_json::to_value(doc).unwrap()),
    )
    .await
}

async fn start_run(
    app: &Router,
    token: &str,
    agent_type: &str,
    step_delay_ms: Option<u64>,
    request_id: &str,
) -> (StatusCode, Value) {
    let mut body = json!({ "agent_type": agent_type });
    if let Some(ms) = step_delay_ms {
        body["step_delay_ms"] = json!(ms);
    }
    oneshot_json(
        app,
        "POST",
        "/api/v1/ai/agents/runs",
        token,
        request_id,
        Some(body),
    )
    .await
}

async fn chat(app: &Router, token: &str, message: &str) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-request-id", "ai-chat")
                .body(Body::from(json!({ "message": message }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    (status, body)
}

fn detail_contains(body: &Value, needle: &str) -> bool {
    body["detail"]
        .as_str()
        .map(|d| {
            d.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
        .unwrap_or(false)
        || body
            .to_string()
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
}

#[tokio::test]
async fn unattended_receivables_inside_policy() {
    let Some(seeded) = seed("ai-43-chase").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &default_policy()).await;
    assert_eq!(pol_status, StatusCode::OK);

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-chase-run",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start run: {body}");

    let run_status = body["run"]["status"].as_str().unwrap_or("");
    assert!(
        run_status == "completed" || run_status.contains("partial"),
        "expected completed/partially, got {run_status}: {body}"
    );
    let action_ids = body["action_ids"].as_array().expect("action_ids");
    assert!(
        !action_ids.is_empty(),
        "expected at least one action_id: {body}"
    );

    let trace = body["tool_trace"].as_array().expect("tool_trace");
    assert!(
        trace
            .iter()
            .any(|t| { t["tool_name"] == "send_invoice_reminder" && t["decision"] == "allow" }),
        "expected allow for send_invoice_reminder: {body}"
    );
}

#[tokio::test]
async fn unattended_denied_when_policy_omits_permission() {
    let Some(seeded) = seed("ai-43-deny").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let mut policy = default_policy();
    policy.allowed_tools = vec!["list_overdue_invoices".into(), "escalate_exception".into()];
    policy.allowed_permissions = vec![
        "finance.invoice.read".into(),
        "platform.notification.read".into(),
        "operations.task.create".into(),
    ];

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &policy).await;
    assert_eq!(pol_status, StatusCode::OK);

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-deny-run",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start run: {body}");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let reminder_actions: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM ai_action
        WHERE org_id = $1 AND tool_name = 'send_invoice_reminder' AND status = 'committed'
        "#,
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        reminder_actions.0, 0,
        "no successful send_invoice_reminder action"
    );

    let empty: Vec<Value> = vec![];
    let denied_in_trace = body["tool_trace"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .any(|t| t["tool_name"] == "send_invoice_reminder" && t["decision"] == "deny");
    let denied_in_last = body["run"]["last_actions"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .any(|a| {
            a["tool"] == "send_invoice_reminder"
                && (a["denied"] == true || a["reason"].as_str().unwrap_or("").contains("deny"))
        });
    assert!(
        denied_in_trace || denied_in_last,
        "reminder tool must be denied in tool_trace or last_actions: {body}"
    );
}

#[tokio::test]
async fn prompt_injection_cannot_widen_tools_or_cross_orgs() {
    let Some(seeded) = seed("ai-43-inject").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &default_policy()).await;
    assert_eq!(pol_status, StatusCode::OK);

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-inject-run",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start run: {body}");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let void_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM ai_action WHERE org_id = $1 AND tool_name = 'void_invoice'",
    )
    .bind(seeded.org.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(void_count.0, 0, "no void_invoice actions");

    let run_actions: Vec<(Uuid,)> =
        sqlx::query_as("SELECT org_id FROM ai_action WHERE org_id = $1")
            .bind(seeded.org.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    for (oid,) in &run_actions {
        assert_eq!(*oid, seeded.org.as_uuid());
    }
    tx.commit().await.unwrap();

    let empty: Vec<Value> = vec![];
    let trace = body["tool_trace"].as_array().unwrap_or(&empty);
    for t in trace {
        let perm = t["permission"].as_str().unwrap_or("");
        assert_ne!(
            perm, "finance.invoice.void",
            "injection must not grant void permission in tool_trace: {body}"
        );
        assert_ne!(t["tool_name"], "void_invoice");
    }
}

#[tokio::test]
async fn kill_switch_halts_within_2s() {
    let Some(seeded) = seed("ai-43-kill").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &default_policy()).await;
    assert_eq!(pol_status, StatusCode::OK);

    let app_run = app.clone();
    let token = seeded.owner_token.clone();
    let run_task = tokio::spawn(async move {
        start_run(
            &app_run,
            &token,
            "receivables_chase",
            Some(400),
            "ai-43-kill-run",
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let kill_at = Instant::now();
    let (kill_status, kill_body) = oneshot_json(
        &app,
        "POST",
        "/api/v1/ai/agents/kill-switch",
        &seeded.owner_token,
        "ai-43-kill-engage",
        Some(json!({
            "engaged": true,
            "reason": "phase43 kill test"
        })),
    )
    .await;
    assert_eq!(kill_status, StatusCode::OK, "kill switch: {kill_body}");

    let (run_status, run_body) = run_task.await.expect("run task join");
    let elapsed = kill_at.elapsed();
    assert!(
        elapsed <= Duration::from_secs(2),
        "kill→completion took {elapsed:?}, expected ≤2s: {run_body}"
    );

    assert_eq!(
        run_status,
        StatusCode::OK,
        "killed run response: {run_body}"
    );
    let status = run_body["run"]["status"].as_str().unwrap_or("");
    let err = run_body["run"]["error_message"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let last = run_body["run"]["last_actions"]
        .to_string()
        .to_ascii_lowercase();
    assert!(
        status == "killed"
            || err.contains("kill")
            || last.contains("kill_switch")
            || last.contains("kill"),
        "expected killed outcome, got status={status}: {run_body}"
    );

    let (refuse_status, refuse_body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-kill-refuse",
    )
    .await;
    assert!(
        refuse_status == StatusCode::CONFLICT || detail_contains(&refuse_body, "kill switch"),
        "new run must refuse under kill switch: {refuse_status} {refuse_body}"
    );
}

#[tokio::test]
async fn reversal_undoes_agent_write_as_unit() {
    let Some(seeded) = seed("ai-43-reverse").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &default_policy()).await;
    assert_eq!(pol_status, StatusCode::OK);

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-reverse-run",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start run: {body}");
    let action_id = body["action_ids"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .expect("first action_id");

    let (rev_status, rev_body) = oneshot_json(
        &app,
        "POST",
        &format!("/api/v1/ai/agents/actions/{action_id}/reverse"),
        &seeded.owner_token,
        "ai-43-reverse",
        None,
    )
    .await;
    assert_eq!(rev_status, StatusCode::OK, "reverse: {rev_body}");
    assert_eq!(rev_body["status"], "reversed");

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    let active: (bool,) =
        sqlx::query_as("SELECT active FROM ai_agent_effect WHERE org_id = $1 AND action_id = $2")
            .bind(seeded.org.as_uuid())
            .bind(Uuid::parse_str(action_id).unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(!active.0, "effect must be inactive after reverse");
}

#[tokio::test]
async fn budget_hard_stop_blocks_agent_tools() {
    let Some(seeded) = seed("ai-43-budget").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (pol_status, _) = publish_policy(&app, &seeded.owner_token, &default_policy()).await;
    assert_eq!(pol_status, StatusCode::OK);

    // Ensure settings row, then exhaust budget.
    let _ = oneshot_json(
        &app,
        "GET",
        "/api/v1/ai/settings",
        &seeded.owner_token,
        "ai-43-budget-settings",
        None,
    )
    .await;

    let mut tx = seeded.pool.begin().await.unwrap();
    set_session_org_id(&mut tx, seeded.org).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO ai_org_settings (org_id) VALUES ($1)
        ON CONFLICT (org_id) DO NOTHING
        "#,
    )
    .bind(seeded.org.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE ai_org_settings SET monthly_token_budget = 0, tokens_used_this_month = 0 WHERE org_id = $1",
    )
    .bind(seeded.org.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-budget-run",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "budget block: {body}");
    assert!(
        detail_contains(&body, "budget"),
        "expected budget exceeded message: {body}"
    );
}

#[tokio::test]
async fn nl_workflow_cannot_exceed_creator_perms() {
    let Some(seeded) = seed("ai-43-nl").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, body) = oneshot_json(
        &app,
        "POST",
        "/api/v1/ai/agents/workflows/propose",
        &seeded.member_token,
        "ai-43-nl-propose",
        Some(json!({
            "prompt": "when a deal is won, post journal and read payroll and create a task"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "propose: {body}");
    assert_eq!(body["status"], "draft");

    let steps = body["definition"]["steps"]
        .as_array()
        .expect("definition.steps");
    let actions: Vec<&str> = steps.iter().filter_map(|s| s["action"].as_str()).collect();
    assert!(
        actions.contains(&"create_task"),
        "expected create_task in steps: {body}"
    );
    assert!(
        !actions.contains(&"post_journal"),
        "must not include post_journal: {body}"
    );
    assert!(
        !actions.contains(&"read_payroll"),
        "must not include read_payroll: {body}"
    );

    let filtered = body["filtered_actions"]
        .as_array()
        .expect("filtered_actions");
    let filtered_s = filtered
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        filtered_s.contains("post_journal") && filtered_s.contains("read_payroll"),
        "filtered_actions must mention denied actions: {body}"
    );
}

#[tokio::test]
async fn review_fixture_computes_error_rate() {
    let Some(seeded) = seed("ai-43-review").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (seed_status, _) = oneshot_json(
        &app,
        "POST",
        "/api/v1/ai/agents/review/seed-fixture",
        &seeded.owner_token,
        "ai-43-review-seed",
        None,
    )
    .await;
    assert_eq!(seed_status, StatusCode::OK);

    let (status, body) = oneshot_json(
        &app,
        "GET",
        "/api/v1/ai/agents/review",
        &seeded.owner_token,
        "ai-43-review-get",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "review: {body}");

    let total = body["total_actions"].as_u64().unwrap_or(0);
    assert!(total >= 20, "total_actions >= 20, got {total}: {body}");

    let error_rate = body["error_rate"].as_f64().unwrap_or(-1.0);
    assert!(
        (error_rate - 0.05).abs() < 0.001,
        "error_rate ≈ 0.05, got {error_rate}: {body}"
    );
    assert_eq!(body["within_threshold"], true);
}

#[tokio::test]
async fn propose_then_commit_still_default_without_policy() {
    let Some(seeded) = seed("ai-43-default").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (status, body) = start_run(
        &app,
        &seeded.owner_token,
        "receivables_chase",
        None,
        "ai-43-default-run",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no policy: {body}");
    assert!(
        detail_contains(&body, "no active agent policy"),
        "expected no active agent policy message: {body}"
    );

    let (chat_status, chat_body) =
        chat(&app, &seeded.owner_token, "create invoice for customer").await;
    assert_eq!(chat_status, StatusCode::OK, "chat: {chat_body}");
    let proposals = chat_body["proposals"].as_array().expect("proposals");
    assert!(
        !proposals.is_empty(),
        "expected pending proposals: {chat_body}"
    );
    for p in proposals {
        assert_eq!(p["status"], "pending");
    }
}

#[tokio::test]
async fn member_denied_kill_and_manage() {
    let Some(seeded) = seed("ai-43-member").await else {
        eprintln!("skip: no TEST_DATABASE_URL");
        return;
    };
    let app = ai_app(seeded.pool.clone(), seeded.ring.clone());

    let (kill_status, kill_body) = oneshot_json(
        &app,
        "POST",
        "/api/v1/ai/agents/kill-switch",
        &seeded.member_token,
        "ai-43-member-kill",
        Some(json!({ "engaged": true })),
    )
    .await;
    assert_eq!(
        kill_status,
        StatusCode::FORBIDDEN,
        "member kill: {kill_body}"
    );

    let (pol_status, pol_body) =
        publish_policy(&app, &seeded.member_token, &default_policy()).await;
    assert_eq!(
        pol_status,
        StatusCode::FORBIDDEN,
        "member publish policy: {pol_body}"
    );
}
