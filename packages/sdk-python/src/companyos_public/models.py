"""Generated models — companyos-python-sdk-gen@1.0.0. Do not edit."""
from __future__ import annotations
from typing import Any, Optional
from pydantic import BaseModel, ConfigDict

class AbsenceReportResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    from: str
    items: list[AbsenceReportRowDto]
    to: str

class AbsenceReportRowDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    employee_display_name: str
    employee_id: str
    leave_type_code: str
    request_count: int
    units_days: str
    units_milli: int

class AcceptInviteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    display_name: str | None = None
    password: str | None = None
    token: str

class AccessAuditResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    all_cleared: bool
    checklist: list[AccessChecklistItem]
    employee_id: str
    user_id: str | None = None

class AccessChecklistItem(BaseModel):
    model_config = ConfigDict(extra="allow")
    cleared: bool
    detail: str
    path: str

class AccessReviewKickoffRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    period_end: str
    period_start: str
    permission_id: str

class AccessReviewQuery(BaseModel):
    model_config = ConfigDict(extra="allow")
    period_end: str
    period_start: str
    permission_id: str

class AccessReviewRunView(BaseModel):
    model_config = ConfigDict(extra="allow")
    completed_at: str | None = None
    created_at: str
    id: str
    period_end: str
    period_start: str
    permission_id: str
    status: str
    summary: str

class AccrueLeaveRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    effective_date: str | None = None
    employee_id: str
    leave_type_id: str
    note: str | None = None
    units_milli: str | None = None

class ActionCatalogueEntry(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    high_risk: bool
    http_method: str
    http_path: str
    key: str
    required_permission: str

class ActionCatalogueResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ActionCatalogueEntry]

class ActivityDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str | None = None
    created_at: str
    customer_id: str | None = None
    deal_id: str | None = None
    id: str
    kind: str
    lead_id: str | None = None
    occurred_at: str
    owner_user_id: str | None = None
    subject: str | None = None

class ActivityListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ActivityDto]

class ActivityVolumeItem(BaseModel):
    model_config = ConfigDict(extra="allow")
    count: int
    kind: str

class AgeingBucket(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    label: str

class AiSettings(BaseModel):
    model_config = ConfigDict(extra="allow")
    auto_execute_allow_list: list[str]
    budget_month: str
    data_sharing: DataSharingSettings
    model_preference: str
    modules_enabled: ModulesEnabled
    monthly_token_budget: int
    tokens_used_this_month: int

class AllocatePaymentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    invoice_id: str

class AnalyticsIngestResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    accepted: bool
    duplicate: bool
    fact: str | None = None

class AnalyticsReconcileResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    expected_count: int
    matched: bool
    mirror_count: int

class ApiKeyExchangeRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    key_hash: str

class ApiKeyExchangeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    access_token: str
    api_key_id: str
    org_id: str
    rate_limit_per_minute: int
    rate_limit_rpm: str | None = None  # deprecated
    scopes: list[str]

class ApiKeyListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ApiKeyView]

class ApiKeyView(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    expires_at: str | None = None
    id: str
    key_prefix: str
    last_used_at: str | None = None
    name: str
    revoked_at: str | None = None
    scopes: list[str]

class ApplySalesEventRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    envelope: str

class ApplySalesEventResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    applied: bool
    project_id: str | None = None

class ApprovalDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    category: str | None = None
    created_at: str
    currency: str | None = None
    current_step: int
    decided_at: str | None = None
    decided_by: str | None = None
    decision_note: str | None = None
    id: str
    mode: str
    policy_id: str
    policy_version: int
    requester_user_id: str
    routing_snapshot: RoutingSnapshot
    status: str
    steps: list[ApprovalStepDto]
    subject_id: str
    subject_type: str
    summary: str | None = None
    title: str
    updated_at: str

class ApprovalListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ApprovalDto]
    total: int

class ApprovalMode(BaseModel):
    model_config = ConfigDict(extra="allow")

class ApprovalPolicyDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    current_version: int
    definition: PolicyDefinition
    id: str
    is_active: bool
    name: str
    subject_type: str
    updated_at: str

class ApprovalStepDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approver_role: str | None = None
    assignee_user_ids: list[str]
    decided_at: str | None = None
    decided_by: str | None = None
    escalate_to_role: str | None = None
    escalated_at: str | None = None
    order: int
    sla_seconds: str | None = None
    status: str

class AskForm(BaseModel):
    model_config = ConfigDict(extra="allow")
    action_type: str
    fields: list[AskFormField]
    proposal_preview: str | None = None

class AskFormField(BaseModel):
    model_config = ConfigDict(extra="allow")
    label: str
    name: str
    type: str
    value: str

class AskRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    page_scope: str | None = None
    query: str

class AskResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    citations: str | None = None
    form: str | None = None
    kind: str
    message: str | None = None
    tool_trace: str | None = None

class AssetAssignmentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_id: str
    assigned_at: str
    assignee_employee_public_id: str
    id: str
    notes: str | None = None
    returned_at: str | None = None

class AssetDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_tag: str | None = None
    assigned_at: str
    employee_id: str
    id: str
    label: str
    returned_at: str | None = None
    status: str

class AssetListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[AssetDto]

class AssignAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_employee_public_id: str
    notes: str | None = None

class AssignTerritoryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    deal_id: str | None = None

class AttendanceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    accuracy_meters: str | None = None
    created_at: str
    employee_id: str
    entry_kind: str
    id: str
    latitude: str | None = None
    local_date: str
    longitude: str | None = None
    note: str | None = None
    recorded_at: str
    reverses_id: str | None = None
    source: str
    timezone: str

class AttendanceImportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    batch_key: str | None = None
    csv: str

class AttendanceImportResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    imported: int
    items: list[AttendanceDto]
    skipped: int

class AttendanceListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[AttendanceDto]
    total: int

class AuditReadRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    action: str
    created_at: str
    email: str
    metadata: str
    resource_id: str
    resource_type: str
    user_id: str

class AuditVerifyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    partition_key: str | None = None

class AuditVerifyResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    first_break: str | None = None
    ok: bool
    partitions_checked: int
    rows_checked: int

class BalanceSheetResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of: str
    assets: list[ReportLine]
    assets_total_minor: int
    currency: str
    equity: list[ReportLine]
    equity_total_minor: int
    liabilities: list[ReportLine]
    liabilities_total_minor: int
    period_id: str | None = None

class BankAccountDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_number_mask: str | None = None
    currency: str
    id: str
    institution: str | None = None
    is_active: bool
    ledger_account_id: str
    name: str

class BankStatementDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    bank_account_id: str
    closing_minor: int
    currency: str
    id: str
    line_count: int
    opening_minor: int
    source: str
    statement_date: str

class BenchmarkMetric(BaseModel):
    model_config = ConfigDict(extra="allow")
    current_value: int
    display_name: str
    metric: str
    previous_value: int
    trend_percent: str | None = None
    unit: MetricUnit

class BenchmarkResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    benchmarks: list[BenchmarkMetric]
    org_id: str
    window_days: int

class BoardColumnDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    status: str
    tasks: list[TaskDto]

class BoardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    pipeline: PipelineDto
    stages: list[BoardStage]

class BoardStage(BaseModel):
    model_config = ConfigDict(extra="allow")
    deals: list[DealDto]
    stage: StageDto

class BranchArm(BaseModel):
    model_config = ConfigDict(extra="allow")
    equals: str
    next: str
    path: str

class BulkDecideRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    comment: str | None = None
    ids: list[str]

class BulkDecideResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    decided: list[ApprovalDto]
    skipped: list[str]

class CalendarEventDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_id: str | None = None
    due_at: str
    id: str
    project_id: str
    status: str
    title: str

class CalendarResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    events: list[CalendarEventDto]

class CapabilityPreviewResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    allowed: list[str]
    denied_sensitive: list[str]
    role_id: str

class CapacityAllocationDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    capacity_minutes: int
    created_at: str
    id: str
    membership_user_id: str
    period_end: str
    period_start: str
    project_id: str | None = None
    updated_at: str

class CapacityAllocationListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[CapacityAllocationDto]
    total: int

class CapacityOverloadResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[CapacityOverloadRow]

class CapacityOverloadRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    booked_minutes: int
    capacity_minutes: int
    member_id: str
    overload_minutes: int

class CardTransactionDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    description: str | None = None
    id: str
    matched_expense_id: str | None = None
    merchant: str | None = None
    reference: str | None = None
    status: str
    txn_date: str

class CarryForwardRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    year: int

class CarryForwardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    entries_posted: int
    idempotent_replay: bool
    status: str
    workflow_id: str
    year: int

class CashFlowPoint(BaseModel):
    model_config = ConfigDict(extra="allow")
    inflow_minor: int
    outflow_minor: int
    period: str

