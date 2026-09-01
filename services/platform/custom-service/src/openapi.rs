//! OpenAPI 3.1 document for `/api/v1/custom/...`.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{entities, industry_packs, layouts, packages, records, scripts, views};
use crate::packs::{IndustryPackSummary, PackMarketplace, PackSeed};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        entities::list_entities,
        entities::create_entity,
        entities::get_entity,
        entities::update_entity,
        entities::delete_entity,
        entities::publish_entity,
        records::list_records,
        records::create_record,
        records::get_record,
        records::update_record,
        records::delete_record,
        views::list_views,
        views::create_view,
        layouts::get_layout,
        layouts::upsert_layout,
        scripts::list_scripts,
        scripts::upsert_script,
        packages::export_package,
        packages::import_package,
        industry_packs::list_packs,
        industry_packs::get_pack,
        industry_packs::install_pack,
        industry_packs::uninstall_pack,
    ),
    components(schemas(
        FieldType,
        FieldDef,
        EntityDefinitionDto,
        CreateEntityRequest,
        UpdateEntityRequest,
        entities::EntityListResponse,
        entities::DeleteEntityRequest,
        entities::MessageResponse,
        CustomRecordDto,
        UpsertRecordRequest,
        records::RecordListResponse,
        CustomViewDto,
        UpsertViewRequest,
        views::ViewListResponse,
        CustomLayoutDto,
        UpsertLayoutRequest,
        CustomScriptDto,
        UpsertScriptRequest,
        scripts::ScriptListResponse,
        CustomPackage,
        PackageEntity,
        PackageView,
        PackageLayout,
        PackageScript,
        ImportPackageRequest,
        ImportPackageResponse,
        IndustryPackSummary,
        PackMarketplace,
        PackSeed,
        industry_packs::IndustryPackListResponse,
        industry_packs::IndustryPackDetail,
        industry_packs::InstallPackResponse,
        industry_packs::UninstallPackResponse,
    )),
    tags(
        (name = "custom-entities", description = "Custom entity definition CRUD + publish"),
        (name = "custom-records", description = "Records for published custom entities"),
        (name = "custom-views", description = "Saved list views"),
        (name = "custom-layouts", description = "Form layouts"),
        (name = "custom-scripts", description = "Lifecycle scripts (before_save / after_save)"),
        (name = "custom-packages", description = "Export / additive import packages"),
        (name = "custom-industry-packs", description = "Phase 4.5 industry vertical packs"),
    ),
    info(
        title = "CompanyOS Custom / Low-code API",
        version = "0.1.0",
        description = "Phase 4.4/4.5 — custom entities, packages, and industry packs."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/custom/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
