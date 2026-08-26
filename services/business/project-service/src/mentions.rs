//! @mention parsing and authz-gated notification intents.
//!
//! A user is NEVER notified about a record they cannot see. We resolve each
//! `@usr_…` mention, load that user's principal, and only insert an
//! `operations_notification_intent` when `operations.task.read` is allowed
//! at the scope required for the task row.

use companyos_authz::{decide_with_scope, Decision, PermissionId, Principal, Scope};
use companyos_ids::{IdKind, PublicId};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::principal::{load_membership_scope, required_scope_for_owner_row};

fn mention_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@((?:usr_)?[0-9a-fA-F-]{36})").expect("mention regex"))
}

/// Extract unique user UUIDs from `@usr_<uuid>` / `@<uuid>` mentions in `body`.
pub fn parse_mention_user_ids(body: &str) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for cap in mention_re().captures_iter(body) {
        let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let uid = if let Ok(u) = Uuid::parse_str(raw) {
            Some(u)
        } else if let Ok(pid) = raw.parse::<PublicId>() {
            if pid.kind() == IdKind::User {
                Some(pid.uuid())
            } else {
                None
            }
        } else {
            None
        };
        if let Some(u) = uid {
            if seen.insert(u) {
                out.push(u);
            }
        }
    }
    out
}

/// Whether `principal` may read a task owned by `owner_user_id` / assigned to
/// `assignee_id` given their membership scope.
pub fn can_read_task(
    principal: &Principal,
    permission: &PermissionId,
    required: Scope,
) -> bool {
    decide_with_scope(principal, permission, required).decision == Decision::Allow
}

/// Resolve allowed mention recipients for a task. Unauthorized users are
/// dropped (not recorded).
#[allow(clippy::too_many_arguments)]
pub async fn filter_mention_recipients(
    pool: &sqlx::PgPool,
    org_id: companyos_tenancy::OrgId,
    actor_user_id: Uuid,
    task_owner_user_id: Uuid,
    task_assignee_id: Option<Uuid>,
    candidate_user_ids: &[Uuid],
    request_id: &str,
) -> Result<Vec<Uuid>, companyos_errors::AppError> {
    let mut allowed = Vec::new();
    for uid in candidate_user_ids {
        if *uid == actor_user_id {
            continue;
        }
        let Ok(scope) = load_membership_scope(pool, org_id, *uid, request_id).await else {
            // No membership → cannot see org records.
            continue;
        };
        let mut tx = pool.begin().await.map_err(|e| {
            companyos_errors::AppError::new(
                companyos_errors::ErrorCode::Internal,
                request_id,
                e.to_string(),
            )
        })?;
        companyos_tenancy::set_session_org_id(&mut tx, org_id)
            .await
            .map_err(|e| {
                companyos_errors::AppError::new(
                    companyos_errors::ErrorCode::Internal,
                    request_id,
                    e.to_string(),
                )
            })?;
        // Visibility: treat assignee as co-owner for "own" when comparing.
        let owner_for_scope = task_assignee_id.unwrap_or(task_owner_user_id);
        let required = required_scope_for_owner_row(
            &mut tx,
            org_id.as_uuid(),
            *uid,
            scope.team_id,
            scope.department_id,
            Some(if *uid == task_owner_user_id || Some(*uid) == task_assignee_id {
                *uid
            } else {
                owner_for_scope
            }),
        )
        .await
        .map_err(|e| {
            companyos_errors::AppError::new(
                companyos_errors::ErrorCode::Internal,
                request_id,
                e.to_string(),
            )
        })?;
        let _ = tx.commit().await;
        if can_read_task(
            &scope.principal,
            &companyos_authz::perms::operations_task_read(),
            required,
        ) {
            allowed.push(*uid);
        }
    }
    Ok(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usr_prefixed_and_bare_uuids() {
        let u = Uuid::parse_str("01900000-0000-7000-8000-000000000001").unwrap();
        let body = format!("hey @usr_{u} and also @{u} thanks");
        let ids = parse_mention_user_ids(&body);
        assert_eq!(ids, vec![u]);
    }

    #[test]
    fn ignores_unknown_prefixes() {
        let body = "ping @org_01900000-0000-7000-8000-000000000001";
        assert!(parse_mention_user_ids(body).is_empty());
    }
}
