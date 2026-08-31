//! Installs and consent.
//!
//! There is exactly one install path. First-party connectors reached through
//! `/api/v1/integrations/{connector_key}/connect` and third-party apps reached
//! through `/api/v1/marketplace/installs` or the OAuth code exchange all call
//! [`create_install`], write the same `marketplace_install` row and mint tokens
//! through [`crate::marketplace::tokens::issue_tokens`]. `listing_kind` is
//! copied onto the install as data and is never branched on here.

use chrono::{DateTime, Utc};
use companyos_authz::{is_allowed, PermissionId, Principal};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{Actor, OrgId};
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::listings::ListingRow;
use super::tokens::{issue_tokens, revoke_install_tokens};
use super::types::{
    InstallDto, IntegrationDto, TokenPair, INSTALL_ACTIVE, INSTALL_REVOKED,
};
use super::{
    conflict, emit_event, forbidden, internal, not_found, org_public, string_array, validation,
};

const INSTALL_COLUMNS: &str = "id, org_id, public_id, listing_id, listing_public_id, listing_slug, \
     listing_name, listing_kind, connector_key, consented_scopes, status, installed_by, \
     installed_at, revoked_at, revoked_by, outbound_enabled, inbound_enabled, last_error";

#[derive(Debug, Clone, FromRow)]
pub struct InstallRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub public_id: String,
    pub listing_id: Uuid,
    pub listing_public_id: String,
    pub listing_slug: String,
    pub listing_name: String,
    pub listing_kind: String,
    pub connector_key: Option<String>,
    pub consented_scopes: serde_json::Value,
    pub status: String,
    pub installed_by: Uuid,
    pub installed_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
    pub outbound_enabled: bool,
    pub inbound_enabled: bool,
    pub last_error: Option<String>,
}

impl InstallRow {
    pub fn consented_scopes(&self) -> Vec<String> {
        string_array(&self.consented_scopes)
    }

    pub fn to_dto(&self) -> InstallDto {
        InstallDto {
            id: self.public_id.clone(),
            org_id: org_public(self.org_id),
            listing_id: self.listing_public_id.clone(),
            listing_slug: self.listing_slug.clone(),
            listing_name: self.listing_name.clone(),
            listing_kind: self.listing_kind.clone(),
            connector_key: self.connector_key.clone(),
            consented_scopes: self.consented_scopes(),
            status: self.status.clone(),
            outbound_enabled: self.outbound_enabled,
            inbound_enabled: self.inbound_enabled,
            last_error: self.last_error.clone(),
            installed_at: self.installed_at,
            revoked_at: self.revoked_at,
        }
    }

    /// Integrations-UI projection; only installs with a connector key qualify.
    pub fn to_integration_dto(&self) -> Option<IntegrationDto> {
        let connector_key = self.connector_key.clone()?;
        Some(IntegrationDto {
            connector_key,
            install_id: self.public_id.clone(),
            name: self.listing_name.clone(),
            status: self.status.clone(),
            scopes: self.consented_scopes(),
            outbound_enabled: self.outbound_enabled,
            inbound_enabled: self.inbound_enabled,
            last_error: self.last_error.clone(),
            installed_at: self.installed_at,
        })
    }
}

/// Consent rule: `consented ⊆ listing.requested_scopes ∩ installer permissions`.
///
/// Returns the de-duplicated consented set in listing order.
pub fn validate_consent(
    listing: &ListingRow,
    consented: &[String],
    principal: &Principal,
    request_id: &str,
) -> Result<Vec<String>, AppError> {
    let requested = listing.requested_scopes();
    let mut resolved: Vec<String> = Vec::new();
    for scope in consented {
        if !requested.iter().any(|r| r == scope) {
            return Err(validation(
                request_id,
                format!("scope {scope} is not requested by listing {}", listing.public_id),
            ));
        }
        if !is_allowed(principal, &PermissionId::from(scope.as_str())) {
            return Err(forbidden(
                request_id,
                format!("installer cannot grant {scope}: it exceeds their own permissions"),
            ));
        }
        if !resolved.iter().any(|r| r == scope) {
            resolved.push(scope.clone());
        }
    }
    if resolved.is_empty() {
        return Err(validation(
            request_id,
            "consented_scopes must contain at least one scope",
        ));
    }
    Ok(resolved)
}

/// The widest consent this principal may grant for this listing.
pub fn default_consent(listing: &ListingRow, principal: &Principal) -> Vec<String> {
    listing
        .requested_scopes()
        .into_iter()
        .filter(|s| is_allowed(principal, &PermissionId::from(s.as_str())))
        .collect()
}

