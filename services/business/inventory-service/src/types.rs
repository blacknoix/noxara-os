//! DTOs + request/response bodies for `/api/v1/inventory/...`.
//!
//! DTO names are distinct from every other service's schemas (never
//! `AssetDto`/`TaskDto` — those belong to HR / Operations).

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

pub const MOVEMENT_TYPES: &[&str] = &[
    "receipt",
    "issue",
    "adjustment",
    "transfer_in",
    "transfer_out",
    "return",
];

pub const PURCHASE_REQUEST_STATUSES: &[&str] = &[
    "draft",
    "pending_approval",
    "approved",
    "rejected",
    "cancelled",
    "converted",
];

pub const PURCHASE_ORDER_STATUSES: &[&str] = &[
    "draft",
    "issued",
    "partially_received",
    "received",
    "cancelled",
];

pub const ASSET_STATUSES: &[&str] = &["in_stock", "assigned", "maintenance", "disposed"];

#[derive(Debug, Deserialize, Default, IntoParams)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Warehouses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WarehouseDto {
    pub id: String,
    pub code: String,
    pub name: String,
    pub location: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WarehouseListResponse {
    pub items: Vec<WarehouseDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateWarehouseRequest {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct UpdateWarehouseRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InventoryItemDto {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub description: Option<String>,
    pub uom: String,
    pub currency: String,
    pub reorder_point_qty: i64,
    pub allow_negative_stock: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InventoryItemListResponse {
    pub items: Vec<InventoryItemDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateInventoryItemRequest {
    pub sku: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub uom: Option<String>,
    pub currency: String,
    #[serde(default)]
    pub reorder_point_qty: Option<i64>,
    #[serde(default)]
    pub allow_negative_stock: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct UpdateInventoryItemRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub reorder_point_qty: Option<i64>,
    #[serde(default)]
    pub allow_negative_stock: Option<bool>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StockLevelDto {
    pub warehouse_id: String,
    pub item_id: String,
    pub qty_on_hand: i64,
    pub avg_unit_cost_minor: i64,
    pub last_movement_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StockLevelListResponse {
    pub items: Vec<StockLevelDto>,
}

// ---------------------------------------------------------------------------
// Stock movements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StockMovementDto {
    pub id: String,
    pub warehouse_id: String,
    pub item_id: String,
    pub qty_delta: i64,
    pub unit_cost_minor: i64,
    pub currency: String,
    pub movement_type: String,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub memo: Option<String>,
    pub created_at: String,
    /// Present only when this movement was an issue/transfer-out and a COGS
    /// journal was posted to finance-service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cogs_journal_public_id: Option<String>,
    #[serde(default)]
    pub qty_on_hand_after: i64,
    #[serde(default)]
    pub avg_unit_cost_minor_after: i64,
    #[serde(default)]
    pub low_stock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StockMovementListResponse {
    pub items: Vec<StockMovementDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateStockMovementRequest {
    pub warehouse_id: String,
    pub item_id: String,
    /// Signed quantity delta. Positive for receipt/return/transfer_in,
    /// negative for issue/transfer_out. `adjustment` may be either sign.
    pub qty_delta: i64,
    #[serde(default)]
    pub unit_cost_minor: Option<i64>,
    pub movement_type: String,
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub memo: Option<String>,
}

#[derive(Debug, Deserialize, Default, IntoParams)]
pub struct MovementListQuery {
    pub warehouse_id: Option<String>,
    pub item_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ReconcileStockRequest {
    #[serde(default)]
    pub warehouse_id: Option<String>,
    #[serde(default)]
    pub item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DriftAlertDto {
    pub id: String,
    pub warehouse_id: String,
    pub item_id: String,
    pub cached_qty: i64,
    pub movement_sum_qty: i64,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconcileStockResponse {
    pub checked: i64,
    pub drift_count: i64,
    pub alerts: Vec<DriftAlertDto>,
}

// ---------------------------------------------------------------------------
// Suppliers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SupplierDto {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub currency: String,
    pub payment_terms: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SupplierListResponse {
    pub items: Vec<SupplierDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateSupplierRequest {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    pub currency: String,
    #[serde(default)]
    pub payment_terms: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct UpdateSupplierRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub payment_terms: Option<String>,
}

// ---------------------------------------------------------------------------
// Purchase requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseRequestLineDto {
    pub id: String,
    pub item_id: String,
    pub qty: i64,
    pub unit_cost_estimate_minor: i64,
    pub line_amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseRequestDto {
    pub id: String,
    pub status: String,
    pub requester_user_id: String,
    pub approval_id: Option<String>,
    pub currency: String,
    pub total_amount_minor: i64,
    pub budget_account_code: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<PurchaseRequestLineDto>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseRequestListResponse {
    pub items: Vec<PurchaseRequestDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePurchaseRequestLineRequest {
    pub item_id: String,
    pub qty: i64,
    pub unit_cost_estimate_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePurchaseRequestRequest {
    pub currency: String,
    #[serde(default)]
    pub budget_account_code: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub lines: Vec<CreatePurchaseRequestLineRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DecidePurchaseRequestRequest {
    pub approve: bool,
    #[serde(default)]
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Purchase orders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseOrderLineDto {
    pub id: String,
    pub item_id: String,
    pub warehouse_id: String,
    pub qty_ordered: i64,
    pub qty_received: i64,
    pub unit_cost_minor: i64,
    pub line_amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseOrderDto {
    pub id: String,
    pub supplier_id: String,
    pub purchase_request_id: Option<String>,
    pub status: String,
    pub currency: String,
    pub total_amount_minor: i64,
    pub issued_at: Option<String>,
    pub lines: Vec<PurchaseOrderLineDto>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseOrderListResponse {
    pub items: Vec<PurchaseOrderDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePurchaseOrderLineRequest {
    pub item_id: String,
    pub warehouse_id: String,
    pub qty_ordered: i64,
    pub unit_cost_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePurchaseOrderRequest {
    pub supplier_id: String,
    #[serde(default)]
    pub purchase_request_id: Option<String>,
    pub currency: String,
    pub lines: Vec<CreatePurchaseOrderLineRequest>,
}

// ---------------------------------------------------------------------------
// Goods receipts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoodsReceiptLineDto {
    pub id: String,
    pub po_line_id: String,
    pub item_id: String,
    pub warehouse_id: String,
    pub qty_received: i64,
    pub unit_cost_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoodsReceiptDto {
    pub id: String,
    pub purchase_order_id: String,
    pub status: String,
    pub received_at: Option<String>,
    pub journal_public_id: Option<String>,
    pub lines: Vec<GoodsReceiptLineDto>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoodsReceiptListResponse {
    pub items: Vec<GoodsReceiptDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateGoodsReceiptLineRequest {
    pub po_line_id: String,
    pub qty_received: i64,
    /// Defaults to the PO line's `unit_cost_minor` when omitted.
    #[serde(default)]
    pub unit_cost_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateGoodsReceiptRequest {
    pub purchase_order_id: String,
    pub lines: Vec<CreateGoodsReceiptLineRequest>,
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InventoryAssetDto {
    pub id: String,
    pub item_id: Option<String>,
    pub name: String,
    pub asset_tag: Option<String>,
    pub status: String,
    pub acquisition_cost_minor: i64,
    pub currency: String,
    pub acquired_at: Option<String>,
    pub useful_life_months: i32,
    pub salvage_minor: i64,
    pub accumulated_depreciation_minor: i64,
    pub last_depreciated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InventoryAssetListResponse {
    pub items: Vec<InventoryAssetDto>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateInventoryAssetRequest {
    pub name: String,
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub asset_tag: Option<String>,
    pub acquisition_cost_minor: i64,
    pub currency: String,
    #[serde(default)]
    pub acquired_at: Option<String>,
    #[serde(default)]
    pub useful_life_months: Option<i32>,
    #[serde(default)]
    pub salvage_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct UpdateInventoryAssetRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub asset_tag: Option<String>,
    #[serde(default)]
    pub useful_life_months: Option<i32>,
    #[serde(default)]
    pub salvage_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssetAssignmentDto {
    pub id: String,
    pub asset_id: String,
    pub assignee_employee_public_id: String,
    pub assigned_at: String,
    pub returned_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssignAssetRequest {
    pub assignee_employee_public_id: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ReturnAssetRequest {
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct DepreciateAssetRequest {
    /// ISO date to depreciate through (defaults to today).
    #[serde(default)]
    pub as_of_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DepreciateAssetResponse {
    pub asset: InventoryAssetDto,
    pub depreciation_expense_minor: i64,
    pub journal_public_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MaintenanceScheduleDto {
    pub id: String,
    pub asset_id: String,
    pub title: String,
    pub interval_days: i32,
    pub next_due_at: String,
    pub last_completed_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MaintenanceScheduleListResponse {
    pub items: Vec<MaintenanceScheduleDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateMaintenanceScheduleRequest {
    pub asset_id: String,
    pub title: String,
    pub interval_days: i32,
    pub next_due_at: String,
    #[serde(default)]
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// Procure-to-pay: vendor bill proxy (calls finance-service)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateVendorBillFromReceiptRequest {
    pub goods_receipt_id: String,
    pub supplier_ref: String,
    #[serde(default)]
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct PayVendorBillRequest {
    /// Defaults to the full outstanding balance when omitted.
    #[serde(default)]
    pub amount_minor: Option<i64>,
    #[serde(default)]
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VendorBillProxyDto {
    pub id: String,
    pub supplier_ref: String,
    pub source_type: String,
    pub source_id: Option<String>,
    pub currency: String,
    pub amount_minor: i64,
    pub amount_paid_minor: i64,
    pub status: String,
    pub payment_journal_public_id: Option<String>,
}