class CategoryAmount(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    category: str

class CategoryLimitDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    category_code: str
    currency: str
    max_amount_minor: int

class CategoryLimitInput(BaseModel):
    model_config = ConfigDict(extra="allow")
    category_code: str
    currency: str | None = None
    max_amount_minor: int

class ChangeRoleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    role: str

class ChatRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    message: str
    page_scope: str | None = None
    session_id: str | None = None
    stream: str | None = None

class ChatResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    citations: list[Citation]
    content: str
    follow_ups: list[str]
    interaction_id: str
    proposals: list[ProposalView]
    role: str
    session_id: str
    tool_trace: list[ToolTraceEntry]
    usage: TokenUsage

class ChecklistItemDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    is_done: bool
    position: int
    title: str

class Citation(BaseModel):
    model_config = ConfigDict(extra="allow")
    href: str | None = None
    record_id: str
    record_type: str
    snippet: str | None = None
    title: str

class ClosePeriodRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    checklist: str | None = None

class CompensationComponentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    component_type: str
    contract_id: str | None = None
    created_at: str
    currency: str
    effective_from: str
    effective_to: str | None = None
    employee_id: str
    id: str
    label: str
    version: int

class CompensationListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[CompensationComponentDto]

class ConfirmProposalRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    note: str | None = None

class ContactDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    customer_id: str
    email: str | None = None
    first_name: str
    id: str
    is_primary: bool
    last_name: str
    owner_user_id: str | None = None
    phone: str | None = None
    title: str | None = None
    updated_at: str

class ContactListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ContactDto]

class ContractDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    contract_type: str
    created_at: str
    effective_from: str
    effective_to: str | None = None
    employee_id: str
    id: str
    notes: str | None = None
    title: str | None = None
    version: int

class ContractListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ContractDto]

class ConvertLeadRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    currency: str | None = None
    deal_name: str | None = None
    force: bool | None = None

class ConvertLeadResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer: CustomerDto
    deal: DealDto
    lead: LeadDto

class CreateActivityRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str | None = None
    customer_id: str | None = None
    deal_id: str | None = None
    kind: str
    lead_id: str | None = None
    occurred_at: str | None = None
    owner_user_id: str | None = None
    subject: str | None = None

class CreateApiKeyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    expires_at: str | None = None
    name: str
    scopes: list[str]

class CreateApiKeyResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    key: ApiKeyView
    secret: str

class CreateApprovalRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    category: str | None = None
    currency: str | None = None
    department_id: str | None = None
    requester_role: str | None = None
    subject_id: str
    subject_type: str
    summary: str | None = None
    title: str | None = None

class CreateAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_tag: str | None = None
    label: str

class CreateAttachmentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    byte_size: str | None = None
    content_type: str | None = None
    file_name: str
    url: str

class CreateBankAccountRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_number_mask: str | None = None
    currency: str
    institution: str | None = None
    ledger_account_id: str
    name: str

class CreateCapacityAllocationRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    capacity_minutes: int
    membership_user_id: str
    period_end: str
    period_start: str
    project_id: str | None = None

class CreateCommentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str

class CreateCompensationRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    component_type: str | None = None
    contract_id: str | None = None
    currency: str
    effective_from: str
    effective_to: str | None = None
    label: str

class CreateContactRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str | None = None
    first_name: str
    is_primary: bool | None = None
    last_name: str
    owner_user_id: str | None = None
    phone: str | None = None
    title: str | None = None

class CreateContractRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    contract_type: str | None = None
    effective_from: str
    effective_to: str | None = None
    notes: str | None = None
    title: str | None = None

class CreateCreditNoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    invoice_id: str
    lines: list[InvoiceLineInput]
    reason: str | None = None

class CreateCustomerRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    billing_address: str | None = None
    email: str | None = None
    name: str
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    website: str | None = None

class CreateCustomerResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer: CustomerDto
    duplicate_warnings: list[DuplicateMatch] | None = None

class CreateDashboardRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    layout: str | None = None
    name: str

class CreateDealRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int | None = None
    currency: str | None = None
    customer_id: str | None = None
    expected_close_date: str | None = None
    lead_id: str | None = None
    name: str
    owner_user_id: str | None = None
    pipeline_id: str | None = None
    probability: str | None = None
    stage_id: str | None = None

class CreateDelegationRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    ends_at: str | None = None
    to_user_id: str

class CreateDepartmentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    name: str
    parent_id: str | None = None

class CreateDocumentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    doc_type: str | None = None
    expires_at: str | None = None
    file_id: str | None = None
    title: str

class CreateDunningProfileRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    is_default: bool | None = None
    name: str
    steps: list[DunningStepDto]

class CreateEmployeeRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    bank_details: str | None = None
    department_id: str | None = None
    display_name: str
    government_id: str | None = None
    legal_first_name: str | None = None
    legal_last_name: str | None = None
    location: str | None = None
    manager_employee_id: str | None = None
    personal_email: str | None = None
    phone: str | None = None
    start_date: str | None = None
    status: str | None = None
    tax_id: str | None = None
    title: str | None = None
    user_id: str | None = None
    work_email: str | None = None

class CreateFinanceEntityRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str
    currency: str | None = None
    is_default: bool | None = None
    name: str

class CreateFiscalPeriodRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str
    end_date: str
    name: str
    start_date: str

class CreateGoodsReceiptLineRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    po_line_id: str
    qty_received: int
    unit_cost_minor: str | None = None

class CreateGoodsReceiptRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    lines: list[CreateGoodsReceiptLineRequest]
    purchase_order_id: str

class CreateHelloRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    message: str

class CreateHolidayRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    half_day_period: str | None = None
    holiday_date: str
    is_half_day: str | None = None
    location: str | None = None
    name: str

class CreateInventoryAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    acquired_at: str | None = None
    acquisition_cost_minor: int
    asset_tag: str | None = None
    currency: str
    item_id: str | None = None
    name: str
    salvage_minor: str | None = None
    useful_life_months: str | None = None

class CreateInventoryItemRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    allow_negative_stock: str | None = None
    currency: str
    description: str | None = None
    name: str
    reorder_point_qty: str | None = None
    sku: str
    uom: str | None = None

class CreateInvoiceFromQuoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    customer_id: str
    customer_name: str
    lines: list[QuoteLineSnapshot]
    notes: str | None = None
    quote_id: str
    terms: str | None = None
    total_minor: str | None = None

class CreateInvoiceRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    base_currency: str | None = None
    currency: str
    customer_id: str
    due_date: str | None = None
    entity_id: str | None = None
    lines: list[InvoiceLineInput]
    notes: str | None = None
    terms: str | None = None

class CreateLeadRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    company_name: str | None = None
    email: str | None = None
    name: str
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    source: str | None = None

class CreateLeaveRequestRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    employee_id: str | None = None
    end_date: str
    end_period: str | None = None
    leave_type_id: str
    reason: str | None = None
    start_date: str
    start_period: str | None = None
    submit: str | None = None
    timezone: str | None = None

class CreateLeaveTypeRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    accrual_cadence: str | None = None
    accrual_units_milli: str | None = None
    allows_half_day: str | None = None
    carry_forward_cap_milli: str | None = None
    category: str | None = None
    code: str
    expiry_days: str | None = None
    name: str
    requires_approval: str | None = None

class CreateLedgerAccountRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_type: str
    code: str
    description: str | None = None
    name: str
    normal_balance: str | None = None
    parent_id: str | None = None
    sort_order: str | None = None

class CreateMaintenanceScheduleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_id: str
    interval_days: int
    next_due_at: str
    notes: str | None = None
    title: str

class CreateMeetingSummaryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    calendar_event_id: str
    starts_at: str | None = None
    title: str | None = None
    transcript: str | None = None

class CreateOrderLineRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    discount_minor: int | None = None
    product_id: str | None = None
    quantity: int
    tax_rate_bps: int | None = None
    unit_price_minor: int

class CreateOrderRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    customer_id: str
    deal_id: str | None = None
    lines: list[CreateOrderLineRequest] | None = None
    notes: str | None = None
    owner_user_id: str | None = None
    quote_id: str | None = None
    territory_id: str | None = None

class CreateOrgRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    business_type: str | None = None
    currency: str | None = None
    name: str
    region: str | None = None
    timezone: str | None = None

class CreatePayrollComponentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    calc_method: str
    code: str
    config_json: dict[str, Any]
    currency: str | None = None
    label: str
    line_kind: str
    sort_order: str | None = None

class CreatePayrollRunRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    adjustment_of_run_id: str | None = None
    currency: str
    period_end: str
    period_start: str

class CreatePolicyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: PolicyDefinition
    name: str
    subject_type: str

class CreateProductRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    active: bool | None = None
    currency: str | None = None
    name: str
    sku: str | None = None
    tax_group: str | None = None
    unit_price_minor: str | None = None

class CreateProjectRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    deal_id: str | None = None
    description: str | None = None
    due_at: str | None = None
    name: str
    owner_user_id: str | None = None
    starts_at: str | None = None
    status: str | None = None

class CreatePurchaseOrderLineRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    item_id: str
    qty_ordered: int
    unit_cost_minor: int
    warehouse_id: str

class CreatePurchaseOrderRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    lines: list[CreatePurchaseOrderLineRequest]
    purchase_request_id: str | None = None
    supplier_id: str

class CreatePurchaseRequestLineRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    item_id: str
    qty: int
    unit_cost_estimate_minor: int

class CreatePurchaseRequestRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    budget_account_code: str | None = None
    currency: str
    lines: list[CreatePurchaseRequestLineRequest]
    notes: str | None = None

class CreateQuoteLineRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    discount_minor: int | None = None
    product_id: str | None = None
    quantity: int
    tax_rate_bps: int | None = None
    unit_price_minor: int

class CreateQuoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    customer_id: str
    deal_id: str | None = None
    lines: list[CreateQuoteLineRequest] | None = None
    notes: str | None = None
    owner_user_id: str | None = None
    quote_number: str | None = None
    valid_until: str | None = None

class CreateRecurringRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    cadence: str
    customer_id: str
    next_run_at: str
    template: CreateInvoiceRequest

class CreateReimbursementBatchRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    expense_ids: list[str]

class CreateReportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: ReportDefinition
    description: str | None = None
    name: str

class CreateScheduleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str | None = None
    cron: str
    enabled: bool | None = None
    export_format: str | None = None
    recipients: list[str] | None = None
    report_id: str
    timezone: str | None = None

class CreateStockMovementRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    item_id: str
    memo: str | None = None
    movement_type: str
    qty_delta: int
    source_id: str | None = None
    source_type: str | None = None
    unit_cost_minor: str | None = None
    warehouse_id: str

class CreateSupplierRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    email: str | None = None
    name: str
    payment_terms: str | None = None
    phone: str | None = None

class CreateTaskRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_id: str | None = None
    blocked_by: str | None = None
    checklist: str | None = None
    description: str | None = None
    due_at: str | None = None
    labels: str | None = None
    priority: str | None = None
    project_id: str
    status: str | None = None
    title: str

class CreateTaxGroupRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    name: str

class CreateTaxRateRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    component_name: str | None = None
    is_component: bool | None = None
    name: str
    rate_bps: int
    supersedes_id: str | None = None
    tax_group_id: str | None = None
    valid_from: str
    valid_to: str | None = None

class CreateTeamRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    department_id: str | None = None
    lead_user_id: str | None = None
    name: str
    parent_team_id: str | None = None

class CreateTerritoryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    name: str
    owner_user_id: str | None = None

class CreateTimesheetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    membership_user_id: str | None = None
    notes: str | None = None
    week_start: str

class CreateVendorBillFromReceiptRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    goods_receipt_id: str
    memo: str | None = None
    supplier_ref: str

class CreateVendorBillRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    memo: str | None = None
    source_id: str | None = None
    source_type: str | None = None
    supplier_ref: str

class CreateWarehouseRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str
    location: str | None = None
    name: str

class CreateWebhookEndpointRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    event_types: list[str]
    url: str

class CreateWebhookEndpointResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    endpoint: WebhookEndpointView
    secret: str

class CreateWorkScheduleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    is_default: str | None = None
    location: str | None = None
    name: str
    timezone: str | None = None
    weekly_hours: str | None = None

class CreateWorkflowDefinitionRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    graph: WorkflowGraph
    name: str

class CreditNoteDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    credit_number: str
    currency: str
    customer_id: str
    id: str
    invoice_id: str
    issued_at: str
    reason: str | None = None
    subtotal_minor: int
    tax_minor: int
    total_minor: int

class CreditNoteListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[CreditNoteDto]
    total: int

class CustomerDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    billing_address: str | None = None
    created_at: str
    email: str | None = None
    id: str
    name: str
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    updated_at: str
    version: int
    website: str | None = None

class CustomerListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[CustomerDto]
    total: int

class DashboardDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    created_by: str
    description: str
    id: str
    layout: str
    name: str
    org_id: str
    updated_at: str
    updated_by: str
    widgets: list[WidgetDto]

class DashboardListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    dashboards: list[DashboardDto]

class DashboardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of: str
    period: str
    role_layout: str
    widgets: list[DashboardWidget]