pub async fn fetch(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    request_id: &str,
) -> Result<InstallRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {INSTALL_COLUMNS} FROM marketplace_install WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id.as_uuid())
    .bind(install_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "install"))
}

pub async fn fetch_active_by_connector(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    connector_key: &str,
    request_id: &str,
) -> Result<InstallRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {INSTALL_COLUMNS} FROM marketplace_install \
         WHERE org_id = $1 AND connector_key = $2 AND status = 'active'"
    ))
    .bind(org_id.as_uuid())
    .bind(connector_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "integration install"))
}

pub async fn list(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<InstallRow>, AppError> {
    sqlx::query_as(&format!(
        "SELECT {INSTALL_COLUMNS} FROM marketplace_install WHERE org_id = $1 \
         ORDER BY installed_at DESC LIMIT 500"
    ))
    .bind(org_id.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))
}

/// Create an install and mint its first token pair.
///
/// The single shared install path — see the module docs. `consented` must have
/// already passed [`validate_consent`]; the subset check is repeated here as
/// defence in depth because OAuth code exchange replays stored consent.
pub async fn create_install(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    installed_by: Uuid,
    actor: Actor,
    listing: &ListingRow,
    consented: &[String],
    request_id: &str,
) -> Result<(InstallRow, TokenPair), AppError> {
    let requested = listing.requested_scopes();
    for scope in consented {
        if !requested.iter().any(|r| r == scope) {
            return Err(validation(
                request_id,
                format!("scope {scope} is not requested by listing {}", listing.public_id),
            ));
        }
    }
    if consented.is_empty() {
        return Err(validation(request_id, "consent must include at least one scope"));
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceInstall, id).as_str();

    let inserted = sqlx::query(
        r#"
        INSERT INTO marketplace_install (
            id, org_id, public_id, listing_id, listing_public_id, listing_slug, listing_name,
            listing_kind, connector_key, consented_scopes, status, installed_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(listing.id)
    .bind(&listing.public_id)
    .bind(&listing.slug)
    .bind(&listing.name)
    .bind(&listing.listing_kind)
    .bind(listing.connector_key.as_deref())
    .bind(json!(consented))
    .bind(INSTALL_ACTIVE)
    .bind(installed_by)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    if inserted.rows_affected() == 0 {
        return Err(conflict(
            request_id,
            format!("{} is already installed", listing.public_id),
        ));
    }

    let tokens = issue_tokens(tx, org_id, id, consented, request_id).await?;
    let install = fetch(tx, org_id, id, request_id).await?;

    emit_event(
        tx,
        org_id,
        actor.clone(),
        "install_created",
        json!({
            "install_id": install.public_id,
            "listing_id": listing.public_id,
            "listing_kind": listing.listing_kind,
            "connector_key": listing.connector_key,
            "consented_scopes": consented,
        }),
        request_id,
    )
    .await?;
    emit_event(
        tx,
        org_id,
        actor,
        "oauth_token_issued",
        json!({
            "install_id": install.public_id,
            "listing_id": listing.public_id,
            "scopes": consented,
            "reason": "install",
        }),
        request_id,
    )
    .await?;

    Ok((install, tokens))
}

/// Uninstall: revoke the install, kill every token, and close both directions.
pub async fn revoke_install(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    revoked_by: Uuid,
    actor: Actor,
    request_id: &str,
) -> Result<InstallRow, AppError> {
    let existing = fetch(tx, org_id, install_id, request_id).await?;
    if existing.status == INSTALL_REVOKED {
        return Ok(existing);
    }

    sqlx::query(
        r#"
        UPDATE marketplace_install
        SET status = $3,
            revoked_at = now(),
            revoked_by = $4,
            outbound_enabled = false,
            inbound_enabled = false
        WHERE org_id = $1 AND id = $2
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(install_id)
    .bind(INSTALL_REVOKED)
    .bind(revoked_by)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let revoked_tokens = revoke_install_tokens(tx, org_id, install_id, request_id).await?;
    let install = fetch(tx, org_id, install_id, request_id).await?;

    emit_event(
        tx,
        org_id,
        actor,
        "install_revoked",
        json!({
            "install_id": install.public_id,
            "listing_id": install.listing_public_id,
            "connector_key": install.connector_key,
            "revoked_tokens": revoked_tokens,
        }),
        request_id,
    )
    .await?;

    Ok(install)
}

/// Re-consent: replace the consented set and rotate tokens to match it.
pub async fn reconsent(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    install_id: Uuid,
    consented: &[String],
    actor: Actor,
    request_id: &str,
) -> Result<(InstallRow, TokenPair), AppError> {
    let existing = fetch(tx, org_id, install_id, request_id).await?;
    if existing.status != INSTALL_ACTIVE {
        return Err(conflict(
            request_id,
            "cannot re-consent a revoked install — install again",
        ));
    }

    sqlx::query(
        "UPDATE marketplace_install SET consented_scopes = $3 WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id.as_uuid())
    .bind(install_id)
    .bind(json!(consented))
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    // Live tokens carry the old scope snapshot — revoke before re-issuing.
    revoke_install_tokens(tx, org_id, install_id, request_id).await?;
    let tokens = issue_tokens(tx, org_id, install_id, consented, request_id).await?;
    let install = fetch(tx, org_id, install_id, request_id).await?;

    emit_event(
        tx,
        org_id,
        actor,
        "oauth_token_issued",
        json!({
            "install_id": install.public_id,
            "listing_id": install.listing_public_id,
            "scopes": consented,
            "reason": "reconsent",
        }),
        request_id,
    )
    .await?;

    Ok((install, tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use companyos_authz::Role;
    use chrono::Utc;

    fn listing(kind: &str, scopes: &[&str]) -> ListingRow {
        ListingRow {
            id: new_uuid_v7(),
            org_id: new_uuid_v7(),
            public_id: "app_test".into(),
            slug: "test".into(),
            name: "Test".into(),
            description: String::new(),
            listing_kind: kind.into(),
            connector_key: None,
            requested_scopes: json!(scopes),
            redirect_uris: json!([]),
            webhook_subscriptions: json!([]),
            status: "published".into(),
            created_by: new_uuid_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn consent_must_be_subset_of_requested_scopes() {
        let listing = listing("third_party", &["sales.customer.read", "sales.deal.read"]);
        let owner = Principal::with_roles(vec![Role::Owner]);
        assert!(validate_consent(&listing, &["sales.customer.read".into()], &owner, "t").is_ok());
        assert!(validate_consent(&listing, &["finance.invoice.read".into()], &owner, "t").is_err());
    }

    #[test]
    fn consent_cannot_exceed_installer_permissions() {
        let listing = listing("third_party", &["sales.customer.read", "finance.invoice.issue"]);
        let member = Principal::with_roles(vec![Role::Member]);
        assert!(validate_consent(&listing, &["sales.customer.read".into()], &member, "t").is_ok());
        // Member holds no finance.invoice.issue, so it cannot be delegated.
        assert!(validate_consent(&listing, &["finance.invoice.issue".into()], &member, "t").is_err());
    }

    #[test]
    fn empty_consent_is_rejected() {
        let listing = listing("first_party", &["sales.customer.read"]);
        let owner = Principal::with_roles(vec![Role::Owner]);
        assert!(validate_consent(&listing, &[], &owner, "t").is_err());
    }

    /// Both listing kinds go through the same consent function and produce the
    /// same result for the same inputs.
    #[test]
    fn consent_rules_do_not_depend_on_listing_kind() {
        let owner = Principal::with_roles(vec![Role::Owner]);
        let first = listing("first_party", &["sales.customer.read"]);
        let third = listing("third_party", &["sales.customer.read"]);
        let a = validate_consent(&first, &["sales.customer.read".into()], &owner, "t").unwrap();
        let b = validate_consent(&third, &["sales.customer.read".into()], &owner, "t").unwrap();
        assert_eq!(a, b);
        assert_eq!(
            default_consent(&first, &owner),
            default_consent(&third, &owner)
        );
    }

    /// Structural guard: the install, token and OAuth paths must contain no
    /// control flow keyed on `listing_kind`. Needles are assembled at runtime
    /// so this assertion's own source cannot satisfy the search.
    #[test]
    fn install_token_and_oauth_paths_never_branch_on_listing_kind() {
        let field = format!("listing_{}", "kind");
        let needles = [
            format!("if {field}"),
            format!("if listing.{field}"),
            format!("match {field}"),
            format!("match listing.{field}"),
            format!("{field} =="),
            format!("== {field}"),
        ];
        let sources = [
            ("install.rs", include_str!("install.rs")),
            ("tokens.rs", include_str!("tokens.rs")),
            ("oauth.rs", include_str!("oauth.rs")),
        ];
        let test_marker = format!("#[cfg({})]", "test");
        for (name, source) in sources {
            let production = match source.find(&test_marker) {
                Some(idx) => &source[..idx],
                None => source,
            };
            for needle in &needles {
                assert!(
                    !production.contains(needle.as_str()),
                    "{name} branches on listing_kind: found {needle:?}"
                );
            }
        }
    }
}
