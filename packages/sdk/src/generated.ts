/** AUTO-GENERATED from openapi.json — do not edit by hand. Run pnpm generate:sdk */

export type Hello = {
  created_by: string;
  /** Prefixed public id (`hel_…`). */
  id: string;
  message: string;
  /** Prefixed org id (`org_…`). */
  org_id: string;
};

export type CreateHelloRequest = {
  message: string;
};

export type HelloListResponse = {
  items: Hello[];
};

export type RegisterRequest = {
  display_name: string;
  email: string;
  org_name: string;
  password: string;
};

export type RegisterResponse = {
  email: string;
  org_id: string;
  user_id: string;
  verification_required: boolean;
};

export type LoginRequest = {
  device_label?: string;
  email: string;
  org_id?: string;
  password: string;
};

export type TokenResponse = {
  access_token: string;
  expires_in: number;
  session_id: string;
  token_type: string;
};

export type MfaChallengeResponse = {
  challenge_token: string;
  message: string;
  mfa_required: boolean;
};

export type SwitchOrgRequest = {
  org_id: string;
};

export type MeResponse = {
  org_id: string;
  policy_version: number;
  roles: string[];
  session_id: string;
  user_id: string;
};

export type MembershipView = {
  org_id: string;
  org_name: string;
  policy_version: number;
  role: string;
};

export type MembershipListResponse = {
  items: MembershipView[];
};

export type SessionView = {
  created_at: string;
  current: boolean;
  device_label?: string;
  id: string;
  ip_address?: string;
  last_seen_at: string;
  org_id: string;
  user_agent?: string;
};

export type SessionListResponse = {
  items: SessionView[];
};

export type MessageResponse = {
  message: string;
};

export type CreateOrgRequest = {
  business_type?: string;
  currency?: string;
  name: string;
  timezone?: string;
};

export type OrgResponse = {
  branding: Record<string, unknown>;
  business_type: string;
  currency: string;
  feature_flags: Record<string, unknown>;
  fiscal_year_start_month: number;
  name: string;
  numbering_series: Record<string, unknown>;
  org_id: string;
  plan: string;
  timezone: string;
};

export type UpdateOrgSettingsRequest = {
  branding?: Record<string, unknown>;
  business_type?: string;
  currency?: string;
  fiscal_year_start_month?: string;
  name?: string;
  numbering_series?: Record<string, unknown>;
  timezone?: string;
};

export type MemberView = {
  department_id?: string;
  display_name: string;
  email: string;
  membership_id: string;
  policy_version: number;
  role: string;
  role_id?: string;
  role_name?: string;
  status: string;
  team_id?: string;
  user_id: string;
};

export type MemberListResponse = {
  items: MemberView[];
};

export type InviteMemberRequest = {
  email: string;
  /** Public role id (`rol_…`) or system key (`owner`, `admin`, …). */
  role: string;
};

export type InviteResponse = {
  email: string;
  expires_at: string;
  invitation_id: string;
  status: string;
};

export type RoleView = {
  approval_limit_amount_minor?: string;
  approval_limit_currency?: string;
  description: string;
  is_system: boolean;
  name: string;
  permissions: RolePermissionView[];
  role_id: string;
  system_key?: string;
};

export type RolePermissionView = {
  effect: string;
  permission_id: string;
  scope: string;
};

export type RoleListResponse = {
  items: RoleView[];
};

export type UpsertRoleRequest = {
  approval_limit_amount_minor?: string;
  approval_limit_currency?: string;
  description?: string;
  name: string;
  permissions: RolePermissionInput[];
};

export type RolePermissionInput = {
  effect: string;
  permission_id: string;
  scope?: string;
};

export type CapabilityPreviewResponse = {
  allowed: string[];
  denied_sensitive: string[];
  role_id: string;
};

export type PermissionCatalogueItem = {
  action: string;
  context: string;
  description: string;
  id: string;
  resource: string;
  sensitive: boolean;
};

export type PermissionCatalogueResponse = {
  items: PermissionCatalogueItem[];
};

export type TeamView = {
  department_id?: string;
  lead_user_id?: string;
  name: string;
  parent_team_id?: string;
  team_id: string;
};

export type TeamListResponse = {
  items: TeamView[];
};

export type DepartmentView = {
  department_id: string;
  name: string;
  parent_id?: string;
};

export type DepartmentListResponse = {
  items: DepartmentView[];
};

export type MyCapabilitiesResponse = {
  allowed: string[];
  org_id: string;
  policy_version: number;
  role: string;
};