class DashboardWidget(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    kind: str
    payload: dict[str, Any]
    range_label: str | None = None
    reason_code: str | None = None
    stale: bool
    status: str
    title: str

class DataSharingSettings(BaseModel):
    model_config = ConfigDict(extra="allow")
    allow_training: bool
    share_with_provider: bool

class DealDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    created_at: str
    currency: str
    customer_id: str | None = None
    expected_close_date: str | None = None
    id: str
    lead_id: str | None = None
    lost_at: str | None = None
    lost_reason: str | None = None
    name: str
    owner_user_id: str | None = None
    pipeline_id: str
    probability: str | None = None
    stage_id: str
    status: str
    updated_at: str
    version: int
    won_at: str | None = None
    won_reason: str | None = None

class DealListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[DealDto]
    total: int

class DecideApprovalRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    comment: str | None = None

class DecideExpenseRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    note: str | None = None

class DecideLeaveRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    note: str | None = None

class DecidePayrollRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    note: str | None = None

class DecidePurchaseRequestRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    note: str | None = None

class DecideReimbursementRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approve: bool
    note: str | None = None

class DecideTimesheetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    note: str | None = None

class DelegationDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    ends_at: str | None = None
    from_user_id: str
    id: str
    revoked_at: str | None = None
    starts_at: str
    to_user_id: str

class DepartmentListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[DepartmentView]

class DepartmentView(BaseModel):
    model_config = ConfigDict(extra="allow")
    department_id: str
    name: str
    parent_id: str | None = None

class DepreciateAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of_date: str | None = None

class DepreciateAssetResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset: InventoryAssetDto
    depreciation_expense_minor: int
    journal_public_id: str | None = None

class DisableWebhookRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str | None = None

class DisqualifyLeadRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str | None = None

class DocumentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    collected: bool
    created_at: str
    doc_type: str
    employee_id: str
    expires_at: str | None = None
    file_id: str | None = None
    id: str
    title: str
    version: int

class DocumentExtractRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    file_id: str | None = None
    kind: str
    text: str | None = None

class DocumentListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[DocumentDto]

class DocumentReview(BaseModel):
    model_config = ConfigDict(extra="allow")
    confidence: float
    extracted: str
    id: str
    kind: str
    proposal_id: str | None = None
    status: str

class DriftAlertDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    cached_qty: int
    detected_at: str
    id: str
    item_id: str
    movement_sum_qty: int
    warehouse_id: str

class DrillRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    link: str
    record_id: str

class DrillRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: ReportDefinition
    limit: str | None = None

class DrillResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    filtered_by_permission: bool
    metric: str
    records: list[DrillRecord]

class DunningProfileDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    id: str
    is_default: bool
    name: str
    steps: list[DunningStepDto]
    updated_at: str
    version: int

class DunningProfileListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[DunningProfileDto]
    total: int

class DunningScheduleQuery(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    invoice_id: str | None = None

class DunningScheduleResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    profile_id: str
    schedule_offsets_days: list[int]
    steps: list[DunningStepDto]

class DunningStepDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str
    label: str
    offset_days: int

class DuplicateCheckResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    matches: list[DuplicateMatch]

class DuplicateMatch(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    email: str | None = None
    lead_id: str | None = None
    name: str
    reason: str
    score: float

class EmployeeDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    bank_details: str | None = None
    created_at: str
    department_id: str | None = None
    display_name: str
    end_date: str | None = None
    government_id: str | None = None
    id: str
    legal_first_name: str | None = None
    legal_last_name: str | None = None
    location: str | None = None
    manager_employee_id: str | None = None
    owner_user_id: str
    personal_email: str | None = None
    phone: str | None = None
    start_date: str | None = None
    status: str
    tax_id: str | None = None
    title: str | None = None
    updated_at: str
    user_id: str | None = None
    version: int
    work_email: str | None = None

class EmployeeListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[EmployeeDto]
    total: int

class EntitlementRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    effective_from: str
    effective_to: str | None = None
    email: str
    permission_id: str
    role_key: str
    user_id: str

class ExpenseDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    approval_id: str | None = None
    category_code: str | None = None
    created_at: str
    currency: str
    description: str
    id: str
    incurred_at: str
    receipt_url: str | None = None
    status: str

class ExpenseListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ExpenseDto]
    total: int

class ExpensePolicyDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    auto_approve_under_minor: int
    category_limits: list[CategoryLimitDto]
    id: str
    is_active: bool
    mileage_rate_minor: int
    mileage_unit: str
    name: str
    over_limit_action: str
    per_diem_minor: int
    require_receipt_over_minor: int

class ExportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    format: str | None = None

class ExportResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    content: str
    content_type: str
    file_id: str
    format: str
    report_id: str
    row_count: int
    run_id: str

class FactSource(BaseModel):
    model_config = ConfigDict(extra="allow")

class FactsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    facts: list[InvoiceIssuedFact]

class FeedResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[NotificationItemDto]

class FileMetaResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    content_type: str
    download_url: str
    file_id: str
    size_bytes: int
    status: str

class FinanceCustomerDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    email: str | None = None
    id: str
    name: str
    outstanding_balance_minor: int
    sales_customer_id: str

class FinanceCustomerListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[FinanceCustomerDto]
    total: int

class FinanceEntityDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str
    created_at: str
    currency: str
    id: str
    is_default: bool
    name: str
    updated_at: str

class FinanceEntityListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[FinanceEntityDto]
    total: int

class FireScheduleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str | None = None
    export_format: str | None = None

class FireScheduleResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    export: ExportResponse
    run_id: str
    schedule_id: str
    state: str
    workflow_id: str
    workflow_type: str

class FiscalPeriodDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    checklist: dict[str, Any]
    closed_at: str | None = None
    code: str
    end_date: str
    id: str
    name: str
    reopen_reason: str | None = None
    reopened_at: str | None = None
    start_date: str
    status: str

class FiscalPeriodListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[FiscalPeriodDto]
    total: int

class FixtureListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[FixtureWorkflowDto]

class FixtureWorkflowDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    graph: WorkflowGraph
    name: str

class ForecastInputs(BaseModel):
    model_config = ConfigDict(extra="allow")
    history_periods: int
    history_values: list[int]
    horizon_periods: int
    method_params: str

class ForecastMethod(BaseModel):
    model_config = ConfigDict(extra="allow")

class ForecastPoint(BaseModel):
    model_config = ConfigDict(extra="allow")
    period_index: int
    period_label: str
    value: int

class ForecastRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    history_periods: int | None = None
    horizon_periods: int | None = None
    method: str | None = None
    org_id: str
    series: str

class ForecastResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    explainability: str
    forecast: list[ForecastPoint]
    history: list[ForecastPoint]
    inputs: ForecastInputs
    method: ForecastMethod
    metric: str
    series: str
    unit: MetricUnit

class FreshnessResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    eventually_consistent: bool
    lag_seconds: int
    last_event_at: str | None = None
    last_ingest_at: str | None = None
    org_id: str

class GoodsReceiptDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    id: str
    journal_public_id: str | None = None
    lines: list[GoodsReceiptLineDto]
    purchase_order_id: str
    received_at: str | None = None
    status: str
    updated_at: str
    version: int

class GoodsReceiptLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    item_id: str
    po_line_id: str
    qty_received: int
    unit_cost_minor: int
    warehouse_id: str

class GoodsReceiptListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[GoodsReceiptDto]
    total: int

class Hello(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_by: str
    id: str
    message: str
    org_id: str

class HelloListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[Hello]

class HolidayDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    half_day_period: str | None = None
    holiday_date: str
    id: str
    is_half_day: bool
    location: str | None = None
    name: str
    version: int

class HolidayListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[HolidayDto]

class HrTaskDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_user_id: str | None = None
    completed_at: str | None = None
    due_at: str | None = None
    employee_id: str
    id: str
    kind: str
    status: str
    title: str
    workflow_id: str | None = None

class HrTaskListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[HrTaskDto]

class HumanStepKind(BaseModel):
    model_config = ConfigDict(extra="allow")

class ImportCardCsvRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    csv: str

class ImportCardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    imported: int
    items: list[CardTransactionDto]

class ImportConfirmRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    csv: str | None = None
    mapping: dict[str, Any] | None = None
    rows: str | None = None
    skip_exact_duplicates: bool | None = None

class ImportConfirmResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    imported: int
    job_id: str
    skipped: int

class ImportPreviewRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    csv: str
    mapping: dict[str, Any] | None = None

class ImportPreviewResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    exact_duplicate_count: int
    near_duplicate_count: int
    rows: list[ImportRowPreview]

class ImportRowInput(BaseModel):
    model_config = ConfigDict(extra="allow")
    company: str | None = None
    email: str | None = None
    name: str | None = None
    phone: str | None = None

class ImportRowPreview(BaseModel):
    model_config = ConfigDict(extra="allow")
    company: str | None = None
    duplicates: list[DuplicateMatch] | None = None
    email: str | None = None
    errors: list[str] | None = None
    name: str | None = None
    phone: str | None = None
    row_number: int

class ImportStatementRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    closing_minor: str | None = None
    csv: str
    opening_minor: str | None = None
    statement_date: str | None = None

class ImportStatementResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    lines_imported: int
    statement: BankStatementDto

class InboxSummaryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    pending_for_me: int

class IngestResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    upserted: bool

class InsightObservation(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str
    estimate: bool
    evidence: list[Citation]
    id: str
    insight_type: str | None = None
    proposal_id: str | None = None
    status: str | None = None
    suggested_action: str | None = None
    suggested_action_detail: str | None = None
    title: str

class InsightsRefreshResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    created: int
    observations: list[InsightObservation]
    pending_proposals: list[str]

class InsightsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    empty_reason: str | None = None
    observations: list[InsightObservation]

class InventoryAssetDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    accumulated_depreciation_minor: int
    acquired_at: str | None = None
    acquisition_cost_minor: int
    asset_tag: str | None = None
    created_at: str
    currency: str
    id: str
    item_id: str | None = None
    last_depreciated_at: str | None = None
    name: str
    salvage_minor: int
    status: str
    updated_at: str
    useful_life_months: int
    version: int

class InventoryAssetListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[InventoryAssetDto]
    total: int

class InventoryItemDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    allow_negative_stock: bool
    created_at: str
    currency: str
    description: str | None = None
    id: str
    is_active: bool
    name: str
    reorder_point_qty: int
    sku: str
    uom: str
    updated_at: str
    version: int

class InventoryItemListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[InventoryItemDto]
    total: int

class InviteMemberRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str
    role: str

class InviteResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str
    expires_at: str
    invitation_id: str
    status: str

class InvoiceActionResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    available: bool
    reason: str

class InvoiceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_credited_minor: int
    amount_paid_minor: int
    balance_minor: int
    base_currency: str
    base_total_minor: int
    created_at: str
    currency: str
    customer_id: str
    discount_minor: int
    due_date: str | None = None
    entity_id: str | None = None
    fx_rate_date: str | None = None
    fx_rate_den: str | None = None
    fx_rate_num: str | None = None
    id: str
    invoice_number: str | None = None
    issue_date: str | None = None
    lines: list[InvoiceLineDto]
    notes: str | None = None
    payment_url: str | None = None
    source_quote_id: str | None = None
    status: str
    subtotal_minor: int
    tax_minor: int
    terms: str | None = None
    total_minor: int
    updated_at: str
    version: int

class InvoiceIssuedFact(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    currency: str | None = None
    event_id: str
    invoice_id: str
    issued_at: str
    org_id: str

class InvoiceLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    discount_minor: int
    id: str
    line_total_minor: int
    quantity: int
    tax_group_id: str | None = None
    tax_minor: int
    tax_rate_bps: int
    tax_rate_id: str | None = None
    unit_price_minor: int

class InvoiceLineInput(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    discount_minor: int | None = None
    quantity: int
    tax_group_id: str | None = None
    tax_rate_bps: int | None = None
    tax_rate_id: str | None = None
    unit_price_minor: int

class InvoiceListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[InvoiceDto]
    total: int

class IssueInvoiceRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    due_date: str | None = None
    fx_rate_date: str | None = None
    fx_rate_den: int | None = None
    fx_rate_num: int | None = None
    issue_date: str | None = None

class JournalEntryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    entry_date: str
    id: str
    lines: list[JournalLineInput]
    memo: str
    period_id: str | None = None
    source_id: str
    source_type: str

class JournalLineInput(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_code: str
    credit_minor: int
    debit_minor: int
    memo: str | None = None

class JournalListQuery(BaseModel):
    model_config = ConfigDict(extra="allow")
    limit: str | None = None
    offset: str | None = None
    period_id: str | None = None
    source_type: str | None = None

class JournalListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[JournalEntryDto]
    total: int

class LeadDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    company_name: str | None = None
    converted_customer_id: str | None = None
    converted_deal_id: str | None = None
    created_at: str
    email: str | None = None
    id: str
    name: str
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    score: int
    source: str | None = None
    status: str
    updated_at: str
    version: int

class LeadListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[LeadDto]
    total: int

class LeaveBalanceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of: str
    balance_days: str
    balance_units_milli: int
    employee_id: str
    leave_type_code: str
    leave_type_id: str
    leave_type_name: str

class LeaveBalanceListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[LeaveBalanceDto]

class LeaveCalendarEntryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    employee_display_name: str
    employee_id: str
    end_date: str
    end_period: str
    leave_request_id: str
    leave_type_code: str
    start_date: str
    start_period: str
    status: str
    units_milli: int

class LeaveCalendarResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[LeaveCalendarEntryDto]

class LeaveLedgerEntryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    effective_date: str
    employee_id: str
    entry_kind: str
    expires_on: str | None = None
    id: str
    leave_request_id: str | None = None
    leave_type_id: str
    note: str | None = None
    units_milli: int

class LeaveRequestDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    created_at: str
    decided_at: str | None = None
    decision_note: str | None = None
    employee_id: str
    end_date: str
    end_period: str
    id: str
    leave_type_id: str
    reason: str | None = None
    start_date: str
    start_period: str
    status: str
    timezone: str
    units_days: str
    units_milli: int
    updated_at: str
    version: int

class LeaveRequestListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[LeaveRequestDto]
    total: int

class LeaveTypeDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    accrual_cadence: str
    accrual_units_milli: int
    allows_half_day: bool
    carry_forward_cap_milli: str | None = None
    category: str
    code: str
    created_at: str
    expiry_days: str | None = None
    id: str
    is_active: bool
    name: str
    requires_approval: bool
    updated_at: str
    version: int

class LeaveTypeListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[LeaveTypeDto]

class LedgerAccountDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_type: str
    code: str
    description: str | None = None
    id: str
    is_active: bool
    name: str
    normal_balance: str
    parent_id: str | None = None
    sort_order: int

class LedgerAccountNode(BaseModel):
    model_config = ConfigDict(extra="allow")
    account: LedgerAccountDto
    children: list[LedgerAccountNode]

class LedgerAccountTreeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    roots: list[LedgerAccountNode]

class ListQuery(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    entity_id: str | None = None
    limit: str | None = None
    offset: str | None = None
    q: str | None = None
    status: str | None = None

class LoginRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    device_label: str | None = None
    email: str
    org_id: str | None = None
    password: str

class LoseDealRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str | None = None

class MagicLinkConsumeRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    device_label: str | None = None
    token: str

class MagicLinkRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str
    org_id: str | None = None

class MaintenanceScheduleDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_id: str
    id: str
    interval_days: int
    last_completed_at: str | None = None
    next_due_at: str
    notes: str | None = None
    title: str

class MaintenanceScheduleListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[MaintenanceScheduleDto]

class MatchCardsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    matched: int
    unmatched: int

class MeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    org_id: str
    policy_version: int
    roles: list[str]
    session_id: str
    user_id: str

class MeasureKind(BaseModel):
    model_config = ConfigDict(extra="allow")

class MeetingSummariesListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[MeetingSummaryView]

class MeetingSummaryView(BaseModel):
    model_config = ConfigDict(extra="allow")
    accepted_at: str | None = None
    accepted_by: str | None = None
    action_items: str
    calendar_connector: str
    calendar_event_id: str
    created_at: str
    id: str
    public_id: str
    status: str
    summary_markdown: str
    transcript: str | None = None

class MemberListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[MemberView]

class MemberView(BaseModel):
    model_config = ConfigDict(extra="allow")
    department_id: str | None = None
    display_name: str
    email: str
    membership_id: str
    policy_version: int
    role: str
    role_id: str | None = None
    role_name: str | None = None
    status: str
    team_id: str | None = None
    user_id: str

class MembershipListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[MembershipView]

class MembershipView(BaseModel):
    model_config = ConfigDict(extra="allow")
    org_id: str
    org_name: str
    policy_version: int
    role: str

class MessageResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    message: str

class MetricDefinition(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    dimensions: list[str]
    display_name: str
    drill_route: str
    fact: FactSource
    flagship: bool
    measure: MeasureKind
    measure_field: str
    name: str
    required_permission: str
    unit: MetricUnit

class MetricListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    metrics: list[MetricDefinition]

class MetricUnit(BaseModel):
    model_config = ConfigDict(extra="allow")

class MfaChallengeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    challenge_token: str
    message: str
    mfa_required: bool

class MfaConfirmRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    challenge_token: str | None = None
    code: str

class MfaConfirmResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    enabled: bool
    recovery_codes: list[str]

class MfaSetupRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    challenge_token: str | None = None

class MfaSetupResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    otpauth_uri: str
    secret: str

class MfaVerifyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    challenge_token: str
    code: str | None = None
    device_label: str | None = None
    recovery_code: str | None = None

class MigrateInstanceRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    target_version: int

class MileageCalculateRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    description: str | None = None
    incurred_at: str | None = None
    miles_or_km: float

class MileageCalculateResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    expense: ExpenseDto
    miles_or_km: float
    rate_minor: int

class ModulesEnabled(BaseModel):
    model_config = ConfigDict(extra="allow")
    ask_mode: bool
    copilot: bool
    document_ai: bool
    insights: bool

class MonitorResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    instances: list[WorkflowInstanceDto]
    summary: MonitorSummaryDto

class MonitorSummaryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    cancelled: int
    completed: int
    failed: int
    running: int
    sla_breached: int
    waiting: int

class MoveTaskRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    position: str | None = None
    status: str

class MyCapabilitiesResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    allowed: list[str]
    org_id: str
    policy_version: int
    role: str

class MyWorkResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    assigned: list[TaskDto]
    mentions: list[TaskCommentDto]
    total_assigned: int

class NotificationIntentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    body_preview: str | None = None
    created_at: str
    id: str
    kind: str
    recipient_user_id: str
    resource_id: str
    resource_type: str

class NotificationItemDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str
    created_at: str
    href: str | None = None
    id: str
    read_at: str | None = None
    resource_id: str | None = None
    resource_type: str | None = None
    title: str

class OffboardRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    end_date: str | None = None
    fail_after: str | None = None
    reason: str | None = None
    reassign_manager_to: str | None = None

class OffboardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    checklist: list[AccessChecklistItem]
    employee: EmployeeDto
    status: str
    workflow_id: str

class OnboardRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_labels: str | None = None
    department_id: str | None = None
    display_name: str
    document_titles: str | None = None
    fail_after: str | None = None
    manager_employee_id: str | None = None
    role: str | None = None
    start_date: str | None = None
    task_titles: str | None = None
    title: str | None = None
    user_id: str | None = None
    work_email: str | None = None

class OnboardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    employee: EmployeeDto
    status: str
    tasks: list[HrTaskDto]
    workflow_id: str

class OrderDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    currency: str
    customer_id: str
    deal_id: str | None = None
    discount_minor: int
    id: str
    lines: list[OrderLineDto]
    notes: str | None = None
    owner_user_id: str | None = None
    quote_id: str | None = None
    status: str
    subtotal_minor: int
    tax_minor: int
    territory_id: str | None = None
    total_minor: int
    updated_at: str
    version: int

class OrderFromDealRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    deal_id: str

class OrderFromQuoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    quote_id: str

class OrderLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    discount_minor: int
    id: str
    line_total_minor: int
    position: int
    product_id: str | None = None
    quantity: int
    tax_minor: int
    tax_rate_bps: int
    unit_price_minor: int

class OrderListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[OrderDto]
    total: int

class OrgBoundsDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    max_concurrent: int
    max_steps_per_instance: int

class OrgResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    branding: str
    business_type: str
    currency: str
    feature_flags: str
    fiscal_year_start_month: int
    name: str
    numbering_series: str
    org_id: str
    plan: str
    region: str
    timezone: str

class PasswordResetConfirm(BaseModel):
    model_config = ConfigDict(extra="allow")
    new_password: str
    token: str

class PasswordResetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str

class PatchTimeEntryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    billable: str | None = None
    entry_date: str | None = None
    minutes: str | None = None
    notes: str | None = None
    project_id: str | None = None
    task_id: str | None = None

class PayVendorBillRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    memo: str | None = None

class PaymentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_allocated_minor: int
    amount_minor: int
    amount_unapplied_minor: int
    currency: str
    customer_id: str
    id: str
    method: str
    notes: str | None = None
    provider: str | None = None
    received_at: str

class PaymentListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PaymentDto]
    total: int

class PayrollComponentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    calc_method: str
    code: str
    config_json: dict[str, Any]
    created_at: str
    currency: str | None = None
    id: str
    is_active: bool
    label: str
    line_kind: str
    sort_order: int
    updated_at: str
    version: int

class PayrollComponentListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PayrollComponentDto]

class PayrollRunDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    adjustment_of_run_id: str | None = None
    approval_id: str | None = None
    approved_at: str | None = None
    calculated_at: str | None = None
    created_at: str
    currency: str
    deductions_minor: int
    employee_count: int
    gross_minor: int
    id: str
    journal_public_id: str | None = None
    net_minor: int
    paid_at: str | None = None
    period_end: str
    period_start: str
    status: str
    updated_at: str
    version: int

class PayrollRunListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PayrollRunDto]
    total: int

class PayslipDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    currency: str
    deductions_minor: int
    employee_id: str
    gross_minor: int
    id: str
    issued_at: str | None = None
    lines: list[PayslipLineDto]
    net_minor: int
    run_id: str
    status: str
    version: int

class PayslipLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    calculation_basis: dict[str, Any]
    component_code: str
    currency: str
    id: str
    label: str
    line_kind: str
    sort_order: int

class PayslipListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PayslipDto]

class PerDiemRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    days: int
    description: str | None = None
    incurred_at: str | None = None

class PerDiemResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    days: int
    expense: ExpenseDto
    per_diem_minor: int

class PermissionCatalogueItem(BaseModel):
    model_config = ConfigDict(extra="allow")
    action: str
    context: str
    description: str
    id: str
    resource: str
    sensitive: bool

class PermissionCatalogueResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PermissionCatalogueItem]

class PipelineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    is_default: bool
    name: str

class PipelineListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PipelineDto]

class PolicyDefinition(BaseModel):
    model_config = ConfigDict(extra="allow")
    match_criteria: PolicyMatch | None = None
    mode: ApprovalMode
    steps: list[PolicyStepDef]

class PolicyListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ApprovalPolicyDto]

class PolicyMatch(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor_gte: str | None = None
    amount_minor_lt: str | None = None
    categories: list[str] | None = None
    department_ids: list[str] | None = None
    discount_bps_gte: str | None = None
    requester_roles: list[str] | None = None

class PolicyStepDef(BaseModel):
    model_config = ConfigDict(extra="allow")
    approver_role: str | None = None
    approver_user_ids: list[str] | None = None
    escalate_to_role: str | None = None
    order: int
    sla_seconds: str | None = None

class PostJournalRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    entry_date: str | None = None
    lines: list[JournalLineInput]
    memo: str | None = None
    reverses_of: str | None = None
    source_id: str | None = None
    source_type: str

class PreferenceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str
    digest_cron: str | None = None
    enabled: bool
    quiet_hours_end: str | None = None
    quiet_hours_start: str | None = None

class PreferencesResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    preferences: list[PreferenceDto]

class PresignUploadRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    content_type: str
    filename: str
    size_bytes: int

class PresignUploadResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    file_id: str
    headers: dict[str, Any]
    upload_url: str

class ProductDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    active: bool
    currency: str | None = None
    id: str
    name: str
    sku: str | None = None
    tax_group: str | None = None
    unit_price_minor: str | None = None

class ProductListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ProductDto]

class ProfitAndLossResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    expense_total_minor: int
    expenses: list[ReportLine]
    from: str | None = None
    net_income_minor: int
    period_id: str | None = None
    revenue: list[ReportLine]
    revenue_total_minor: int
    to: str | None = None

class ProjectDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    customer_id: str | None = None
    deal_id: str | None = None
    description: str | None = None
    due_at: str | None = None
    id: str
    name: str
    owner_user_id: str
    starts_at: str | None = None
    status: str
    updated_at: str
    version: int

class ProjectListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ProjectDto]
    total: int

class ProposalView(BaseModel):
    model_config = ConfigDict(extra="allow")
    action_type: str
    citations: list[Citation]
    command: str
    created_at: str
    id: str
    rendered_diff: str
    status: str
    tool_name: str

class ProposalsListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ProposalView]

class PublishWorkflowRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    note: str | None = None

class PurchaseOrderDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    currency: str
    id: str
    issued_at: str | None = None
    lines: list[PurchaseOrderLineDto]
    purchase_request_id: str | None = None
    status: str
    supplier_id: str
    total_amount_minor: int
    updated_at: str
    version: int

class PurchaseOrderLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    item_id: str
    line_amount_minor: int
    qty_ordered: int
    qty_received: int
    unit_cost_minor: int
    warehouse_id: str

class PurchaseOrderListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PurchaseOrderDto]
    total: int

class PurchaseRequestDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    budget_account_code: str | None = None
    created_at: str
    currency: str
    id: str
    lines: list[PurchaseRequestLineDto]
    notes: str | None = None
    requester_user_id: str
    status: str
    total_amount_minor: int
    updated_at: str
    version: int

class PurchaseRequestLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    item_id: str
    line_amount_minor: int
    qty: int
    unit_cost_estimate_minor: int

class PurchaseRequestListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[PurchaseRequestDto]
    total: int

class PutPreferencesRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    preferences: list[PreferenceDto]

class QueryFilter(BaseModel):
    model_config = ConfigDict(extra="allow")
    field: str
    op: str
    value: str

class QueryResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    hits: list[SearchHit]

class QueryResult(BaseModel):
    model_config = ConfigDict(extra="allow")
    dry_run: bool
    elapsed_ms: int
    eventually_consistent: bool
    filtered_by_permission: bool
    freshness_as_of: str | None = None
    metric: str
    permission_denied_empty: bool
    rows: list[QueryRow]

class QueryRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    dimensions: dict[str, Any]
    drill_links: list[str]
    record_ids: list[str]
    value: int

class QuoteDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    accepted_at: str | None = None
    approval_id: str | None = None
    created_at: str
    currency: str
    customer_id: str
    deal_id: str | None = None
    discount_minor: int
    id: str
    lines: list[QuoteLineDto]
    notes: str | None = None
    owner_user_id: str | None = None
    previous_quote_id: str | None = None
    quote_number: str
    status: str
    subtotal_minor: int
    tax_minor: int
    total_minor: int
    updated_at: str
    valid_until: str | None = None
    version: int
    version_number: int

class QuoteLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    discount_minor: int
    id: str
    line_total_minor: int
    position: int
    product_id: str | None = None
    quantity: int
    tax_minor: int
    tax_rate_bps: int
    unit_price_minor: int

class QuoteLineSnapshot(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str
    discount_minor: int | None = None
    quantity: int
    tax_rate_bps: int | None = None
    unit_price_minor: int

class QuoteListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[QuoteDto]
    total: int

class ReconcileRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    line_ids: str | None = None

class ReconcileResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    match_rate: float
    matched: int
    reconciliations: list[ReconciliationDto]
    unmatched: int

class ReconcileStockRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    item_id: str | None = None
    warehouse_id: str | None = None

class ReconcileStockResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    alerts: list[DriftAlertDto]
    checked: int
    drift_count: int

class ReconciliationDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    auto_matched: bool
    bank_account_id: str
    id: str
    match_kind: str
    matched_payment_id: str | None = None
    statement_line_id: str

class RecordAttendanceRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    accuracy_meters: str | None = None
    employee_id: str | None = None
    entry_kind: str
    latitude: str | None = None
    longitude: str | None = None
    note: str | None = None
    recorded_at: str | None = None
    reverses_id: str | None = None
    source: str | None = None
    timezone: str | None = None

class RecordPaymentRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    customer_id: str
    invoice_id: str | None = None
    notes: str | None = None
    received_at: str | None = None

class RecurringInvoiceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    active: bool
    cadence: str
    customer_id: str
    id: str
    next_run_at: str

class RecurringListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[RecurringInvoiceDto]
    total: int

class RegisterRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    display_name: str
    email: str
    org_name: str
    password: str
    region: str | None = None

class RegisterResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str
    org_id: str
    user_id: str
    verification_required: bool

class ReimbursementBatchDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    created_at: str
    currency: str
    expense_ids: list[str]
    id: str
    status: str
    total_minor: int

class ReindexRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    org_id: str

class ReindexResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    job_id: str
    status: str

class RejectQuoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str | None = None

class RenewalPipelineResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[ContractDto]
    within_days: int

class ReopenPeriodRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str

class ReplayWebhookResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    delivery: WebhookDeliveryView

class ReportDefinition(BaseModel):
    model_config = ConfigDict(extra="allow")
    dimensions: list[str] | None = None
    filters: list[QueryFilter] | None = None
    group_by: list[str] | None = None
    metric: str
    org_id: str | None = None
    region: str | None = None
    visualization: str | None = None

class ReportDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    created_by: str
    definition: ReportDefinition
    description: str
    id: str
    name: str
    org_id: str
    updated_at: str
    updated_by: str
    visualization: str

class ReportLine(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_code: str
    account_name: str
    amount_minor: int

class ReportListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    reports: list[ReportDto]

class ReportSummaryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    ageing: list[AgeingBucket]
    as_of: str
    cash_flow: list[CashFlowPoint]
    cash_minor: int
    currency: str
    expenses_by_category: list[CategoryAmount]
    expenses_minor: int
    receivables_minor: int
    revenue_minor: int

class ReportSummaryResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    activity_volume: list[ActivityVolumeItem]
    pipeline_by_stage: list[StageSummary]
    weighted_forecast: WeightedForecast
    win_rate: WinRateSummary

class ResendVerificationRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str

class ResolvedStepSnapshot(BaseModel):
    model_config = ConfigDict(extra="allow")
    approver_role: str | None = None
    assignee_user_ids: list[str]
    escalate_to_role: str | None = None
    order: int
    sla_seconds: str | None = None

class RetentionConfigView(BaseModel):
    model_config = ConfigDict(extra="allow")
    default_retention_days: int
    overrides: str
    updated_at: str
    version: int

class RetentionDryRunResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    cutoff_date: str
    partitions: list[str]
    would_affect_estimate: int

class ReturnAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    notes: str | None = None

class RoleListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[RoleView]

class RolePermissionInput(BaseModel):
    model_config = ConfigDict(extra="allow")
    effect: str
    permission_id: str
    scope: str | None = None

class RolePermissionView(BaseModel):
    model_config = ConfigDict(extra="allow")
    effect: str
    permission_id: str
    scope: str

class RoleView(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_limit_amount_minor: str | None = None
    approval_limit_currency: str | None = None
    description: str
    is_system: bool
    name: str
    permissions: list[RolePermissionView]
    role_id: str
    system_key: str | None = None

class RotateApiKeyResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    key: ApiKeyView
    secret: str

class RotateWebhookSecretResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    endpoint: WebhookEndpointView
    secret: str

class RoutingSnapshot(BaseModel):
    model_config = ConfigDict(extra="allow")
    match_criteria: PolicyMatch
    mode: ApprovalMode
    policy_name: str
    policy_public_id: str
    policy_version: int
    rationale: str
    steps: list[ResolvedStepSnapshot]

class RunDueResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_invoice_ids: list[str]
    processed: int

class RunReportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    dry_run: bool | None = None

class RunReportResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    report_id: str | None = None
    result: QueryResult
    run_id: str

class ScheduleDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str
    created_at: str
    cron: str
    enabled: bool
    export_format: str
    id: str
    last_run_at: str | None = None
    next_run_at: str | None = None
    recipients: list[str]
    report_id: str
    timezone: str
    updated_at: str

class SearchHit(BaseModel):
    model_config = ConfigDict(extra="allow")
    body: str
    doc_id: str
    doc_type: str
    href: str | None = None
    title: str

class SessionDetail(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    interactions: list[ChatResponse]
    page_scope: str | None = None
    title: str
    updated_at: str

class SessionListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[SessionView]

class SessionSummary(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    page_scope: str | None = None
    title: str
    updated_at: str

class SessionView(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    current: bool
    device_label: str | None = None
    id: str
    ip_address: str | None = None
    last_seen_at: str
    org_id: str
    user_agent: str | None = None

class SessionsListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[SessionSummary]

class SetCustomerDunningProfileRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    profile_id: str

class SimulateQueryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: ReportDefinition

class SimulateRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    graph: WorkflowGraph
    max_steps: str | None = None
    payload: str | None = None

class SimulateResult(BaseModel):
    model_config = ConfigDict(extra="allow")
    error: str | None = None
    ok: bool
    side_effects: bool
    steps: list[SimulateStepResult]

class SimulateStepResult(BaseModel):
    model_config = ConfigDict(extra="allow")
    action: str | None = None
    detail: str | None = None
    node_id: str
    node_type: str
    permission: str | None = None
    permission_allowed: str | None = None
    status: str
    step_index: int

class SseTokenResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    expires_in_secs: int
    token: str

class SsoConfigView(BaseModel):
    model_config = ConfigDict(extra="allow")
    config: str
    display_name: str
    enabled: bool
    id: str
    org_id: str
    protocol: str

class SsoListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[SsoConfigView]

class StageDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    is_lost: bool
    is_won: bool
    name: str
    pipeline_id: str
    position: int
    probability: int

class StageSummary(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str
    open_amount_minor: int
    open_deal_count: int
    stage_id: str
    stage_name: str

class StartWorkflowRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    dry_run: bool | None = None
    payload: str | None = None

class StatementLineDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str
    description: str | None = None
    id: str
    line_no: int
    reference: str | None = None
    statement_id: str
    status: str
    txn_date: str

class StockLevelDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    avg_unit_cost_minor: int
    item_id: str
    last_movement_at: str | None = None
    qty_on_hand: int
    updated_at: str
    warehouse_id: str

class StockLevelListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[StockLevelDto]

class StockMovementDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    avg_unit_cost_minor_after: int | None = None
    cogs_journal_public_id: str | None = None
    created_at: str
    currency: str
    id: str
    item_id: str
    low_stock: bool | None = None
    memo: str | None = None
    movement_type: str
    qty_delta: int
    qty_on_hand_after: int | None = None
    source_id: str | None = None
    source_type: str | None = None
    unit_cost_minor: int
    warehouse_id: str

class StockMovementListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[StockMovementDto]
    total: int

class StripePaymentObject(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount: int
    currency: str
    customer_id: str
    id: str
    invoice_id: str | None = None
    status: str

class StripeWebhookData(BaseModel):
    model_config = ConfigDict(extra="allow")
    object: StripePaymentObject

class StripeWebhookFixture(BaseModel):
    model_config = ConfigDict(extra="allow")
    created: int
    data: StripeWebhookData
    id: str
    type: str

class SubmitExpenseRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    category_code: str | None = None
    currency: str
    description: str
    incurred_at: str | None = None
    receipt_url: str | None = None

class SuggestionChip(BaseModel):
    model_config = ConfigDict(extra="allow")
    action_type: str
    id: str
    label: str
    proposal_id: str | None = None

class SuggestionsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    chips: list[SuggestionChip]

class SummaryResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    my_open_tasks: int
    open_tasks: int
    overdue: int
    pending_approvals_for_me: int | None = None
    projects_active: int

class SupplierDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    currency: str
    email: str | None = None
    id: str
    name: str
    payment_terms: str | None = None
    phone: str | None = None
    updated_at: str
    version: int

class SupplierListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[SupplierDto]
    total: int

class SwitchOrgRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    org_id: str

class TaskAttachmentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    byte_size: str | None = None
    content_type: str | None = None
    created_at: str
    file_name: str
    id: str
    url: str

class TaskBoardResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    columns: list[BoardColumnDto]
    project_id: str | None = None

class TaskCommentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    author_user_id: str
    body: str
    created_at: str
    id: str
    mentioned_user_ids: list[str]

class TaskDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_id: str | None = None
    attachments: list[TaskAttachmentDto]
    blocked_by: list[str]
    checklist: list[ChecklistItemDto]
    completed_at: str | None = None
    created_at: str
    description: str | None = None
    due_at: str | None = None
    id: str
    labels: list[str]
    owner_user_id: str
    position: float
    priority: str
    project_id: str
    status: str
    title: str
    updated_at: str
    version: int

class TaskListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TaskDto]
    total: int

class TaxGroupDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    description: str | None = None
    id: str
    name: str

class TaxGroupListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TaxGroupDto]
    total: int

class TaxRateDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    component_name: str | None = None
    created_at: str
    id: str
    is_component: bool
    name: str
    rate_bps: int
    supersedes_id: str | None = None
    tax_group_id: str | None = None
    valid_from: str
    valid_to: str | None = None

class TaxRateListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TaxRateDto]

class TaxResolveQuery(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of: str | None = None
    group_id: str | None = None
    rate_id: str | None = None

class TaxResolveResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    as_of: str
    rate_bps: int
    tax_group_id: str | None = None
    tax_rate_id: str | None = None

class TeamListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TeamView]

class TeamView(BaseModel):
    model_config = ConfigDict(extra="allow")
    department_id: str | None = None
    lead_user_id: str | None = None
    name: str
    parent_team_id: str | None = None
    team_id: str

class TerritoryAssignmentDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    assigned_at: str
    customer_id: str | None = None
    deal_id: str | None = None
    territory_id: str

class TerritoryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    description: str | None = None
    id: str
    name: str
    owner_user_id: str | None = None
    updated_at: str
    version: int

class TerritoryListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TerritoryDto]
    total: int

class TimeEntryDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    billable: bool
    created_at: str
    entry_date: str
    id: str
    membership_user_id: str
    minutes: int
    notes: str | None = None
    project_id: str
    status: str
    task_id: str | None = None
    timesheet_id: str | None = None
    updated_at: str
    version: int

class TimelineEventDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    actor_user_id: str | None = None
    event_type: str
    id: str
    metadata: str
    occurred_at: str
    summary: str

class TimelineResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TimelineEventDto]

class TimesheetDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_id: str | None = None
    approved_at: str | None = None
    approved_by: str | None = None
    created_at: str
    entries: list[TimeEntryDto]
    id: str
    membership_user_id: str
    notes: str | None = None
    status: str
    submitted_at: str | None = None
    updated_at: str
    version: int
    week_start: str

class TimesheetListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TimesheetDto]
    total: int

class TokenResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    access_token: str
    expires_in: int
    session_id: str
    token_type: str

class TokenUsage(BaseModel):
    model_config = ConfigDict(extra="allow")
    cost_estimate_minor: int
    currency: str
    input_tokens: int
    latency_ms: int
    model: str
    output_tokens: int
    prompt_template_version: str

class ToolTraceEntry(BaseModel):
    model_config = ConfigDict(extra="allow")
    args_summary: str
    decision: str
    duration_ms: int
    permission: str
    reason: str
    tool_name: str

class TrialBalanceResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    balanced: bool
    currency: str
    period_id: str | None = None
    rows: list[TrialBalanceRow]
    total_credit_minor: int
    total_debit_minor: int

class TrialBalanceRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    account_code: str
    account_name: str
    account_type: str
    credit_minor: int
    debit_minor: int

class TriggerCatalogueEntry(BaseModel):
    model_config = ConfigDict(extra="allow")
    aggregate: str
    context: str
    description: str
    event_key: str
    event_type: str
    subject_suffix: str

class TriggerCatalogueResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[TriggerCatalogueEntry]

class UpdateAiSettingsRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    auto_execute_allow_list: str | None = None
    data_sharing: str | None = None
    model_preference: str | None = None
    modules_enabled: str | None = None
    monthly_token_budget: str | None = None

class UpdateChecklistRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    checklist: dict[str, Any]

class UpdateContactRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str | None = None
    first_name: str | None = None
    is_primary: str | None = None
    last_name: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    title: str | None = None

class UpdateContractRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    auto_renew: str | None = None
    currency: str | None = None
    end_date: str | None = None
    owner_user_id: str | None = None
    renewal_notice_days: str | None = None
    start_date: str | None = None
    status: str | None = None
    term_months: str | None = None
    title: str | None = None
    value_minor: str | None = None

class UpdateCustomerRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    billing_address: str | None = None
    email: str | None = None
    name: str | None = None
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    website: str | None = None

class UpdateDashboardRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    layout: str | None = None
    name: str | None = None

class UpdateDealRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: str | None = None
    currency: str | None = None
    expected_close_date: str | None = None
    name: str | None = None
    note: str | None = None
    owner_user_id: str | None = None
    probability: str | None = None
    stage_id: str | None = None

class UpdateDunningProfileRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    is_default: str | None = None
    name: str | None = None
    steps: str | None = None

class UpdateEmployeeRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    bank_details: str | None = None
    department_id: str | None = None
    display_name: str | None = None
    end_date: str | None = None
    government_id: str | None = None
    legal_first_name: str | None = None
    legal_last_name: str | None = None
    location: str | None = None
    manager_employee_id: str | None = None
    personal_email: str | None = None
    phone: str | None = None
    start_date: str | None = None
    status: str | None = None
    tax_id: str | None = None
    title: str | None = None
    user_id: str | None = None
    work_email: str | None = None

class UpdateFinanceEntityRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str | None = None
    currency: str | None = None
    is_default: str | None = None
    name: str | None = None

class UpdateInventoryAssetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    asset_tag: str | None = None
    name: str | None = None
    salvage_minor: str | None = None
    useful_life_months: str | None = None

class UpdateInventoryItemRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    allow_negative_stock: str | None = None
    description: str | None = None
    is_active: str | None = None
    name: str | None = None
    reorder_point_qty: str | None = None

class UpdateInvoiceRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    due_date: str | None = None
    lines: str | None = None
    notes: str | None = None
    terms: str | None = None

class UpdateLeadRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    company_name: str | None = None
    email: str | None = None
    name: str | None = None
    notes: str | None = None
    owner_user_id: str | None = None
    phone: str | None = None
    score: str | None = None
    source: str | None = None
    status: str | None = None

class UpdateLedgerAccountRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    is_active: str | None = None
    name: str | None = None
    parent_id: str | None = None
    sort_order: str | None = None

class UpdateOrderStatusRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    status: str

class UpdateOrgBoundsRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    max_concurrent: int
    max_steps_per_instance: int

class UpdateOrgSettingsRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    branding: str | None = None
    business_type: str | None = None
    currency: str | None = None
    fiscal_year_start_month: str | None = None
    name: str | None = None
    numbering_series: str | None = None
    timezone: str | None = None

class UpdatePolicyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: str | None = None
    is_active: str | None = None
    name: str | None = None

class UpdateProductRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    active: str | None = None
    currency: str | None = None
    name: str | None = None
    sku: str | None = None
    tax_group: str | None = None
    unit_price_minor: str | None = None

class UpdateProjectRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    customer_id: str | None = None
    deal_id: str | None = None
    description: str | None = None
    due_at: str | None = None
    name: str | None = None
    owner_user_id: str | None = None
    starts_at: str | None = None
    status: str | None = None

class UpdateQuoteRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    currency: str | None = None
    lines: str | None = None
    notes: str | None = None
    owner_user_id: str | None = None
    valid_until: str | None = None

class UpdateReportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    definition: str | None = None
    description: str | None = None
    name: str | None = None

class UpdateRetentionRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    default_retention_days: str | None = None
    overrides: str | None = None

class UpdateScheduleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    channel: str | None = None
    cron: str | None = None
    enabled: str | None = None
    export_format: str | None = None
    recipients: str | None = None
    timezone: str | None = None

class UpdateSelfProfileRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    display_name: str | None = None
    location: str | None = None
    personal_email: str | None = None
    phone: str | None = None

class UpdateSupplierRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    email: str | None = None
    name: str | None = None
    payment_terms: str | None = None
    phone: str | None = None

class UpdateTaskRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    assignee_id: str | None = None
    blocked_by: str | None = None
    description: str | None = None
    due_at: str | None = None
    labels: str | None = None
    position: str | None = None
    priority: str | None = None
    status: str | None = None
    title: str | None = None

class UpdateTerritoryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    name: str | None = None
    owner_user_id: str | None = None

class UpdateWarehouseRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    is_active: str | None = None
    location: str | None = None
    name: str | None = None

class UpdateWorkflowDefinitionRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    description: str | None = None
    graph: str | None = None
    name: str | None = None

class UpsertExpensePolicyRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    auto_approve_under_minor: str | None = None
    category_limits: str | None = None
    mileage_rate_minor: str | None = None
    mileage_unit: str | None = None
    name: str | None = None
    over_limit_action: str | None = None
    per_diem_minor: str | None = None
    require_receipt_over_minor: str | None = None

class UpsertRoleRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    approval_limit_amount_minor: str | None = None
    approval_limit_currency: str | None = None
    description: str | None = None
    name: str
    permissions: list[RolePermissionInput]

class UpsertSsoRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    config: str
    display_name: str
    enabled: bool | None = None
    protocol: str

class UpsertTimeEntryRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    billable: str | None = None
    entry_date: str
    id: str | None = None
    minutes: int
    notes: str | None = None
    project_id: str
    task_id: str | None = None

class UpsertWidgetRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    config: str | None = None
    id: str | None = None
    metric_name: str
    position: int | None = None
    title: str
    visualization: str | None = None

class VendorBillDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    amount_paid_minor: int
    created_at: str
    currency: str
    id: str
    memo: str | None = None
    payment_journal_public_id: str | None = None
    source_id: str | None = None
    source_type: str
    status: str
    supplier_ref: str
    updated_at: str
    version: int

class VendorBillListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[VendorBillDto]
    total: int

class VendorBillProxyDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    amount_paid_minor: int
    currency: str
    id: str
    payment_journal_public_id: str | None = None
    source_id: str | None = None
    source_type: str
    status: str
    supplier_ref: str

class VerifyEmailRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    token: str

class WarehouseDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    code: str
    created_at: str
    id: str
    is_active: bool
    location: str | None = None
    name: str
    updated_at: str
    version: int

class WarehouseListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WarehouseDto]
    total: int

class WebhookAck(BaseModel):
    model_config = ConfigDict(extra="allow")
    duplicate: bool
    payment_id: str | None = None
    received: bool

class WebhookDeliveryListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WebhookDeliveryView]

