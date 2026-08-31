//! Listing persistence and publisher-side lifecycle (draft → submitted).

use chrono::{DateTime, Utc};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::OrgId;
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::types::{
    CreateListingRequest, ListingDto, KIND_FIRST_PARTY, KIND_THIRD_PARTY, LISTING_DRAFT,
    LISTING_PUBLISHED, LISTING_SUBMITTED,
};
use super::{conflict, internal, not_found, org_public, string_array, validation};

pub const LISTING_COLUMNS: &str = "id, org_id, public_id, slug, name, description, listing_kind, \
     connector_key, requested_scopes, redirect_uris, webhook_subscriptions, status, created_by, \
     created_at, updated_at";

#[derive(Debug, Clone, FromRow)]
pub struct ListingRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub public_id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub listing_kind: String,
    pub connector_key: Option<String>,
    pub requested_scopes: serde_json::Value,
    pub redirect_uris: serde_json::Value,
    pub webhook_subscriptions: serde_json::Value,
    pub status: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ListingRow {
    pub fn requested_scopes(&self) -> Vec<String> {
        string_array(&self.requested_scopes)
    }

    pub fn redirect_uris(&self) -> Vec<String> {
        string_array(&self.redirect_uris)
    }

    pub fn to_dto(&self) -> ListingDto {
        ListingDto {
            id: self.public_id.clone(),
            org_id: org_public(self.org_id),
            slug: self.slug.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            listing_kind: self.listing_kind.clone(),
            connector_key: self.connector_key.clone(),
            requested_scopes: self.requested_scopes(),
            redirect_uris: self.redirect_uris(),
            webhook_subscriptions: string_array(&self.webhook_subscriptions),
            status: self.status.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Validate redirect / callback URLs fail-closed against SSRF.
///
/// `allow_private` only relaxes the address check (loopback fixtures in tests);
/// the scheme and host requirements always apply.
pub fn validate_urls(
    urls: &[String],
    allow_private: bool,
    request_id: &str,
    label: &str,
) -> Result<(), AppError> {
    for raw in urls {
        let parsed = url::Url::parse(raw)
            .map_err(|_| validation(request_id, format!("{label} {raw} is not an absolute URL")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(validation(
                request_id,
                format!("{label} {raw} must use http or https"),
            ));
        }
        if parsed.host().is_none() {
            return Err(validation(
                request_id,
                format!("{label} {raw} must include a host"),
            ));
        }
        if allow_private {
            continue;
        }
        crate::ssrf::assert_url_safe(raw)
            .map_err(|e| validation(request_id, format!("{label} {raw} rejected: {e}")))?;
    }
    Ok(())
}

/// Requested scopes must name real permissions from the authz catalogue.
pub fn validate_requested_scopes(scopes: &[String], request_id: &str) -> Result<(), AppError> {
    for scope in scopes {
        if !companyos_authz::PERMISSION_CATALOGUE
            .iter()
            .any(|p| p.id == scope.as_str())
        {
            return Err(validation(
                request_id,
                format!("requested scope {scope} is not a known permission"),
            ));
        }
    }
    Ok(())
}

pub async fn fetch_by_uuid(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LISTING_COLUMNS} FROM marketplace_listing WHERE id = $1"
    ))
    .bind(listing_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "listing"))
}

pub async fn fetch_owned(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LISTING_COLUMNS} FROM marketplace_listing WHERE id = $1 AND org_id = $2"
    ))
    .bind(listing_id)
    .bind(org_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "listing"))
}

/// Resolve a published listing by public id — the install entry point.
pub async fn fetch_published(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    let row = fetch_by_uuid(tx, listing_id, request_id).await?;
    if row.status != LISTING_PUBLISHED {
        return Err(validation(
            request_id,
            format!("listing {} is not published", row.public_id),
        ));
    }
    Ok(row)
}

/// Resolve a published listing by connector key (integrations alias).
pub async fn fetch_published_by_connector(
    tx: &mut Transaction<'_, Postgres>,
    connector_key: &str,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LISTING_COLUMNS} FROM marketplace_listing \
         WHERE connector_key = $1 AND status = 'published'"
    ))
    .bind(connector_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?
    .ok_or_else(|| not_found(request_id, "connector"))
}

pub async fn list_published(
    tx: &mut Transaction<'_, Postgres>,
    request_id: &str,
) -> Result<Vec<ListingRow>, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LISTING_COLUMNS} FROM marketplace_listing WHERE status = 'published' \
         ORDER BY name ASC LIMIT 500"
    ))
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))
}