export type DashboardResponse = {
  /** RFC3339 timestamp when this snapshot was produced. */
  as_of: string;
  /** Requested period window (e.g. `30d`); accepted but does not invent metrics. */
  period: string;
  /** Derived from the caller's primary role. */
  role_layout: string;
  widgets: DashboardWidget[];
};

export type DashboardWidget = {
  id: string;
  /** checklist | stat | module_empty | feed | pipeline */
  kind: string;
  /** Widget-specific JSON body (checklist items, empty lists, module stubs). */
  payload: Record<string, unknown>;
  range_label?: string;
  /** module_not_enabled | coming_in_later_phase | no_data | crm_unreachable */
  reason_code?: string;
  /** Always false for honest empties in Phase 1.4 (pattern present for later). */
  stale: boolean;
  /** ready | empty | unavailable | loading */
  status: string;
  title: string;
};

export type CustomerDto = {
  billing_address?: string;
  created_at: string;
  email?: string;
  id: string;
  name: string;
  notes?: string;
  owner_user_id?: string;
  phone?: string;
  updated_at: string;
  version: number;
  website?: string;
};

export type DealDto = {
  amount_minor: number;
  created_at: string;
  currency: string;
  customer_id?: string;
  expected_close_date?: string;
  id: string;
  lead_id?: string;
  lost_at?: string;
  lost_reason?: string;
  name: string;
  owner_user_id?: string;
  pipeline_id: string;
  probability?: string;
  stage_id: string;
  status: string;
  updated_at: string;
  version: number;
  won_at?: string;
  won_reason?: string;
};

export type QuoteDto = {
  accepted_at?: string;
  created_at: string;
  currency: string;
  customer_id: string;
  deal_id?: string;
  discount_minor: number;
  id: string;
  lines: QuoteLineDto[];
  notes?: string;
  owner_user_id?: string;
  previous_quote_id?: string;
  quote_number: string;
  status: string;
  subtotal_minor: number;
  tax_minor: number;
  total_minor: number;
  updated_at: string;
  valid_until?: string;
  version: number;
  version_number: number;
};

export type QuoteLineDto = {
  description: string;
  discount_minor: number;
  id: string;
  line_total_minor: number;
  position: number;
  product_id?: string;
  quantity: number;
  tax_minor: number;
  tax_rate_bps: number;
  unit_price_minor: number;
};

export type LeadDto = {
  company_name?: string;
  converted_customer_id?: string;
  converted_deal_id?: string;
  created_at: string;
  email?: string;
  id: string;
  name: string;
  notes?: string;
  owner_user_id?: string;
  phone?: string;
  score: number;
  source?: string;
  status: string;
  updated_at: string;
  version: number;
};

export type BoardResponse = {
  pipeline: PipelineDto;
  stages: BoardStage[];
};

export type BoardStage = {
  deals: DealDto[];
  stage: StageDto;
};

export type StageDto = {
  id: string;
  is_lost: boolean;
  is_won: boolean;
  name: string;
  pipeline_id: string;
  position: number;
  probability: number;
};

export type ReportSummaryResponse = {
  activity_volume: ActivityVolumeItem[];
  pipeline_by_stage: StageSummary[];
  weighted_forecast: WeightedForecast;
  win_rate: WinRateSummary;
};

export type PipelineDto = {
  id: string;
  is_default: boolean;
  name: string;
};

export type CreateDealRequest = {
  amount_minor?: number;
  currency?: string;
  customer_id?: string;
  expected_close_date?: string;
  lead_id?: string;
  name: string;
  owner_user_id?: string;
  pipeline_id?: string;
  probability?: string;
  stage_id?: string;
};

export type CreateCustomerRequest = {
  billing_address?: string;
  email?: string;
  name: string;
  notes?: string;
  owner_user_id?: string;
  phone?: string;
  website?: string;
};

export type CreateQuoteRequest = {
  currency?: string;
  customer_id: string;
  deal_id?: string;
  lines?: CreateQuoteLineRequest[];
  notes?: string;
  owner_user_id?: string;
  quote_number?: string;
  valid_until?: string;
};

export type CreateQuoteLineRequest = {
  description?: string;
  discount_minor?: number;
  product_id?: string;
  quantity: number;
  tax_rate_bps?: number;
  unit_price_minor: number;
};

export type InvoiceActionResponse = {
  available: boolean;
  reason: string;
};

export type StageSummary = {
  currency: string;
  open_amount_minor: number;
  open_deal_count: number;
  stage_id: string;
  stage_name: string;
};

