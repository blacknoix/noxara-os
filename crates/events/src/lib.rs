//! Domain event envelope and NATS subject naming.
//!
//! Subject format: `companyos.{org_id}.{context}.{aggregate}.{event}.v{n}`
//! where `org_id` is the public prefixed id (`org_…`).

use chrono::{DateTime, Utc};
use companyos_ids::new_uuid_v7;
use companyos_tenancy::{Actor, OrgId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Bounded contexts that own events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Workspace,
    Sales,
    Finance,
    Operations,
    People,
    Ai,
    Admin,
    /// Phase 0 hello slice lives under workspace for subject stability.
    Core,
    /// Identity & authentication events.
    Auth,
    /// Inventory & procurement (Phase 2.5) events.
    Inventory,
    /// Configurable workflow engine (Phase 3.1) lifecycle events.
    Workflow,
    /// Analytics & reporting (Phase 3.2) lifecycle events.
    Analytics,
}

impl Context {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Sales => "sales",
            Self::Finance => "finance",
            Self::Operations => "operations",
            Self::People => "people",
            Self::Ai => "ai",
            Self::Admin => "admin",
            Self::Core => "core",
            Self::Auth => "auth",
            Self::Inventory => "inventory",
            Self::Workflow => "workflow",
            Self::Analytics => "analytics",
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EventError {
    #[error("invalid event subject: {0}")]
    InvalidSubject(String),
}

/// Build a JetStream / NATS subject.
pub fn event_subject(
    org_id: OrgId,
    context: Context,
    aggregate: &str,
    event: &str,
    version: u32,
) -> String {
    format!(
        "companyos.{}.{}.{}.{}.v{version}",
        org_id.to_public(),
        context.as_str(),
        aggregate,
        event,
    )
}

/// CloudEvents-ish envelope used on the wire and in the outbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub subject: String,
    pub org_id: OrgId,
    pub context: Context,
    pub aggregate: String,
    pub event_type: String,
    pub version: u32,
    pub occurred_at: DateTime<Utc>,
    pub actor: Actor,
    pub payload: serde_json::Value,
    /// Consumer idempotency key (defaults to event_id string).
    pub idempotency_key: String,
}

impl EventEnvelope {
    pub fn new(
        org_id: OrgId,
        context: Context,
        aggregate: impl Into<String>,
        event_type: impl Into<String>,
        version: u32,
        actor: Actor,
        payload: serde_json::Value,
    ) -> Self {
        let aggregate = aggregate.into();
        let event_type = event_type.into();
        let event_id = new_uuid_v7();
        let subject = event_subject(org_id, context, &aggregate, &event_type, version);
        Self {
            event_id,
            subject,
            org_id,
            context,
            aggregate,
            event_type,
            version,
            occurred_at: Utc::now(),
            actor,
            payload,
            idempotency_key: event_id.to_string(),
        }
    }
}

/// Parse and lightly validate a subject string.
pub fn parse_subject(subject: &str) -> Result<(String, String, String, String, u32), EventError> {
    let parts: Vec<&str> = subject.split('.').collect();
    // companyos / org_… / context / aggregate / event / vN
    if parts.len() != 6 || parts[0] != "companyos" {
        return Err(EventError::InvalidSubject(subject.to_string()));
    }
    if !parts[1].starts_with("org_") {
        return Err(EventError::InvalidSubject(subject.to_string()));
    }
    let version = parts[5]
        .strip_prefix('v')
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| EventError::InvalidSubject(subject.to_string()))?;
    Ok((
        parts[1].to_string(),
        parts[2].to_string(),
        parts[3].to_string(),
        parts[4].to_string(),
        version,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_tenancy::Actor;

    #[test]
    fn subject_format() {
        let org = OrgId::generate();
        let sub = event_subject(org, Context::Core, "hello", "created", 1);
        assert!(sub.starts_with("companyos.orga_") || sub.starts_with("companyos.org_"));
        let expected_prefix = format!("companyos.{}", org.to_public());
        assert!(sub.starts_with(&expected_prefix));
        assert!(sub.ends_with(".core.hello.created.v1"));
        let parsed = parse_subject(&sub).unwrap();
        assert_eq!(parsed.0, org.to_public().as_str());
        assert_eq!(parsed.1, "core");
        assert_eq!(parsed.2, "hello");
        assert_eq!(parsed.3, "created");
        assert_eq!(parsed.4, 1);
    }

    #[test]
    fn envelope_includes_org_and_actor() {
        let org = OrgId::generate();
        let actor = Actor::human(new_uuid_v7());
        let env = EventEnvelope::new(
            org,
            Context::Core,
            "hello",
            "created",
            1,
            actor.clone(),
            serde_json::json!({"message": "hi"}),
        );
        assert_eq!(env.org_id, org);
        assert_eq!(env.actor, actor);
        assert!(env.subject.contains("hello.created.v1"));
        assert!(!env.idempotency_key.is_empty());
    }

    #[test]
    fn parse_rejects_bad_subjects() {
        assert!(parse_subject("foo").is_err());
        assert!(parse_subject("companyos.notorg.core.hello.created.v1").is_err());
    }
}
