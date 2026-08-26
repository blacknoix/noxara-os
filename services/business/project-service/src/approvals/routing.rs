//! Policy match + assignee resolution for approval routing.

use companyos_ids::{IdKind, PublicId};
use uuid::Uuid;

use super::types::{
    ApprovalMode, CreateApprovalRequest, PolicyDefinition, PolicyMatch, ResolvedStepSnapshot,
    RoutingSnapshot,
};

#[derive(Debug, Clone)]
pub struct RouteContext {
    pub amount_minor: Option<i64>,
    pub discount_bps: Option<i64>,
    pub category: Option<String>,
    pub department_id: Option<Uuid>,
    pub requester_role: Option<String>,
}

impl RouteContext {
    pub fn from_request(req: &CreateApprovalRequest, discount_bps: Option<i64>) -> Self {
        let department_id = req
            .department_id
            .as_deref()
            .and_then(|s| s.parse::<PublicId>().ok())
            .filter(|p| p.kind() == IdKind::Department)
            .map(|p| p.uuid())
            .or_else(|| {
                req.department_id
                    .as_deref()
                    .and_then(|s| Uuid::parse_str(s).ok())
            });
        Self {
            amount_minor: req.amount_minor,
            discount_bps,
            category: req.category.clone(),
            department_id,
            requester_role: req.requester_role.clone(),
        }
    }
}

pub fn matches_criteria(m: &PolicyMatch, ctx: &RouteContext) -> bool {
    if let Some(gte) = m.amount_minor_gte {
        if ctx.amount_minor.unwrap_or(0) < gte {
            return false;
        }
    }
    if let Some(lt) = m.amount_minor_lt {
        if ctx.amount_minor.unwrap_or(0) >= lt {
            return false;
        }
    }
    if let Some(bps) = m.discount_bps_gte {
        if ctx.discount_bps.unwrap_or(0) < bps {
            return false;
        }
    }
    if !m.categories.is_empty() {
        let Some(cat) = ctx.category.as_deref() else {
            return false;
        };
        if !m.categories.iter().any(|c| c.eq_ignore_ascii_case(cat)) {
            return false;
        }
    }
    if !m.department_ids.is_empty() {
        let Some(dep) = ctx.department_id else {
            return false;
        };
        let dep_str = dep.to_string();
        let dep_pub = PublicId::new(IdKind::Department, dep).as_str();
        if !m
            .department_ids
            .iter()
            .any(|d| d == &dep_str || d == &dep_pub)
        {
            return false;
        }
    }
    if !m.requester_roles.is_empty() {
        let Some(role) = ctx.requester_role.as_deref() else {
            return false;
        };
        if !m
            .requester_roles
            .iter()
            .any(|r| r.eq_ignore_ascii_case(role))
        {
            return false;
        }
    }
    true
}

/// Resolve step assignees: explicit user ids, else members with the role key.
pub async fn resolve_step_assignees(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    step: &super::types::PolicyStepDef,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut ids: Vec<Uuid> = Vec::new();
    for raw in &step.approver_user_ids {
        if let Ok(u) = Uuid::parse_str(raw) {
            ids.push(u);
        } else if let Ok(pid) = raw.parse::<PublicId>() {
            if pid.kind() == IdKind::User {
                ids.push(pid.uuid());
            }
        }
    }
    if ids.is_empty() {
        if let Some(role_key) = step.approver_role.as_deref() {
            let rows: Vec<(Uuid,)> = sqlx::query_as(
                r#"
                SELECT m.user_id
                FROM membership m
                JOIN org_role r ON r.id = m.role_id
                WHERE m.org_id = $1
                  AND m.revoked_at IS NULL
                  AND lower(r.system_key) = lower($2)
                "#,
            )
            .bind(org_id)
            .bind(role_key)
            .fetch_all(&mut **tx)
            .await?;
            ids.extend(rows.into_iter().map(|(u,)| u));
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub fn build_rationale(
    policy_name: &str,
    version: i32,
    mode: ApprovalMode,
    def: &PolicyDefinition,
    ctx: &RouteContext,
) -> String {
    let mut parts = vec![format!(
        "Routed by policy \"{policy_name}\" v{version} ({})",
        mode.as_str()
    )];
    if let Some(a) = ctx.amount_minor {
        parts.push(format!("amount_minor={a}"));
    }
    if let Some(b) = ctx.discount_bps {
        parts.push(format!("discount_bps={b}"));
    }
    if let Some(c) = &ctx.category {
        parts.push(format!("category={c}"));
    }
    if let Some(r) = &ctx.requester_role {
        parts.push(format!("requester_role={r}"));
    }
    if !def.match_criteria.categories.is_empty()
        || def.match_criteria.amount_minor_gte.is_some()
        || def.match_criteria.discount_bps_gte.is_some()
    {
        parts.push("matched policy criteria".into());
    }
    parts.join("; ")
}

pub async fn build_routing_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    policy_public_id: &str,
    policy_name: &str,
    policy_version: i32,
    def: &PolicyDefinition,
    ctx: &RouteContext,
) -> Result<RoutingSnapshot, sqlx::Error> {
    let mut steps = Vec::new();
    let mut ordered = def.steps.clone();
    ordered.sort_by_key(|s| s.order);
    for step in &ordered {
        let assignees = resolve_step_assignees(tx, org_id, step).await?;
        steps.push(ResolvedStepSnapshot {
            order: step.order,
            approver_role: step.approver_role.clone(),
            assignee_user_ids: assignees,
            sla_seconds: step.sla_seconds,
            escalate_to_role: step.escalate_to_role.clone(),
        });
    }
    Ok(RoutingSnapshot {
        policy_public_id: policy_public_id.to_string(),
        policy_name: policy_name.to_string(),
        policy_version,
        mode: def.mode,
        match_criteria: def.match_criteria.clone(),
        steps,
        rationale: build_rationale(policy_name, policy_version, def.mode, def, ctx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::types::PolicyMatch;

    #[test]
    fn amount_and_role_match() {
        let m = PolicyMatch {
            amount_minor_gte: Some(10_000),
            requester_roles: vec!["member".into()],
            ..Default::default()
        };
        let ctx = RouteContext {
            amount_minor: Some(15_000),
            discount_bps: None,
            category: None,
            department_id: None,
            requester_role: Some("member".into()),
        };
        assert!(matches_criteria(&m, &ctx));
        let ctx2 = RouteContext {
            amount_minor: Some(5_000),
            ..ctx.clone()
        };
        assert!(!matches_criteria(&m, &ctx2));
    }
}
