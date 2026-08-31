//! Review checklist and publication gate.
//!
//! Publication is blocked until **every required checklist item is complete**
//! and `security_review_completed` is true. `security_review_completed` is
//! derived — never client-supplied — from the `security_*` checklist items, so
//! a publisher cannot flip the flag without doing the work.

use chrono::{DateTime, Utc};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::OrgId;
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::types::{ChecklistItem, ReviewDto};
use super::{conflict, forbidden, internal, not_found};

pub const REVIEW_PENDING: &str = "pending";
pub const REVIEW_IN_REVIEW: &str = "in_review";
pub const REVIEW_REJECTED: &str = "rejected";
pub const REVIEW_PUBLISHED: &str = "published";

const REVIEW_COLUMNS: &str = "id, org_id, listing_id, public_id, checklist, \
     security_review_completed, status, reviewer_notes, updated_at";

#[derive(Debug, Clone, FromRow)]
pub struct ReviewRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub listing_id: Uuid,
    pub public_id: String,
    pub checklist: serde_json::Value,
    pub security_review_completed: bool,
    pub status: String,
    pub reviewer_notes: String,
    pub updated_at: DateTime<Utc>,
}

impl ReviewRow {
    pub fn checklist(&self) -> Vec<ChecklistItem> {
        serde_json::from_value(self.checklist.clone()).unwrap_or_default()
    }

    pub fn to_dto(&self, listing_public_id: &str, listing_status: &str) -> ReviewDto {
        ReviewDto {
            id: self.public_id.clone(),
            listing_id: listing_public_id.to_string(),
            listing_status: listing_status.to_string(),
            checklist: self.checklist(),
            security_review_completed: self.security_review_completed,
            status: self.status.clone(),
            reviewer_notes: self.reviewer_notes.clone(),
            updated_at: self.updated_at,
        }
    }
}

/// The default checklist applied when a listing first enters review.
///
/// Contains three `security_*` items; all of them must be completed before
/// `security_review_completed` becomes true.
pub fn default_checklist() -> Vec<ChecklistItem> {
    [
        (
            "security_scope_review",
            "Requested scopes reviewed and minimised to least privilege",
            true,
        ),
        (
            "security_secret_handling",
            "Client secrets and app tokens are stored hashed and never logged",
            true,
        ),
        (
            "security_redirect_uris",
            "Redirect URIs are absolute HTTPS endpoints with no open redirect",
            true,
        ),
        (
            "listing_metadata",
            "Listing name, description and support contact are complete",
            true,
        ),
        (
            "data_retention",
            "Data retention and deletion behaviour is documented",
            false,
        ),
    ]
    .into_iter()
    .map(|(id, label, required)| ChecklistItem {
        id: id.to_string(),
        label: label.to_string(),
        required,
        completed: false,
        completed_by: None,
        completed_at: None,
    })
    .collect()
}

/// Derived security gate: every `security_*` item is complete (and at least one exists).
pub fn security_review_complete(items: &[ChecklistItem]) -> bool {
    let mut security_items = items.iter().filter(|i| i.is_security()).peekable();
    security_items.peek().is_some() && items.iter().filter(|i| i.is_security()).all(|i| i.completed)
}

/// Publication gate: required items complete **and** security review complete.
pub fn publishable(items: &[ChecklistItem], security_completed: bool) -> Result<(), String> {
    let outstanding: Vec<&str> = items
        .iter()
        .filter(|i| i.required && !i.completed)
        .map(|i| i.id.as_str())
        .collect();
    if !outstanding.is_empty() {
        return Err(format!(
            "required checklist items incomplete: {}",
            outstanding.join(", ")
        ));
    }
    if !security_completed {
        return Err("security review is not complete".to_string());
    }
    Ok(())
}

