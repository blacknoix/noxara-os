//! First-party connector catalogue seed.
//!
//! First-party connectors are ordinary published listings — they exist so the
//! Integrations UI has something to connect to. Seeding is idempotent across
//! the whole database: a connector key already published by any publisher org
//! is reused rather than duplicated, which also keeps the partial unique index
//! `marketplace_listing_connector_published_uniq` satisfied.

use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::set_seed_flag;
use super::types::{KIND_FIRST_PARTY, LISTING_PUBLISHED};

pub struct FirstPartyConnector {
    pub connector_key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub requested_scopes: &'static [&'static str],
}

/// The five bundled first-party connectors.
pub const FIRST_PARTY_CATALOGUE: &[FirstPartyConnector] = &[
    FirstPartyConnector {
        connector_key: "email.google",
        name: "Google Workspace Email",
        description: "Sync Gmail threads onto customer and deal timelines.",
        requested_scopes: &[
            "sales.activity.read",
            "sales.activity.create",
            "platform.notification.read",
        ],
    },
    FirstPartyConnector {
        connector_key: "calendar.microsoft",
        name: "Microsoft 365 Calendar",
        description: "Two-way sync of meetings with deals and tasks.",
        requested_scopes: &[
            "sales.activity.read",
            "sales.activity.create",
            "operations.task.read",
        ],
    },
    FirstPartyConnector {
        connector_key: "payments.stripe",
        name: "Stripe Payments",
        description: "Reconcile Stripe charges against issued invoices.",
        requested_scopes: &[
            "finance.invoice.read",
            "finance.payment.read",
            "finance.payment.create",
        ],
    },
    FirstPartyConnector {
        connector_key: "storage.s3",
        name: "Amazon S3 Storage",
        description: "Archive generated documents to an S3 bucket.",
        requested_scopes: &["platform.file.read", "platform.file.create"],
    },
    FirstPartyConnector {
        connector_key: "chat.slack",
        name: "Slack",
        description: "Deliver notifications and search results into Slack.",
        requested_scopes: &["platform.notification.read", "platform.search.read"],
    },
];

/// Phase 4.5 industry packs as first-party marketplace listings (config apps, not forks).
pub const INDUSTRY_PACK_CATALOGUE: &[FirstPartyConnector] = &[
    FirstPartyConnector {
        connector_key: "industry.professional-services",
        name: "Industry pack: Professional Services",
        description: "Custom entities and seed defaults for professional services orgs.",
        requested_scopes: &["workspace.org.read", "custom.builder.read"],
    },
    FirstPartyConnector {
        connector_key: "industry.retail",
        name: "Industry pack: Retail",
        description: "Catalogue and POS-light custom entities for retail orgs.",
        requested_scopes: &["workspace.org.read", "custom.builder.read"],
    },
    FirstPartyConnector {
        connector_key: "industry.light-manufacturing",
        name: "Industry pack: Light manufacturing",
        description: "BOM and work-order custom entities for light manufacturing.",
        requested_scopes: &["workspace.org.read", "custom.builder.read"],
    },
    FirstPartyConnector {
        connector_key: "industry.healthcare-admin",
        name: "Industry pack: Healthcare admin",
        description: "Appointment and admin-note custom entities. No PHI authz bypass.",
        requested_scopes: &["workspace.org.read", "custom.builder.read"],
    },
];

#[derive(Debug, Clone)]
pub struct SeededListing {
    pub id: Uuid,
    pub org_id: Uuid,
    pub public_id: String,
    pub slug: String,
    pub connector_key: String,
}

/// Upsert first-party connector + industry-pack listings as published, owned by
/// `publisher_org_id`.
///
/// Safe to call repeatedly and from concurrent test binaries.
pub async fn seed_first_party_catalogue(
    pool: &PgPool,
    publisher_org_id: OrgId,
) -> anyhow::Result<Vec<SeededListing>> {
    let mut seeded =
        Vec::with_capacity(FIRST_PARTY_CATALOGUE.len() + INDUSTRY_PACK_CATALOGUE.len());
    for connector in FIRST_PARTY_CATALOGUE
        .iter()
        .chain(INDUSTRY_PACK_CATALOGUE.iter())
    {
        seeded.push(seed_one(pool, publisher_org_id, connector).await?);
    }
    Ok(seeded)
}

