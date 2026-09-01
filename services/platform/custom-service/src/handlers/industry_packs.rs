//! `/api/v1/custom/industry-packs` — Phase 4.5 industry vertical packs.
//!
//! Install = import `companyos.custom.package` + apply seed defaults + record
//! marketplace listing reference. Uninstall marks the pack inactive and does
//! **not** delete tenant business data or custom records.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::packages::apply_package_import;
use super::{conflict, internal, not_found, require_perm, set_org};
use crate::auth::AuthCtx;
use crate::packs::{self, IndustryPack, IndustryPackSummary};
use crate::state::AppState;
use crate::types::ImportPackageResponse;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/custom/industry-packs", get(list_packs))
        .route("/api/v1/custom/industry-packs/{pack_id}", get(get_pack))
        .route(
            "/api/v1/custom/industry-packs/{pack_id}/install",
            post(install_pack),
        )
        .route(
            "/api/v1/custom/industry-packs/{pack_id}/uninstall",
            post(uninstall_pack),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IndustryPackListResponse {
    pub items: Vec<IndustryPackSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IndustryPackDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub marketplace_connector_key: String,
    pub entity_slugs: Vec<String>,
    pub installed: bool,
    pub seed_pipeline_stages: Vec<String>,
    pub seed_expense_categories: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InstallPackResponse {
    pub install_id: String,
    pub pack_id: String,
    pub pack_version: String,
    pub package: ImportPackageResponse,
    pub marketplace_connector_key: String,
    pub marketplace_install_id: Option<String>,
    pub seed_applied: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UninstallPackResponse {
    pub pack_id: String,
    pub status: String,
    pub note: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/industry-packs",
    tag = "custom-industry-packs",
    responses((status = 200, body = IndustryPackListResponse))
)]
pub async fn list_packs(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<IndustryPackListResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    // Catalogue is readable with builder.read; install still requires package.import.
    require_perm(&state, &auth, perms::custom_builder_read()).await?;

    let installed = installed_pack_ids(&state, auth.ctx.org_id.as_uuid(), rid).await?;
    let items = packs::catalogue()
        .into_iter()
        .map(|p| {
            let is_installed = installed.contains(&p.id);
            packs::summary(&p, is_installed)
        })
        .collect();
    Ok(Json(IndustryPackListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/industry-packs/{pack_id}",
    tag = "custom-industry-packs",
    responses((status = 200, body = IndustryPackDetail))
)]
pub async fn get_pack(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(pack_id): Path<String>,
) -> Result<Json<IndustryPackDetail>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_builder_read()).await?;
    let pack = packs::get(&pack_id).ok_or_else(|| not_found(rid, "industry pack"))?;
    let installed = installed_pack_ids(&state, auth.ctx.org_id.as_uuid(), rid).await?;
    let summary = packs::summary(&pack, installed.contains(&pack.id));
    Ok(Json(IndustryPackDetail {
        id: summary.id,
        name: summary.name,
        description: summary.description,
        version: summary.version,
        marketplace_connector_key: summary.marketplace_connector_key,
        entity_slugs: summary.entity_slugs,
        installed: summary.installed,
        seed_pipeline_stages: pack.seed.pipeline_stages.clone(),
        seed_expense_categories: pack.seed.expense_categories.clone(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/industry-packs/{pack_id}/install",
    tag = "custom-industry-packs",
    responses((status = 201, body = InstallPackResponse))
)]
pub async fn install_pack(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(pack_id): Path<String>,
) -> Result<(StatusCode, Json<InstallPackResponse>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    // Member cannot install org-wide packs (deny-matrix via sensitive import).
    require_perm(&state, &auth, perms::custom_package_import()).await?;

    let pack = packs::get(&pack_id).ok_or_else(|| not_found(rid, "industry pack"))?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let existing: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT public_id FROM custom_industry_pack_install
        WHERE org_id = $1 AND pack_id = $2 AND status = 'installed'
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&pack.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;
    if let Some((pid,)) = existing {
        return Err(conflict(
            rid,
            format!("industry pack '{}' already installed ({pid})", pack.id),
        ));
    }

    let package = apply_package_import(
        &mut tx,
        auth.ctx.org_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.user_id,
        auth.ctx.actor.is_ai,
        &pack.package,
        rid,
    )
    .await?;

    let seed_applied = apply_seed(&mut tx, auth.ctx.org_id.as_uuid(), &pack, rid).await?;

    let marketplace_install_id = try_install_marketplace_listing(
        &mut tx,
        auth.ctx.org_id.as_uuid(),
        &pack,
        auth.ctx.actor.on_behalf_of,
        rid,
    )
    .await?;

    let install_id = new_uuid_v7();
    let install_public = PublicId::new(IdKind::IndustryPackInstall, install_id);
    let package_install_uuid = package
        .package_id
        .parse::<PublicId>()
        .ok()
        .map(|p| p.uuid());

    sqlx::query(
        r#"
        INSERT INTO custom_industry_pack_install (
            id, org_id, public_id, pack_id, pack_version, package_name,
            package_install_id, marketplace_connector_key, marketplace_install_public_id,
            seed_applied, status, installed_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'installed',$11)
        "#,
    )
    .bind(install_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(install_public.as_str())
    .bind(&pack.id)
    .bind(&pack.version)
    .bind(&pack.package.name)
    .bind(package_install_uuid)
    .bind(&pack.marketplace.connector_key)
    .bind(&marketplace_install_id)
    .bind(&seed_applied)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.industry_pack.install",
        "industry_pack",
        &install_public.as_str(),
        serde_json::json!({
            "pack_id": pack.id,
            "version": pack.version,
            "marketplace_connector_key": pack.marketplace.connector_key,
            "entities_imported": package.entities_imported,
        }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok((
        StatusCode::CREATED,
        Json(InstallPackResponse {
            install_id: install_public.as_str(),
            pack_id: pack.id,
            pack_version: pack.version,
            package,
            marketplace_connector_key: pack.marketplace.connector_key,
            marketplace_install_id,
            seed_applied,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/industry-packs/{pack_id}/uninstall",
    tag = "custom-industry-packs",
    responses((status = 200, body = UninstallPackResponse))
)]
pub async fn uninstall_pack(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(pack_id): Path<String>,
) -> Result<Json<UninstallPackResponse>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_package_import()).await?;
    let _ = packs::get(&pack_id).ok_or_else(|| not_found(rid, "industry pack"))?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let updated = sqlx::query(
        r#"
        UPDATE custom_industry_pack_install
        SET status = 'uninstalled',
            uninstalled_at = now(),
            uninstalled_by = $3
        WHERE org_id = $1 AND pack_id = $2 AND status = 'installed'
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&pack_id)
    .bind(auth.ctx.actor.on_behalf_of)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    if updated.rows_affected() == 0 {
        return Err(not_found(rid, "installed industry pack"));
    }

    // Revoke marketplace install if present — do not delete custom records / seed rows.
    let connector: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT marketplace_connector_key FROM custom_industry_pack_install
        WHERE org_id = $1 AND pack_id = $2
        ORDER BY installed_at DESC LIMIT 1
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .bind(&pack_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal(rid))?;

    if let Some((key,)) = connector {
        let _ = sqlx::query(
            r#"
            UPDATE marketplace_install
            SET status = 'revoked', revoked_at = now(), revoked_by = $3
            WHERE org_id = $1 AND connector_key = $2 AND status = 'active'
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&key)
        .bind(auth.ctx.actor.on_behalf_of)
        .execute(&mut *tx)
        .await;
        // Ignore errors when marketplace schema is absent.
    }

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.industry_pack.uninstall",
        "industry_pack",
        &pack_id,
        serde_json::json!({
            "pack_id": pack_id,
            "data_retained": true,
        }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(UninstallPackResponse {
        pack_id,
        status: "uninstalled".into(),
        note: "Pack marked uninstalled; custom entities and tenant business data were retained."
            .into(),
    }))
}

async fn installed_pack_ids(
    state: &AppState,
    org_id: Uuid,
    rid: &str,
) -> Result<std::collections::HashSet<String>, AppError> {
    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    companyos_tenancy::set_session_org_id(&mut tx, companyos_tenancy::OrgId::new(org_id))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT pack_id FROM custom_industry_pack_install
        WHERE org_id = $1 AND status = 'installed'
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_else(|_| vec![]);
    tx.commit().await.ok();
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn apply_seed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    pack: &IndustryPack,
    rid: &str,
) -> Result<serde_json::Value, AppError> {
    // Seed is additive data — never branches CRM/finance/HR/inventory domain code.
    for (i, name) in pack.seed.pipeline_stages.iter().enumerate() {
        let _ = sqlx::query(
            r#"
            INSERT INTO workspace_seed_pipeline_stage (id, org_id, name, position)
            VALUES ($1,$2,$3,$4)
            ON CONFLICT (org_id, name) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(name)
        .bind(i as i32)
        .execute(&mut **tx)
        .await
        .map_err(internal(rid))?;
    }
    for name in &pack.seed.expense_categories {
        let _ = sqlx::query(
            r#"
            INSERT INTO workspace_seed_expense_category (id, org_id, name)
            VALUES ($1,$2,$3)
            ON CONFLICT (org_id, name) DO NOTHING
            "#,
        )
        .bind(new_uuid_v7())
        .bind(org_id)
        .bind(name)
        .execute(&mut **tx)
        .await
        .map_err(internal(rid))?;
    }

    let seed_json = serde_json::json!({
        "pipeline_stages": pack.seed.pipeline_stages,
        "expense_categories": pack.seed.expense_categories,
    });

    let _ = sqlx::query(
        r#"
        UPDATE organization
        SET seed_defaults = COALESCE(seed_defaults, '{}'::jsonb) || $2::jsonb,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(org_id)
    .bind(&seed_json)
    .execute(&mut **tx)
    .await
    .map_err(internal(rid))?;

    Ok(seed_json)
}

/// Best-effort marketplace install when Phase 3.4 tables are present.
async fn try_install_marketplace_listing(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    pack: &IndustryPack,
    installed_by: Uuid,
    rid: &str,
) -> Result<Option<String>, AppError> {
    let listing: Option<(Uuid, String, String, String)> = sqlx::query_as(
        r#"
        SELECT id, public_id, slug, name
        FROM marketplace_listing
        WHERE connector_key = $1 AND status = 'published'
        LIMIT 1
        "#,
    )
    .bind(&pack.marketplace.connector_key)
    .fetch_optional(&mut **tx)
    .await
    .unwrap_or(None);

    let Some((listing_id, listing_public_id, listing_slug, listing_name)) = listing else {
        return Ok(None);
    };

    let id = new_uuid_v7();
    let public_id = PublicId::new(IdKind::MarketplaceInstall, id).as_str();
    let scopes = serde_json::json!(pack.marketplace.requested_scopes);

    let result = sqlx::query(
        r#"
        INSERT INTO marketplace_install (
            id, org_id, public_id, listing_id, listing_public_id, listing_slug, listing_name,
            listing_kind, connector_key, consented_scopes, status, installed_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,'first_party',$8,$9,'active',$10)
        ON CONFLICT (org_id, listing_id) WHERE status = 'active' DO NOTHING
        "#,
    )
    .bind(id)
    .bind(org_id)
    .bind(&public_id)
    .bind(listing_id)
    .bind(&listing_public_id)
    .bind(&listing_slug)
    .bind(&listing_name)
    .bind(&pack.marketplace.connector_key)
    .bind(&scopes)
    .bind(installed_by)
    .execute(&mut **tx)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => Ok(Some(public_id)),
        Ok(_) => {
            // Already installed — look up existing.
            let existing: Option<(String,)> = sqlx::query_as(
                r#"
                SELECT public_id FROM marketplace_install
                WHERE org_id = $1 AND listing_id = $2 AND status = 'active'
                "#,
            )
            .bind(org_id)
            .bind(listing_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal(rid))?;
            Ok(existing.map(|(p,)| p))
        }
        Err(_) => Ok(None),
    }
}
