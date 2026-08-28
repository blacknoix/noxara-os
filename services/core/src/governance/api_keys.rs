//! Organization API keys — hashed at rest; raw secrets are returned exactly
//! once (at creation or rotation) and never stored or re-shown.

use chrono::{DateTime, Utc};
use companyos_auth_token::{generate_opaque_token, hash_token};
use companyos_errors::AppError;
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use companyos_tenancy::{set_session_org_id, OrgId};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::types::ApiKeyView;
use super::{internal, not_found, tenancy_internal};

const KEY_PREFIX_LEN: usize = 8;

type ApiKeyDbRow = (
    String,
    String,
    String,
    Value,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub async fn list(
    pool: &PgPool,
    org_id: OrgId,
    request_id: &str,
) -> Result<Vec<ApiKeyView>, AppError> {
    let mut tx = pool.begin().await.map_err(internal(request_id))?;
    set_session_org_id(&mut tx, org_id)
        .await
        .map_err(tenancy_internal(request_id))?;

    let rows: Vec<ApiKeyDbRow> = sqlx::query_as(
        r#"
        SELECT public_id, name, key_prefix, scopes, expires_at, revoked_at, created_at, last_used_at
        FROM org_api_key
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(request_id))?;
    tx.commit().await.map_err(internal(request_id))?;

    Ok(rows
        .into_iter()
        .map(
            |(id, name, key_prefix, scopes, expires_at, revoked_at, created_at, last_used_at)| {
                ApiKeyView {
                    id,
                    name,
                    key_prefix,
                    scopes: serde_json::from_value(scopes).unwrap_or_default(),
                    expires_at: expires_at.map(|d| d.to_rfc3339()),
                    revoked_at: revoked_at.map(|d| d.to_rfc3339()),
                    created_at: created_at.to_rfc3339(),
                    last_used_at: last_used_at.map(|d| d.to_rfc3339()),
                }
            },
        )
        .collect())
}

pub async fn create(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    created_by: Uuid,
    name: &str,
    scopes: &[String],
    expires_at: Option<DateTime<Utc>>,
    request_id: &str,
) -> Result<(ApiKeyView, String), AppError> {
    let secret = generate_opaque_token();
    let key_hash = hash_token(&secret);
    let key_prefix: String = secret.chars().take(KEY_PREFIX_LEN).collect();
    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::ApiKey, id);
    let scopes_json = serde_json::to_value(scopes).unwrap_or_default();

    let (created_at,): (DateTime<Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO org_api_key (
            id, org_id, public_id, name, key_prefix, key_hash, scopes, expires_at, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING created_at
        "#,
    )
    .bind(id)
    .bind(org_id.as_uuid())
    .bind(public_id.as_str())
    .bind(name)
    .bind(&key_prefix)
    .bind(&key_hash)
    .bind(&scopes_json)
    .bind(expires_at)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok((
        ApiKeyView {
            id: public_id.as_str(),
            name: name.to_string(),
            key_prefix,
            scopes: scopes.to_vec(),
            expires_at: expires_at.map(|d| d.to_rfc3339()),
            revoked_at: None,
            created_at: created_at.to_rfc3339(),
            last_used_at: None,
        },
        secret,
    ))
}

pub async fn rotate(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    key_public_id: &str,
    request_id: &str,
) -> Result<(ApiKeyView, String), AppError> {
    type ExistingKeyRow = (Uuid, String, Value, Option<DateTime<Utc>>, Uuid);
    let existing: Option<ExistingKeyRow> = sqlx::query_as(
        r#"
        SELECT id, name, scopes, expires_at, created_by
        FROM org_api_key
        WHERE org_id = $1 AND public_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(key_public_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    let Some((old_id, name, scopes, expires_at, created_by)) = existing else {
        return Err(not_found(request_id, "api key"));
    };

    sqlx::query("UPDATE org_api_key SET revoked_at = now(), updated_at = now() WHERE id = $1")
        .bind(old_id)
        .execute(&mut **tx)
        .await
        .map_err(internal(request_id))?;

    let secret = generate_opaque_token();
    let key_hash = hash_token(&secret);
    let key_prefix: String = secret.chars().take(KEY_PREFIX_LEN).collect();
    let new_id = new_uuid_v7();
    let new_public_id = PublicId::new(IdKind::ApiKey, new_id);

    let (created_at,): (DateTime<Utc>,) = sqlx::query_as(
        r#"
        INSERT INTO org_api_key (
            id, org_id, public_id, name, key_prefix, key_hash, scopes, expires_at, rotated_from, created_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING created_at
        "#,
    )
    .bind(new_id)
    .bind(org_id.as_uuid())
    .bind(new_public_id.as_str())
    .bind(&name)
    .bind(&key_prefix)
    .bind(&key_hash)
    .bind(&scopes)
    .bind(expires_at)
    .bind(old_id)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    Ok((
        ApiKeyView {
            id: new_public_id.as_str(),
            name,
            key_prefix,
            scopes: serde_json::from_value(scopes).unwrap_or_default(),
            expires_at: expires_at.map(|d| d.to_rfc3339()),
            revoked_at: None,
            created_at: created_at.to_rfc3339(),
            last_used_at: None,
        },
        secret,
    ))
}

pub async fn revoke(
    tx: &mut Transaction<'_, Postgres>,
    org_id: OrgId,
    key_public_id: &str,
    request_id: &str,
) -> Result<(), AppError> {
    let updated: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE org_api_key SET revoked_at = now(), updated_at = now()
        WHERE org_id = $1 AND public_id = $2 AND revoked_at IS NULL
        RETURNING id
        "#,
    )
    .bind(org_id.as_uuid())
    .bind(key_public_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal(request_id))?;

    if updated.is_none() {
        return Err(not_found(request_id, "api key"));
    }
    Ok(())
}