async fn seed_one(
    pool: &PgPool,
    publisher_org_id: OrgId,
    connector: &FirstPartyConnector,
) -> anyhow::Result<SeededListing> {
    if let Some(existing) = find_published(pool, publisher_org_id, connector.connector_key).await? {
        return Ok(existing);
    }

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceApp, id).as_str();

    let mut tx = pool.begin().await?;
    set_seed_flag(&mut tx).await?;
    set_session_org_id(&mut tx, publisher_org_id).await?;
    let result = sqlx::query(
        r#"
        INSERT INTO marketplace_listing (
            id, org_id, public_id, slug, name, description, listing_kind, connector_key,
            requested_scopes, redirect_uris, webhook_subscriptions, status, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'[]'::jsonb,'[]'::jsonb,$10,$11)
        "#,
    )
    .bind(id)
    .bind(publisher_org_id.as_uuid())
    .bind(&public_id)
    .bind(connector.connector_key)
    .bind(connector.name)
    .bind(connector.description)
    .bind(KIND_FIRST_PARTY)
    .bind(connector.connector_key)
    .bind(json!(connector.requested_scopes))
    .bind(LISTING_PUBLISHED)
    .bind(Uuid::nil())
    .execute(&mut *tx)
    .await;

    match result {
        Ok(_) => {
            tx.commit().await?;
            Ok(SeededListing {
                id,
                org_id: publisher_org_id.as_uuid(),
                public_id,
                slug: connector.connector_key.to_string(),
                connector_key: connector.connector_key.to_string(),
            })
        }
        // Another org (or a racing seeder) already published this connector.
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {
            tx.rollback().await?;
            find_published(pool, publisher_org_id, connector.connector_key)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "connector {} conflicted but is not published",
                        connector.connector_key
                    )
                })
        }
        Err(e) => Err(e.into()),
    }
}

async fn find_published(
    pool: &PgPool,
    session_org: OrgId,
    connector_key: &str,
) -> anyhow::Result<Option<SeededListing>> {
    let mut tx = pool.begin().await?;
    set_seed_flag(&mut tx).await?;
    set_session_org_id(&mut tx, session_org).await?;
    let row: Option<(Uuid, Uuid, String, String)> = sqlx::query_as(
        "SELECT id, org_id, public_id, slug FROM marketplace_listing \
         WHERE connector_key = $1 AND status = 'published'",
    )
    .bind(connector_key)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.map(|(id, org_id, public_id, slug)| SeededListing {
        id,
        org_id,
        public_id,
        slug,
        connector_key: connector_key.to_string(),
    }))
}

/// Optional boot-time seed: set `MARKETPLACE_SEED_ORG=org_…` to publish the
/// first-party catalogue under that org on startup.
pub async fn bootstrap_from_env(pool: &PgPool) -> anyhow::Result<()> {
    let Ok(raw) = std::env::var("MARKETPLACE_SEED_ORG") else {
        return Ok(());
    };
    if raw.is_empty() {
        return Ok(());
    }
    let public: PublicId = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("MARKETPLACE_SEED_ORG must be an org_… public id"))?;
    let org_id = OrgId::from_public(&public)
        .map_err(|_| anyhow::anyhow!("MARKETPLACE_SEED_ORG must be an org_… public id"))?;
    let seeded = seed_first_party_catalogue(pool, org_id).await?;
    tracing::info!(
        count = seeded.len(),
        org = %org_id,
        "seeded first-party marketplace catalogue"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_covers_the_five_bundled_connectors() {
        let keys: Vec<&str> = FIRST_PARTY_CATALOGUE
            .iter()
            .map(|c| c.connector_key)
            .collect();
        assert_eq!(
            keys,
            vec![
                "email.google",
                "calendar.microsoft",
                "payments.stripe",
                "storage.s3",
                "chat.slack"
            ]
        );
    }

    #[test]
    fn seeded_scopes_are_real_permissions() {
        for connector in FIRST_PARTY_CATALOGUE {
            for scope in connector.requested_scopes {
                assert!(
                    companyos_authz::PERMISSION_CATALOGUE
                        .iter()
                        .any(|p| p.id == *scope),
                    "{scope} is not in the permission catalogue"
                );
            }
        }
    }
}