class WebhookDeliveryView(BaseModel):
    model_config = ConfigDict(extra="allow")
    attempt: int
    created_at: str
    delivered_at: str | None = None
    endpoint_id: str
    event_subject: str
    event_type: str
    id: str
    next_retry_at: str | None = None
    response_body: str | None = None
    status: str
    status_code: str | None = None

class WebhookEndpointListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WebhookEndpointView]

class WebhookEndpointView(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    description: str
    disabled_at: str | None = None
    disabled_reason: str | None = None
    event_types: list[str]
    failure_count: int
    id: str
    last_delivery_at: str | None = None
    secret_prefix: str
    status: str
    url: str

class WeightedForecast(BaseModel):
    model_config = ConfigDict(extra="allow")
    amount_minor: int
    currency: str

class WhoCouldSeeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[EntitlementRow]

class WhoDidSeeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[AuditReadRow]

class WidgetDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    config: str
    created_at: str
    dashboard_id: str
    id: str
    metric_name: str
    position: int
    title: str
    visualization: str

class WinDealRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    reason: str | None = None

class WinRateSummary(BaseModel):
    model_config = ConfigDict(extra="allow")
    lost_count: int
    win_rate_pct: float
    won_count: int

class WorkScheduleDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    id: str
    is_default: bool
    location: str | None = None
    name: str
    timezone: str
    updated_at: str
    version: int
    weekly_hours: str

class WorkScheduleListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WorkScheduleDto]

class WorkflowDefinitionDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    created_by: str
    current_published_version: str | None = None
    description: str
    graph: str | None = None
    id: str
    latest_version_id: str | None = None
    name: str
    status: str
    updated_at: str

class WorkflowDefinitionListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WorkflowDefinitionDto]

class WorkflowGraph(BaseModel):
    model_config = ConfigDict(extra="allow")
    entry: str
    nodes: list[WorkflowNode]
    sla_seconds: str | None = None
    trigger: WorkflowTrigger

class WorkflowInstanceDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    actor_user_id: str
    completed_at: str | None = None
    current_node_id: str | None = None
    definition_id: str
    error_message: str | None = None
    id: str
    sla_deadline: str | None = None
    started_at: str
    status: str
    step_count: int
    temporal_workflow_id: str
    updated_at: str
    version_id: str
    version_number: int
    waiting_until: str | None = None

class WorkflowInstanceListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WorkflowInstanceDto]

class WorkflowNode(BaseModel):
    model_config = ConfigDict(extra="allow")

class WorkflowTrigger(BaseModel):
    model_config = ConfigDict(extra="allow")

class WorkflowVersionDto(BaseModel):
    model_config = ConfigDict(extra="allow")
    created_at: str
    definition_id: str
    graph: WorkflowGraph
    id: str
    published_at: str | None = None
    published_by: str | None = None
    required_permissions: list[str]
    version: int

class WorkflowVersionListResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    items: list[WorkflowVersionDto]

