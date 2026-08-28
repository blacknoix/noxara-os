//! OpenAPI 3.1 document for the CompanyOS Inventory & Procurement API
//! (`/api/v1/inventory/...`).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::handlers::{
    assets, goods_receipts, items, movements, purchase_orders, purchase_requests, suppliers,
    vendor_bills, warehouses,
};
use crate::state::AppState;
use crate::types::*;

#[derive(OpenApi)]
#[openapi(
    paths(
        warehouses::list_warehouses,
        warehouses::create_warehouse,
        warehouses::get_warehouse,
        warehouses::update_warehouse,
        items::list_items,
        items::create_item,
        items::get_item,
        items::update_item,
        items::get_item_stock,
        movements::list_movements,
        movements::create_movement,
        movements::reconcile_stock,
        suppliers::list_suppliers,
        suppliers::create_supplier,
        suppliers::get_supplier,
        suppliers::update_supplier,
        purchase_requests::list_purchase_requests,
        purchase_requests::create_purchase_request,
        purchase_requests::get_purchase_request,
        purchase_requests::submit_purchase_request,
        purchase_requests::decide_purchase_request,
        purchase_orders::list_purchase_orders,
        purchase_orders::create_purchase_order,
        purchase_orders::get_purchase_order,
        purchase_orders::issue_purchase_order,
        goods_receipts::list_goods_receipts,
        goods_receipts::create_goods_receipt,
        goods_receipts::get_goods_receipt,
        goods_receipts::post_goods_receipt,
        assets::list_assets,
        assets::create_asset,
        assets::get_asset,
        assets::update_asset,
        assets::assign_asset,
        assets::return_asset,
        assets::depreciate_asset,
        assets::create_maintenance_schedule,
        assets::list_maintenance_due,
        vendor_bills::create_vendor_bill_from_receipt,
        vendor_bills::pay_vendor_bill,
    ),
    components(schemas(
        WarehouseDto,
        WarehouseListResponse,
        CreateWarehouseRequest,
        UpdateWarehouseRequest,
        InventoryItemDto,
        InventoryItemListResponse,
        CreateInventoryItemRequest,
        UpdateInventoryItemRequest,
        StockLevelDto,
        StockLevelListResponse,
        StockMovementDto,
        StockMovementListResponse,
        CreateStockMovementRequest,
        ReconcileStockRequest,
        ReconcileStockResponse,
        DriftAlertDto,
        SupplierDto,
        SupplierListResponse,
        CreateSupplierRequest,
        UpdateSupplierRequest,
        PurchaseRequestLineDto,
        PurchaseRequestDto,
        PurchaseRequestListResponse,
        CreatePurchaseRequestLineRequest,
        CreatePurchaseRequestRequest,
        DecidePurchaseRequestRequest,
        PurchaseOrderLineDto,
        PurchaseOrderDto,
        PurchaseOrderListResponse,
        CreatePurchaseOrderLineRequest,
        CreatePurchaseOrderRequest,
        GoodsReceiptLineDto,
        GoodsReceiptDto,
        GoodsReceiptListResponse,
        CreateGoodsReceiptLineRequest,
        CreateGoodsReceiptRequest,
        InventoryAssetDto,
        InventoryAssetListResponse,
        CreateInventoryAssetRequest,
        UpdateInventoryAssetRequest,
        AssetAssignmentDto,
        AssignAssetRequest,
        ReturnAssetRequest,
        DepreciateAssetRequest,
        DepreciateAssetResponse,
        MaintenanceScheduleDto,
        MaintenanceScheduleListResponse,
        CreateMaintenanceScheduleRequest,
        CreateVendorBillFromReceiptRequest,
        PayVendorBillRequest,
        VendorBillProxyDto,
    )),
    tags(
        (name = "inventory-warehouses", description = "Warehouse master data"),
        (name = "inventory-items", description = "SKU master data + stock levels"),
        (name = "inventory-movements", description = "Append-only stock ledger + reconciliation"),
        (name = "inventory-suppliers", description = "Supplier master data"),
        (name = "inventory-purchase-requests", description = "Purchase requests (draft -> approved -> converted)"),
        (name = "inventory-purchase-orders", description = "Purchase orders issued to suppliers"),
        (name = "inventory-goods-receipts", description = "Goods receipt notes (partial receipt supported)"),
        (name = "inventory-assets", description = "Fixed asset register, assignment, depreciation, maintenance"),
        (name = "inventory-procure-to-pay", description = "Vendor bill proxy to finance-service"),
    ),
    info(
        title = "CompanyOS Inventory & Procurement API",
        version = "0.1.0",
        description = "Phase 2.5 Inventory & Procurement — warehouses, items, stock ledger (weighted-average valuation), suppliers, procure-to-pay (purchase request -> purchase order -> goods receipt -> vendor bill), and fixed assets."
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/inventory/openapi.json",
        get(|| async { Json(ApiDoc::openapi()) }),
    )
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi json")
}
