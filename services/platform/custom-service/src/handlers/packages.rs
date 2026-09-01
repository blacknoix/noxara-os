//! `/api/v1/custom/packages` — export / additive import of customisation packages.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use companyos_authz::perms;
use companyos_errors::{AppError, ErrorCode};
use companyos_ids::{new_uuid_v7, IdKind, PublicId};
use serde_json::Value;

use super::{internal, require_perm, set_org, validation};
use crate::auth::AuthCtx;
use crate::permissions::{register_entity_permissions, validate_slug};
use crate::state::AppState;
use crate::types::{
    CustomPackage, FieldDef, ImportPackageRequest, ImportPackageResponse, PackageEntity,
    PackageLayout, PackageScript, PackageView, PACKAGE_FORMAT, PACKAGE_FORMAT_VERSION,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/custom/packages/export", get(export_package))
        .route("/api/v1/custom/packages/import", post(import_package))
}

#[utoipa::path(
    get,
    path = "/api/v1/custom/packages/export",
    tag = "custom-packages",
    responses((status = 200, body = CustomPackage))
)]
pub async fn export_package(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> Result<Json<CustomPackage>, AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_package_export()).await?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let entity_rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        r#"
        SELECT slug, label, description, fields
        FROM custom_entity_definition
        WHERE org_id = $1 AND status = 'published' AND deleted_at IS NULL
        ORDER BY slug
        "#,
    )
    .bind(auth.ctx.org_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(internal(rid))?;

    let mut entities = Vec::new();
    let mut permissions = Vec::new();
    let mut slugs = Vec::new();
    for (slug, label, description, fields_json) in entity_rows {
        let fields: Vec<FieldDef> = serde_json::from_value(fields_json)
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;
        permissions.push(format!("custom.{slug}.read"));
        permissions.push(format!("custom.{slug}.write"));
        slugs.push(slug.clone());
        entities.push(PackageEntity {
            slug,
            label,
            description,
            fields,
        });
    }

    let view_rows: Vec<(String, String, Value, Value, Value)> = if slugs.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            r#"
            SELECT entity_slug, name, columns, filters, sort
            FROM custom_view
            WHERE org_id = $1 AND entity_slug = ANY($2)
            ORDER BY entity_slug, name
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&slugs)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(rid))?
    };

    let layout_rows: Vec<(String, String, Value)> = if slugs.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            r#"
            SELECT entity_slug, name, sections
            FROM custom_layout
            WHERE org_id = $1 AND entity_slug = ANY($2)
            ORDER BY entity_slug
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&slugs)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(rid))?
    };

    let script_rows: Vec<(String, String, String)> = if slugs.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            r#"
            SELECT entity_slug, hook, source
            FROM custom_script
            WHERE org_id = $1 AND entity_slug = ANY($2) AND enabled = true
            ORDER BY entity_slug, hook
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&slugs)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal(rid))?
    };

    tx.commit().await.map_err(internal(rid))?;

    Ok(Json(CustomPackage {
        format: PACKAGE_FORMAT.to_string(),
        format_version: PACKAGE_FORMAT_VERSION,
        name: "org-export".into(),
        version: "1.0.0".into(),
        entities,
        views: view_rows
            .into_iter()
            .map(|(entity_slug, name, columns, filters, sort)| PackageView {
                entity_slug,
                name,
                columns,
                filters,
                sort,
            })
            .collect(),
        layouts: layout_rows
            .into_iter()
            .map(|(entity_slug, name, sections)| PackageLayout {
                entity_slug,
                name,
                sections,
            })
            .collect(),
        scripts: script_rows
            .into_iter()
            .map(|(entity_slug, hook, source)| PackageScript {
                entity_slug,
                hook,
                source,
            })
            .collect(),
        permissions,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/custom/packages/import",
    tag = "custom-packages",
    request_body = ImportPackageRequest,
    responses((status = 201, body = ImportPackageResponse))
)]
pub async fn import_package(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<ImportPackageRequest>,
) -> Result<(StatusCode, Json<ImportPackageResponse>), AppError> {
    let rid = auth.ctx.request_id.as_str();
    require_perm(&state, &auth, perms::custom_package_import()).await?;

    let pkg = body.package;
    if pkg.format != PACKAGE_FORMAT {
        return Err(validation(
            rid,
            format!("unsupported package format '{}'", pkg.format),
        ));
    }
    if pkg.format_version != PACKAGE_FORMAT_VERSION {
        return Err(validation(
            rid,
            format!(
                "unsupported package format_version {} (expected {PACKAGE_FORMAT_VERSION})",
                pkg.format_version
            ),
        ));
    }

    let install_id = new_uuid_v7();
    let install_public = PublicId::new(IdKind::CustomPackage, install_id);
    let actor = auth.ctx.actor.on_behalf_of;
    let artifact = serde_json::to_value(&pkg)
        .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

    let mut tx = state.pool.begin().await.map_err(internal(rid))?;
    set_org(&mut tx, auth.ctx.org_id, rid).await?;

    let mut entities_imported = 0usize;
    let mut permissions = Vec::new();

    for ent in &pkg.entities {
        validate_slug(&ent.slug).map_err(|e| validation(rid, e))?;
        let fields_json = serde_json::to_value(&ent.fields)
            .map_err(|e| AppError::new(ErrorCode::Internal, rid, e.to_string()))?;

        let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM custom_entity_definition
            WHERE org_id = $1 AND slug = $2 AND deleted_at IS NULL
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&ent.slug)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(rid))?;

        let entity_id = if let Some((id,)) = existing {
            // Additive: leave existing definition as-is; still ensure published + perms.
            id
        } else {
            let id = new_uuid_v7();
            let public_id = PublicId::new(IdKind::CustomEntity, id);
            sqlx::query(
                r#"
                INSERT INTO custom_entity_definition (
                    id, org_id, public_id, slug, label, description, fields,
                    status, published_version, created_by, updated_by
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,'draft',0,$8,$8)
                "#,
            )
            .bind(id)
            .bind(auth.ctx.org_id.as_uuid())
            .bind(public_id.as_str())
            .bind(&ent.slug)
            .bind(&ent.label)
            .bind(&ent.description)
            .bind(&fields_json)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(internal(rid))?;
            entities_imported += 1;
            id
        };

        sqlx::query(
            r#"
            UPDATE custom_entity_definition
            SET status = 'published',
                published_version = GREATEST(published_version, 1),
                updated_by = $3,
                updated_at = now()
            WHERE org_id = $1 AND id = $2 AND deleted_at IS NULL
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(entity_id)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;

        let (read_id, write_id) = register_entity_permissions(&mut tx, auth.ctx.org_id, &ent.slug)
            .await
            .map_err(internal(rid))?;
        permissions.push(read_id);
        permissions.push(write_id);
    }

    for view in &pkg.views {
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CustomView, id);
        // Additive: skip if a view with same name already exists for the slug.
        let exists: Option<(uuid::Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM custom_view
            WHERE org_id = $1 AND entity_slug = $2 AND name = $3
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&view.entity_slug)
        .bind(&view.name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(rid))?;
        if exists.is_some() {
            continue;
        }
        sqlx::query(
            r#"
            INSERT INTO custom_view (
                id, org_id, public_id, entity_slug, name, columns, filters, sort, created_by
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(&view.entity_slug)
        .bind(&view.name)
        .bind(&view.columns)
        .bind(&view.filters)
        .bind(&view.sort)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
    }

    for layout in &pkg.layouts {
        let exists: Option<(uuid::Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM custom_layout
            WHERE org_id = $1 AND entity_slug = $2
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&layout.entity_slug)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(rid))?;
        if exists.is_some() {
            continue;
        }
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CustomLayout, id);
        sqlx::query(
            r#"
            INSERT INTO custom_layout (
                id, org_id, public_id, entity_slug, name, sections, created_by
            ) VALUES ($1,$2,$3,$4,$5,$6,$7)
            "#,
        )
        .bind(id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(&layout.entity_slug)
        .bind(&layout.name)
        .bind(&layout.sections)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
    }

    for script in &pkg.scripts {
        let exists: Option<(uuid::Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM custom_script
            WHERE org_id = $1 AND entity_slug = $2 AND hook = $3
            "#,
        )
        .bind(auth.ctx.org_id.as_uuid())
        .bind(&script.entity_slug)
        .bind(&script.hook)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal(rid))?;
        if exists.is_some() {
            continue;
        }
        let id = new_uuid_v7();
        let public_id = PublicId::new(IdKind::CustomScript, id);
        sqlx::query(
            r#"
            INSERT INTO custom_script (
                id, org_id, public_id, entity_slug, hook, source, enabled, created_by
            ) VALUES ($1,$2,$3,$4,$5,$6,true,$7)
            "#,
        )
        .bind(id)
        .bind(auth.ctx.org_id.as_uuid())
        .bind(public_id.as_str())
        .bind(&script.entity_slug)
        .bind(&script.hook)
        .bind(&script.source)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(internal(rid))?;
    }

    sqlx::query(
        r#"
        INSERT INTO custom_package_install (
            id, org_id, public_id, package_name, package_version, artifact, installed_by
        ) VALUES ($1,$2,$3,$4,$5,$6,$7)
        ON CONFLICT (org_id, package_name, package_version) DO NOTHING
        "#,
    )
    .bind(install_id)
    .bind(auth.ctx.org_id.as_uuid())
    .bind(install_public.as_str())
    .bind(&pkg.name)
    .bind(&pkg.version)
    .bind(&artifact)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(internal(rid))?;

    crate::audit::insert_audit(
        &mut *tx,
        auth.ctx.org_id.as_uuid(),
        auth.ctx.actor.user_id,
        auth.ctx.actor.on_behalf_of,
        auth.ctx.actor.is_ai,
        "custom.package.import",
        "custom_package",
        &install_public.as_str(),
        serde_json::json!({
            "name": pkg.name,
            "version": pkg.version,
            "entities_imported": entities_imported,
        }),
    )
    .await
    .map_err(internal(rid))?;

    tx.commit().await.map_err(internal(rid))?;

    permissions.sort();
    permissions.dedup();

    Ok((
        StatusCode::CREATED,
        Json(ImportPackageResponse {
            package_id: install_public.as_str(),
            entities_imported,
            permissions,
        }),
    ))
}