/// Fetch the review row for a listing, creating it with the default checklist.
pub async fn ensure_review(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ReviewRow, AppError> {
    if let Some(row) = fetch_optional(tx, org_id, listing_id, request_id).await? {
        return Ok(row);
    }
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceReview, id).as_str();
    sqlx::query(
        r#"
        INSERT INTO marketplace_review (
            id, org_id, listing_id, public_id, checklist, security_review_completed, status
        ) VALUES ($1,$2,$3,$4,$5,false,$6)
        ON CONFLICT (listing_id) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(listing_id)
    .bind(&public_id)
    .bind(json!(default_checklist()))
    .bind(REVIEW_PENDING)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    fetch_optional(tx, org_id, listing_id, request_id)
        .await?
        .ok_or_else(|| not_found(request_id, "review"))
}

pub async fn fetch_optional(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    request_id: &str,
) -> Result<Option<ReviewRow>, AppError> {
    sqlx::query_as(&format!(
        "SELECT {REVIEW_COLUMNS} FROM marketplace_review WHERE org_id = $1 AND listing_id = $2"
    ))
    .bind(org_id.as_uuid())
    .bind(listing_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))
}

/// Mark checklist items complete and recompute the derived security gate.
pub async fn complete_items(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    completed_item_ids: &[String],
    reviewer_notes: Option<&str>,
    reviewer: Uuid,
    request_id: &str,
) -> Result<ReviewRow, AppError> {
    let review = ensure_review(tx, org_id, listing_id, request_id).await?;
    let mut items = review.checklist();

    for wanted in completed_item_ids {
        if !items.iter().any(|i| &i.id == wanted) {
            return Err(super::validation(
                request_id,
                format!("unknown checklist item {wanted}"),
            ));
        }
    }

    let now = Utc::now();
    let reviewer_public = PublicId::new(IdKind::User, reviewer).as_str();
    for item in items.iter_mut() {
        if completed_item_ids.iter().any(|id| id == &item.id) && !item.completed {
            item.completed = true;
            item.completed_by = Some(reviewer_public.clone());
            item.completed_at = Some(now);
        }
    }

    let security_completed = security_review_complete(&items);

    sqlx::query(
        r#"
        UPDATE marketplace_review
        SET checklist = $3,
            security_review_completed = $4,
            status = $5,
            reviewer_notes = COALESCE($6, reviewer_notes),
            updated_at = now()
        WHERE org_id = $1 AND listing_id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(listing_id)
    .bind(json!(items))
    .bind(security_completed)
    .bind(REVIEW_IN_REVIEW)
    .bind(reviewer_notes)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    fetch_optional(tx, org_id, listing_id, request_id)
        .await?
        .ok_or_else(|| not_found(request_id, "review"))
}

/// Assert the listing may be published, or explain why not.
pub async fn assert_publishable(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ReviewRow, AppError> {
    let review = ensure_review(tx, org_id, listing_id, request_id).await?;
    publishable(&review.checklist(), review.security_review_completed)
        .map_err(|reason| forbidden(request_id, format!("cannot publish: {reason}")))?;
    Ok(review)
}

pub async fn set_status(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    status: &str,
    reviewer_notes: Option<&str>,
    request_id: &str,
) -> Result<ReviewRow, AppError> {
    sqlx::query(
        "UPDATE marketplace_review SET status = $3, \
         reviewer_notes = COALESCE($4, reviewer_notes), updated_at = now() \
         WHERE org_id = $1 AND listing_id = $2",
    )
    .bind(org_id.as_uuid())
    .bind(listing_id)
    .bind(status)
    .bind(reviewer_notes)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;
    fetch_optional(tx, org_id, listing_id, request_id)
        .await?
        .ok_or_else(|| conflict(request_id, "review row missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_checklist_has_at_least_two_security_items() {
        let items = default_checklist();
        let security = items.iter().filter(|i| i.is_security()).count();
        assert!(security >= 2, "expected >= 2 security items, got {security}");
        assert!(items.iter().filter(|i| i.is_security()).all(|i| i.required));
    }

    #[test]
    fn publish_blocked_until_security_items_complete() {
        let mut items = default_checklist();
        // Complete only the non-security required items.
        for item in items.iter_mut().filter(|i| !i.is_security()) {
            item.completed = true;
        }
        assert!(!security_review_complete(&items));
        assert!(publishable(&items, security_review_complete(&items)).is_err());

        for item in items.iter_mut() {
            item.completed = true;
        }
        assert!(security_review_complete(&items));
        assert!(publishable(&items, security_review_complete(&items)).is_ok());
    }

    #[test]
    fn security_flag_alone_does_not_bypass_required_items() {
        let items = default_checklist();
        assert!(publishable(&items, true).is_err());
    }

    #[test]
    fn empty_checklist_is_never_security_complete() {
        assert!(!security_review_complete(&[]));
    }
}