export type WinRateSummary = {
  lost_count: number;
  win_rate_pct: number;
  won_count: number;
};

export type WeightedForecast = {
  amount_minor: number;
  currency: string;
};

export type ActivityVolumeItem = {
  count: number;
  kind: string;
};

export type InvoiceDto = {
  amount_credited_minor: number;
  amount_paid_minor: number;
  balance_minor: number;
  base_currency: string;
  base_total_minor: number;
  created_at: string;
  currency: string;
  customer_id: string;
  discount_minor: number;
  due_date?: string;
  fx_rate_date?: string;
  fx_rate_den?: string;
  fx_rate_num?: string;
  id: string;
  invoice_number?: string;
  issue_date?: string;
  lines: InvoiceLineDto[];
  notes?: string;
  payment_url?: string;
  source_quote_id?: string;
  status: string;
  subtotal_minor: number;
  tax_minor: number;
  terms?: string;
  total_minor: number;
  updated_at: string;
  version: number;
};

export type InvoiceLineDto = {
  description: string;
  discount_minor: number;
  id: string;
  line_total_minor: number;
  quantity: number;
  tax_minor: number;
  tax_rate_bps: number;
  unit_price_minor: number;
};

export type InvoiceLineInput = {
  description: string;
  discount_minor?: number;
  quantity: number;
  tax_rate_bps?: number;
  unit_price_minor: number;
};

export type InvoiceListResponse = {
  items: InvoiceDto[];
  total: number;
};

export type CreateInvoiceRequest = {
  base_currency?: string;
  currency: string;
  /** Finance customer public id (`cus_…` projection) or sales customer id. */
  customer_id: string;
  due_date?: string;
  lines: InvoiceLineInput[];
  notes?: string;
  terms?: string;
};

export type CreateInvoiceFromQuoteRequest = {
  currency: string;
  customer_id: string;
  customer_name: string;
  lines: QuoteLineSnapshot[];
  notes?: string;
  quote_id: string;
  terms?: string;
  total_minor?: string;
};

export type QuoteLineSnapshot = {
  description: string;
  discount_minor?: number;
  quantity: number;
  tax_rate_bps?: number;
  unit_price_minor: number;
};

export type IssueInvoiceRequest = {
  due_date?: string;
  fx_rate_date?: string;
  fx_rate_den?: number;
  /** FX rate as rational num/den captured at issue (document currency → base). */
  fx_rate_num?: number;
  issue_date?: string;
};

export type PaymentDto = {
  amount_allocated_minor: number;
  amount_minor: number;
  amount_unapplied_minor: number;
  currency: string;
  customer_id: string;
  id: string;
  method: string;
  notes?: string;
  provider?: string;
  received_at: string;
};

export type RecordPaymentRequest = {
  amount_minor: number;
  currency: string;
  customer_id: string;
  invoice_id?: string;
  notes?: string;
  received_at?: string;
};

export type CreditNoteDto = {
  credit_number: string;
  currency: string;
  customer_id: string;
  id: string;
  invoice_id: string;
  issued_at: string;
  reason?: string;
  subtotal_minor: number;
  tax_minor: number;
  total_minor: number;
};

export type CreateCreditNoteRequest = {
  invoice_id: string;
  lines: InvoiceLineInput[];
  reason?: string;
};

export type ExpenseDto = {
  amount_minor: number;
  category_code?: string;
  created_at: string;
  currency: string;
  description: string;
  id: string;
  incurred_at: string;
  receipt_url?: string;
  status: string;
};

export type SubmitExpenseRequest = {
  amount_minor: number;
  category_code?: string;
  currency: string;
  description: string;
  incurred_at?: string;
  receipt_url?: string;
};

export type ReportSummaryDto = {
  ageing: AgeingBucket[];
  as_of: string;
  cash_flow: CashFlowPoint[];
  cash_minor: number;
  currency: string;
  expenses_by_category: CategoryAmount[];
  expenses_minor: number;
  receivables_minor: number;
  revenue_minor: number;
};

export type AgeingBucket = {
  amount_minor: number;
  label: string;
};

export type CategoryAmount = {
  amount_minor: number;
  category: string;
};

export type CashFlowPoint = {
  inflow_minor: number;
  outflow_minor: number;
  period: string;
};

export type FinanceCustomerDto = {
  currency: string;
  email?: string;
  id: string;
  name: string;
  outstanding_balance_minor: number;
  sales_customer_id: string;
};

export type WebhookAck = {
  duplicate: boolean;
  payment_id?: string;
  received: boolean;
};
