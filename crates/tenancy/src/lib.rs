//! Tenancy primitives: every tenant-owned path requires an `OrgId`.
//!
//! Request context **cannot** be built without `org_id`. When talking to
//! PostgreSQL, callers must set `app.org_id` for RLS.

use std::fmt;

use companyos_ids::{IdKind, PublicId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Strongly-typed organization identifier (internal UUIDv7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrgId(pub Uuid);

impl OrgId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn generate() -> Self {
        Self(companyos_ids::new_uuid_v7())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    pub fn to_public(&self) -> PublicId {
        PublicId::new(IdKind::Org, self.0)
    }

    pub fn from_public(id: &PublicId) -> Result<Self, TenancyError> {
        if id.kind() != IdKind::Org {
            return Err(TenancyError::NotAnOrgId);
        }
        Ok(Self(id.uuid()))
    }
}

impl fmt::Display for OrgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_public())
    }
}

/// Actor performing a request (human or AI-on-behalf-of-human).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub user_id: Uuid,
    /// When AI acts, this is the human it acts on behalf of (same as user_id in v1 propose-then-commit).
    pub on_behalf_of: Uuid,
    pub is_ai: bool,
}

impl Actor {
    pub fn human(user_id: Uuid) -> Self {
        Self {
            user_id,
            on_behalf_of: user_id,
            is_ai: false,
        }
    }

    pub fn ai_on_behalf_of(ai_user_id: Uuid, human_id: Uuid) -> Self {
        Self {
            user_id: ai_user_id,
            on_behalf_of: human_id,
            is_ai: true,
        }
    }
}

/// Request-scoped tenancy + actor context.
///
/// Construction requires `org_id` — there is no `Default` or empty builder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestContext {
    pub org_id: OrgId,
    pub actor: Actor,
    pub request_id: String,
}

impl RequestContext {
    pub fn new(org_id: OrgId, actor: Actor, request_id: impl Into<String>) -> Self {
        Self {
            org_id,
            actor,
            request_id: request_id.into(),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TenancyError {
    #[error("org_id is required")]
    MissingOrgId,
    #[error("public id is not an org_ id")]
    NotAnOrgId,
    #[error("failed to bind postgres session org_id: {0}")]
    SessionBind(String),
}

/// Set PostgreSQL session variable used by RLS policies: `app.org_id`.
///
/// **Must be called inside an open transaction.** Uses `set_config(..., is_local=true)`
/// (equivalent to `SET LOCAL`) so the binding lasts for the remainder of the transaction only.
pub async fn set_session_org_id(
    conn: &mut sqlx::PgConnection,
    org_id: OrgId,
) -> Result<(), TenancyError> {
    sqlx::query("SELECT set_config('app.org_id', $1, true)")
        .bind(org_id.as_uuid().to_string())
        .execute(&mut *conn)
        .await
        .map_err(|e| TenancyError::SessionBind(e.to_string()))?;
    Ok(())
}

/// Allow auth-service to list memberships for a user across orgs (RLS bypass key).
///
/// **Must be called inside an open transaction.** Pair with clearing after use.
pub async fn set_auth_lookup_user(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> Result<(), TenancyError> {
    sqlx::query("SELECT set_config('app.auth_lookup_user', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(|e| TenancyError::SessionBind(e.to_string()))?;
    Ok(())
}

/// Allow invitation accept to look up a row by token hash (RLS bypass key).
pub async fn set_invite_token_hash(
    conn: &mut sqlx::PgConnection,
    token_hash: &str,
) -> Result<(), TenancyError> {
    sqlx::query("SELECT set_config('app.invite_token_hash', $1, true)")
        .bind(token_hash)
        .execute(&mut *conn)
        .await
        .map_err(|e| TenancyError::SessionBind(e.to_string()))?;
    Ok(())
}

/// Clear session org (tests / connection reuse). Must be inside a transaction.
pub async fn clear_session_org_id(conn: &mut sqlx::PgConnection) -> Result<(), TenancyError> {
    sqlx::query("SELECT set_config('app.org_id', '', true)")
        .execute(&mut *conn)
        .await
        .map_err(|e| TenancyError::SessionBind(e.to_string()))?;
    Ok(())
}

/// Shared Postgres advisory-lock key for schema migrations.
///
/// `cargo test --workspace` runs many integration-test binaries in parallel
/// against one DB; without serialization, concurrent DDL races produce flaky
/// 500s on register/migrate. Production multi-process startup is also safe.
pub const SCHEMA_MIGRATION_LOCK_KEY: i64 = 0x0043_4F53_4D49_4701; // "COSMIG\x01"

/// Run `f` while holding the shared schema-migration advisory lock.
///
/// Uses `pg_try_advisory_lock` and releases the pool connection while waiting,
/// so the lock holder can still check out other connections for DDL (avoids
/// pool-exhaustion deadlock when many tests contend).
pub async fn with_schema_migration_lock<F, Fut, T, E>(pool: &sqlx::PgPool, f: F) -> Result<T, E>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: From<sqlx::Error>,
{
    // Serialize waiters inside this process before touching Postgres.
    static LOCAL: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let local = LOCAL.get_or_init(|| tokio::sync::Mutex::new(()));
    let _local_guard = local.lock().await;

    let mut lock_conn = loop {
        let mut conn = pool.acquire().await?;
        let locked: (bool,) = sqlx::query_as("SELECT pg_try_advisory_lock($1)")
            .bind(SCHEMA_MIGRATION_LOCK_KEY)
            .fetch_one(&mut *conn)
            .await?;
        if locked.0 {
            break conn;
        }
        drop(conn);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    };

    let result = f().await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SCHEMA_MIGRATION_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await;
    result
}

/// Acquire the shared schema-migration advisory lock (session-level).
///
/// Prefer [`with_schema_migration_lock`] — this pair is only safe when lock and
/// unlock use the same checked-out connection.
pub async fn acquire_schema_migration_lock(
    conn: &mut sqlx::PgConnection,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SCHEMA_MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Release the shared schema-migration advisory lock on the same connection.
pub async fn release_schema_migration_lock(
    conn: &mut sqlx::PgConnection,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SCHEMA_MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_requires_org_id() {
        let org = OrgId::generate();
        let actor = Actor::human(companyos_ids::new_uuid_v7());
        let ctx = RequestContext::new(org, actor, "req_1");
        assert_eq!(ctx.org_id, org);
        assert!(!ctx.request_id.is_empty());
    }

    #[test]
    fn org_id_public_round_trip() {
        let org = OrgId::generate();
        let pub_id = org.to_public();
        assert!(pub_id.as_str().starts_with("org_"));
        let back = OrgId::from_public(&pub_id).unwrap();
        assert_eq!(back, org);
    }

    #[test]
    fn from_public_rejects_non_org() {
        let usr = PublicId::generate(IdKind::User);
        assert_eq!(OrgId::from_public(&usr), Err(TenancyError::NotAnOrgId));
    }

    #[test]
    fn ai_actor_records_human() {
        let human = companyos_ids::new_uuid_v7();
        let ai = companyos_ids::new_uuid_v7();
        let actor = Actor::ai_on_behalf_of(ai, human);
        assert!(actor.is_ai);
        assert_eq!(actor.on_behalf_of, human);
    }

    #[test]
    fn serde_org_id() {
        let org = OrgId::generate();
        let json = serde_json::to_string(&org).unwrap();
        let back: OrgId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, org);
    }
}