pub async fn list_owned(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<ListingRow>, AppError> {
    sqlx::query_as(&format!(
        "SELECT {LISTING_COLUMNS} FROM marketplace_listing WHERE org_id = $1 \
         ORDER BY updated_at DESC LIMIT 500"
    ))
    .bind(org_id.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(internal(request_id))
}

pub struct CreatedListing {
    pub listing: ListingRow,
    pub client_id: String,
    pub client_public_id: String,
    /// Plaintext client secret — surfaced once and never persisted.
    pub client_secret: String,
}

/// Create a draft listing plus its OAuth client.
pub async fn create_listing(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    created_by: Uuid,
    req: &CreateListingRequest,
    allow_private_urls: bool,
    request_id: &str,
) -> Result<CreatedListing, AppError> {
    if req.name.trim().is_empty() {
        return Err(validation(request_id, "name is required"));
    }
    let slug = req.slug.trim().to_lowercase();
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
    {
        return Err(validation(
            request_id,
            "slug must be non-empty and use [a-z0-9._-]",
        ));
    }

    let listing_kind = req.listing_kind.as_deref().unwrap_or(KIND_THIRD_PARTY);
    if !matches!(listing_kind, KIND_FIRST_PARTY | KIND_THIRD_PARTY) {
        return Err(validation(
            request_id,
            "listing_kind must be first_party or third_party",
        ));
    }

    validate_requested_scopes(&req.requested_scopes, request_id)?;
    // Empty entries come from optional form fields; drop them rather than
    // failing the whole create.
    let redirect_uris: Vec<String> = req
        .redirect_uris
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();
    validate_urls(
        &redirect_uris,
        allow_private_urls,
        request_id,
        "redirect_uri",
    )?;
    // Subscriptions may be event-type names or callback URLs; SSRF-check the URLs.
    let webhook_urls: Vec<String> = req
        .webhook_subscriptions
        .iter()
        .filter(|s| url::Url::parse(s).is_ok())
        .cloned()
        .collect();
    validate_urls(
        &webhook_urls,
        allow_private_urls,
        request_id,
        "webhook_subscription",
    )?;

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceApp, id).as_str();

    let inserted = sqlx::query(
        r#"
        INSERT INTO marketplace_listing (
            id, org_id, public_id, slug, name, description, listing_kind, connector_key,
            requested_scopes, redirect_uris, webhook_subscriptions, status, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        ON CONFLICT (org_id, slug) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(&public_id)
    .bind(&slug)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(listing_kind)
    .bind(req.connector_key.as_deref())
    .bind(json!(req.requested_scopes))
    .bind(json!(redirect_uris))
    .bind(json!(req.webhook_subscriptions))
    .bind(LISTING_DRAFT)
    .bind(created_by)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    if inserted.rows_affected() == 0 {
        return Err(conflict(
            request_id,
            format!("a listing with slug {slug} already exists in this organization"),
        ));
    }

    let client_secret = generate_opaque_token();
    let client_uuid = new_uuid_v7();
    let client_public_id = PublicId::new(IdKind::MarketplaceOauthClient, client_uuid).as_str();
    let client_id = format!("mcl_{client_uuid}");

    sqlx::query(
        r#"
        INSERT INTO marketplace_oauth_client (
            id, org_id, listing_id, public_id, client_id, client_secret_hash
        ) VALUES ($1,$2,$3,$4,$5,$6)
        "#,
    )
    .bind(client_uuid)
    .bind(org_id.as_uuid())
    .bind(id)
    .bind(&client_public_id)
    .bind(&client_id)
    .bind(hash_token(&client_secret))
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let listing = fetch_owned(tx, org_id, id, request_id).await?;
    Ok(CreatedListing {
        listing,
        client_id,
        client_public_id,
        client_secret,
    })
}

/// Move a draft/rejected listing into the review queue.
pub async fn submit_listing(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    let listing = fetch_owned(tx, org_id, listing_id, request_id).await?;
    if !matches!(
        listing.status.as_str(),
        LISTING_DRAFT | "rejected" | LISTING_SUBMITTED
    ) {
        return Err(conflict(
            request_id,
            format!("listing in status {} cannot be submitted", listing.status),
        ));
    }
    if listing.requested_scopes().is_empty() {
        return Err(validation(
            request_id,
            "listing must request at least one scope before submission",
        ));
    }

    sqlx::query(
        "UPDATE marketplace_listing SET status = $3, updated_at = now() \
         WHERE id = $1 AND org_id = $2",
    )
    .bind(listing_id)
    .bind(org_id.as_uuid())
    .bind(LISTING_SUBMITTED)
    .execute(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    fetch_owned(tx, org_id, listing_id, request_id).await
}

pub async fn set_status(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    listing_id: Uuid,
    status: &str,
    request_id: &str,
) -> Result<ListingRow, AppError> {
    sqlx::query(
        "UPDATE marketplace_listing SET status = $3, updated_at = now() \
         WHERE id = $1 AND org_id = $2",
    )
    .bind(listing_id)
    .bind(org_id.as_uuid())
    .bind(status)
    .execute(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => conflict(
            request_id,
            "another published listing already claims this connector key",
        ),
        _ => internal(request_id)(e),
    })?;
    fetch_owned(tx, org_id, listing_id, request_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_and_loopback_redirects() {
        assert!(validate_urls(&["ftp://x.example/cb".into()], false, "t", "redirect_uri").is_err());
        assert!(
            validate_urls(&["http://127.0.0.1/cb".into()], false, "t", "redirect_uri").is_err()
        );
        assert!(validate_urls(&["https://8.8.8.8/cb".into()], false, "t", "redirect_uri").is_ok());
    }

    #[test]
    fn allow_private_still_requires_http_scheme() {
        assert!(validate_urls(&["http://127.0.0.1/cb".into()], true, "t", "redirect_uri").is_ok());
        assert!(validate_urls(&["ftp://127.0.0.1/cb".into()], true, "t", "redirect_uri").is_err());
        assert!(validate_urls(&["nonsense".into()], true, "t", "redirect_uri").is_err());
    }

    #[test]
    fn requested_scopes_must_exist_in_catalogue() {
        assert!(validate_requested_scopes(&["sales.customer.read".into()], "t").is_ok());
        assert!(validate_requested_scopes(&["not.a.permission".into()], "t").is_err());
    }
}
