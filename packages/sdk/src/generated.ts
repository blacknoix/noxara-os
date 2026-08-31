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
  approval_id?: string;
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
  approval_id?: string;
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

export type ProjectDto = {
  created_at: string;
  customer_id?: string;
  deal_id?: string;
  description?: string;
  due_at?: string;
  id: string;
  name: string;
  owner_user_id: string;
  starts_at?: string;
  status: string;
  updated_at: string;
  version: number;
};

export type TaskDto = {
  assignee_id?: string;
  attachments: TaskAttachmentDto[];
  blocked_by: string[];
  checklist: ChecklistItemDto[];
  completed_at?: string;
  created_at: string;
  description?: string;
  due_at?: string;
  id: string;
  labels: string[];
  owner_user_id: string;
  position: number;
  priority: string;
  project_id: string;
  status: string;
  title: string;
  updated_at: string;
  version: number;
};

export type TaskBoardResponse = {
  columns: BoardColumnDto[];
  project_id?: string;
};

export type BoardColumnDto = {
  status: string;
  tasks: TaskDto[];
};

export type ChecklistItemDto = {
  id: string;
  is_done: boolean;
  position: number;
  title: string;
};

export type TaskAttachmentDto = {
  byte_size?: string;
  content_type?: string;
  created_at: string;
  file_name: string;
  id: string;
  url: string;
};

export type TaskCommentDto = {
  author_user_id: string;
  body: string;
  created_at: string;
  id: string;
  mentioned_user_ids: string[];
};

export type MyWorkResponse = {
  assigned: TaskDto[];
  mentions: TaskCommentDto[];
  total_assigned: number;
};

export type SummaryResponse = {
  my_open_tasks: number;
  open_tasks: number;
  overdue: number;
  /** Pending approvals assigned to the current user (Phase 1.7). */
  pending_approvals_for_me?: number;
  projects_active: number;
};

export type NotificationItemDto = {
  body: string;
  created_at: string;
  href?: string;
  id: string;
  read_at?: string;
  resource_id?: string;
  resource_type?: string;
  title: string;
};

export type FeedResponse = {
  items: NotificationItemDto[];
};

export type SearchHit = {
  body: string;
  doc_id: string;
  doc_type: string;
  href?: string;
  title: string;
};

export type PresignUploadRequest = {
  content_type: string;
  filename: string;
  size_bytes: number;
};

export type PresignUploadResponse = {
  file_id: string;
  headers: Record<string, unknown>;
  upload_url: string;
};

export type FileMetaResponse = {
  content_type: string;
  /** Download URL. Clients SHOULD set `Content-Disposition: attachment`
when proxying to force download rather than inline render. */
  download_url: string;
  file_id: string;
  size_bytes: number;
  status: string;
};

export type ChatRequest = {
  message: string;
  page_scope?: string;
  session_id?: string;
  stream?: string;
};

export type ChatResponse = {
  citations: Citation[];
  content: string;
  follow_ups: string[];
  interaction_id: string;
  proposals: ProposalView[];
  role: string;
  session_id: string;
  tool_trace: ToolTraceEntry[];
  usage: TokenUsage;
};

export type ProposalView = {
  action_type: string;
  citations: Citation[];
  command: Record<string, unknown>;
  created_at: string;
  id: string;
  rendered_diff: string;
  status: string;
  tool_name: string;
};

export type ConfirmProposalRequest = {
  note?: string;
};

export type ProposalsListResponse = {
  items: ProposalView[];
};

export type AskRequest = {
  page_scope?: string;
  query: string;
};

export type AskResponse = {
  citations?: string;
  form?: string;
  kind: string;
  message?: string;
  tool_trace?: string;
};

export type AskForm = {
  action_type: string;
  fields: AskFormField[];
  proposal_preview?: string;
};

export type AskFormField = {
  label: string;
  name: string;
  type: string;
  value: string;
};

export type AiSettings = {
  auto_execute_allow_list: string[];
  budget_month: string;
  data_sharing: DataSharingSettings;
  model_preference: string;
  modules_enabled: ModulesEnabled;
  monthly_token_budget: number;
  tokens_used_this_month: number;
};

export type ModulesEnabled = {
  ask_mode: boolean;
  copilot: boolean;
  document_ai: boolean;
  insights: boolean;
};

export type DataSharingSettings = {
  allow_training: boolean;
  share_with_provider: boolean;
};

export type UpdateAiSettingsRequest = {
  auto_execute_allow_list?: string;
  data_sharing?: string;
  model_preference?: string;
  modules_enabled?: string;
  monthly_token_budget?: string;
};

export type InsightsResponse = {
  empty_reason?: string;
  observations: InsightObservation[];
};

export type InsightObservation = {
  body: string;
  estimate: boolean;
  evidence: Citation[];
  id: string;
  suggested_action?: string;
  title: string;
};

export type DocumentExtractRequest = {
  file_id?: string;
  kind: string;
  text?: string;
};

export type DocumentReview = {
  confidence: number;
  extracted: Record<string, unknown>;
  id: string;
  kind: string;
  proposal_id?: string;
  status: string;
};

export type TokenUsage = {
  cost_estimate_minor: number;
  currency: string;
  input_tokens: number;
  latency_ms: number;
  model: string;
  output_tokens: number;
  prompt_template_version: string;
};

export type Citation = {
  href?: string;
  record_id: string;
  record_type: string;
  snippet?: string;
  title: string;
};

export type ToolTraceEntry = {
  args_summary: string;
  decision: string;
  duration_ms: number;
  permission: string;
  reason: string;
  tool_name: string;
};

export type SessionSummary = {
  id: string;
  page_scope?: string;
  title: string;
  updated_at: string;
};

export type SessionDetail = {
  id: string;
  interactions: ChatResponse[];
  page_scope?: string;
  title: string;
  updated_at: string;
};

export type SessionsListResponse = {
  items: SessionSummary[];
};

export type SuggestionChip = {
  action_type: string;
  id: string;
  label: string;
  proposal_id?: string;
};

export type SuggestionsResponse = {
  chips: SuggestionChip[];
};

export type EmployeeDto = {
  bank_details?: string;
  created_at: string;
  department_id?: string;
  display_name: string;
  end_date?: string;
  /** Present only when caller holds `hr.employee.read_sensitive`. */
  government_id?: string;
  id: string;
  legal_first_name?: string;
  legal_last_name?: string;
  location?: string;
  manager_employee_id?: string;
  owner_user_id: string;
  personal_email?: string;
  phone?: string;
  start_date?: string;
  status: string;
  tax_id?: string;
  title?: string;
  updated_at: string;
  user_id?: string;
  version: number;
  work_email?: string;
};

export type EmployeeListResponse = {
  items: EmployeeDto[];
  total: number;
};

export type CreateEmployeeRequest = {
  bank_details?: string;
  /** Opaque Workspace department public id (`dep_…`). */
  department_id?: string;
  display_name: string;
  /** Restricted — encrypted at rest; requires write + stored for sensitive read. */
  government_id?: string;
  legal_first_name?: string;
  legal_last_name?: string;
  location?: string;
  /** Manager employee public id (`emp_…`). */
  manager_employee_id?: string;
  personal_email?: string;
  phone?: string;
  start_date?: string;
  status?: string;
  tax_id?: string;
  title?: string;
  user_id?: string;
  work_email?: string;
};

export type UpdateEmployeeRequest = {
  bank_details?: string;
  department_id?: string;
  display_name?: string;
  end_date?: string;
  government_id?: string;
  legal_first_name?: string;
  legal_last_name?: string;
  location?: string;
  manager_employee_id?: string;
  personal_email?: string;
  phone?: string;
  start_date?: string;
  status?: string;
  tax_id?: string;
  title?: string;
  user_id?: string;
  work_email?: string;
};

export type UpdateSelfProfileRequest = {
  display_name?: string;
  location?: string;
  personal_email?: string;
  phone?: string;
};

export type CompensationComponentDto = {
  amount_minor: number;
  component_type: string;
  contract_id?: string;
  created_at: string;
  currency: string;
  effective_from: string;
  effective_to?: string;
  employee_id: string;
  id: string;
  label: string;
  version: number;
};

export type CompensationListResponse = {
  items: CompensationComponentDto[];
};

export type CreateCompensationRequest = {
  amount_minor: number;
  component_type?: string;
  contract_id?: string;
  currency: string;
  effective_from: string;
  effective_to?: string;
  label: string;
};

export type ContractDto = {
  contract_type: string;
  created_at: string;
  effective_from: string;
  effective_to?: string;
  employee_id: string;
  id: string;
  notes?: string;
  title?: string;
  version: number;
};

export type DocumentDto = {
  collected: boolean;
  created_at: string;
  doc_type: string;
  employee_id: string;
  expires_at?: string;
  file_id?: string;
  id: string;
  title: string;
  version: number;
};

export type AssetDto = {
  asset_tag?: string;
  assigned_at: string;
  employee_id: string;
  id: string;
  label: string;
  returned_at?: string;
  status: string;
};

export type TimelineEventDto = {
  actor_user_id?: string;
  event_type: string;
  id: string;
  metadata: Record<string, unknown>;
  occurred_at: string;
  summary: string;
};

export type OnboardRequest = {
  asset_labels?: string;
  department_id?: string;
  display_name: string;
  document_titles?: string;
  /** Test hook: inject activity failure after this step for compensation tests. */
  fail_after?: string;
  manager_employee_id?: string;
  /** Role key to assign via membership update activity (opaque; applied by access step). */
  role?: string;
  start_date?: string;
  task_titles?: string;
  title?: string;
  /** Link existing user (`usr_…`); otherwise user_id stays null until linked. */
  user_id?: string;
  work_email?: string;
};

export type OnboardResponse = {
  employee: EmployeeDto;
  status: string;
  tasks: HrTaskDto[];
  workflow_id: string;
};

export type OffboardRequest = {
  end_date?: string;
  fail_after?: string;
  reason?: string;
  reassign_manager_to?: string;
};

export type OffboardResponse = {
  checklist: AccessChecklistItem[];
  employee: EmployeeDto;
  status: string;
  workflow_id: string;
};

export type AccessChecklistItem = {
  cleared: boolean;
  detail: string;
  path: string;
};

export type AccessAuditResponse = {
  all_cleared: boolean;
  checklist: AccessChecklistItem[];
  employee_id: string;
  user_id?: string;
};

export type HrTaskDto = {
  assignee_user_id?: string;
  completed_at?: string;
  due_at?: string;
  employee_id: string;
  id: string;
  kind: string;
  status: string;
  title: string;
  workflow_id?: string;
};

export type WorkScheduleDto = {
  created_at: string;
  id: string;
  is_default: boolean;
  location?: string;
  name: string;
  timezone: string;
  updated_at: string;
  version: number;
  weekly_hours: Record<string, unknown>;
};

export type WorkScheduleListResponse = {
  items: WorkScheduleDto[];
};

export type CreateWorkScheduleRequest = {
  is_default?: string;
  location?: string;
  name: string;
  timezone?: string;
  weekly_hours?: Record<string, unknown>;
};

export type HolidayDto = {
  created_at: string;
  half_day_period?: string;
  holiday_date: string;
  id: string;
  is_half_day: boolean;
  location?: string;
  name: string;
  version: number;
};

export type HolidayListResponse = {
  items: HolidayDto[];
};

export type CreateHolidayRequest = {
  half_day_period?: string;
  holiday_date: string;
  is_half_day?: string;
  location?: string;
  name: string;
};

export type AttendanceDto = {
  accuracy_meters?: string;
  created_at: string;
  employee_id: string;
  entry_kind: string;
  id: string;
  latitude?: string;
  local_date: string;
  longitude?: string;
  note?: string;
  recorded_at: string;
  reverses_id?: string;
  source: string;
  timezone: string;
};

export type AttendanceListResponse = {
  items: AttendanceDto[];
  total: number;
};

export type RecordAttendanceRequest = {
  accuracy_meters?: string;
  employee_id?: string;
  entry_kind: string;
  latitude?: string;
  longitude?: string;
  note?: string;
  recorded_at?: string;
  /** Public id of the fact row to reverse (creates append-only reversal). */
  reverses_id?: string;
  source?: string;
  timezone?: string;
};

export type AttendanceImportRequest = {
  batch_key?: string;
  /** CSV body: employee_id,entry_kind,recorded_at[,timezone,latitude,longitude,accuracy_meters,note] */
  csv: string;
};

export type AttendanceImportResponse = {
  imported: number;
  items: AttendanceDto[];
  skipped: number;
};

export type LeaveTypeDto = {
  accrual_cadence: string;
  accrual_units_milli: number;
  allows_half_day: boolean;
  carry_forward_cap_milli?: string;
  category: string;
  code: string;
  created_at: string;
  expiry_days?: string;
  id: string;
  is_active: boolean;
  name: string;
  requires_approval: boolean;
  updated_at: string;
  version: number;
};

export type LeaveTypeListResponse = {
  items: LeaveTypeDto[];
};

export type CreateLeaveTypeRequest = {
  accrual_cadence?: string;
  accrual_units_milli?: string;
  allows_half_day?: string;
  carry_forward_cap_milli?: string;
  category?: string;
  code: string;
  expiry_days?: string;
  name: string;
  requires_approval?: string;
};

export type LeaveRequestDto = {
  approval_id?: string;
  created_at: string;
  decided_at?: string;
  decision_note?: string;
  employee_id: string;
  end_date: string;
  end_period: string;
  id: string;
  leave_type_id: string;
  reason?: string;
  start_date: string;
  start_period: string;
  status: string;
  timezone: string;
  units_days: string;
  units_milli: number;
  updated_at: string;
  version: number;
};

export type LeaveRequestListResponse = {
  items: LeaveRequestDto[];
  total: number;
};

export type CreateLeaveRequestRequest = {
  employee_id?: string;
  end_date: string;
  end_period?: string;
  leave_type_id: string;
  reason?: string;
  start_date: string;
  start_period?: string;
  /** When true, immediately submit into the approval engine. */
  submit?: string;
  timezone?: string;
};

export type DecideLeaveRequest = {
  approve: boolean;
  note?: string;
};

export type LeaveBalanceDto = {
  as_of: string;
  balance_days: string;
  balance_units_milli: number;
  employee_id: string;
  leave_type_code: string;
  leave_type_id: string;
  leave_type_name: string;
};

export type LeaveBalanceListResponse = {
  items: LeaveBalanceDto[];
};

export type LeaveCalendarEntryDto = {
  employee_display_name: string;
  employee_id: string;
  end_date: string;
  end_period: string;
  leave_request_id: string;
  leave_type_code: string;
  start_date: string;
  start_period: string;
  status: string;
  units_milli: number;
};

export type LeaveCalendarResponse = {
  items: LeaveCalendarEntryDto[];
};

export type AbsenceReportRowDto = {
  employee_display_name: string;
  employee_id: string;
  leave_type_code: string;
  request_count: number;
  units_days: string;
  units_milli: number;
};

export type AbsenceReportResponse = {
  from: string;
  items: AbsenceReportRowDto[];
  to: string;
};

export type CarryForwardRequest = {
  year: number;
};

export type CarryForwardResponse = {
  entries_posted: number;
  idempotent_replay: boolean;
  status: string;
  workflow_id: string;
  year: number;
};

export type AccrueLeaveRequest = {
  effective_date?: string;
  employee_id: string;
  leave_type_id: string;
  note?: string;
  units_milli?: string;
};

export type LeaveLedgerEntryDto = {
  created_at: string;
  effective_date: string;
  employee_id: string;
  entry_kind: string;
  expires_on?: string;
  id: string;
  leave_request_id?: string;
  leave_type_id: string;
  note?: string;
  units_milli: number;
};

export type PayrollComponentDto = {
  calc_method: string;
  code: string;
  config_json: Record<string, unknown>;
  created_at: string;
  currency?: string;
  id: string;
  is_active: boolean;
  label: string;
  line_kind: string;
  sort_order: number;
  updated_at: string;
  version: number;
};

export type PayrollComponentListResponse = {
  items: PayrollComponentDto[];
};

export type CreatePayrollComponentRequest = {
  calc_method: string;
  code: string;
  config_json: Record<string, unknown>;
  currency?: string;
  label: string;
  line_kind: string;
  sort_order?: string;
};

export type PayrollRunDto = {
  adjustment_of_run_id?: string;
  approval_id?: string;
  approved_at?: string;
  calculated_at?: string;
  created_at: string;
  currency: string;
  deductions_minor: number;
  employee_count: number;
  gross_minor: number;
  id: string;
  journal_public_id?: string;
  net_minor: number;
  paid_at?: string;
  period_end: string;
  period_start: string;
  status: string;
  updated_at: string;
  version: number;
};

export type PayrollRunListResponse = {
  items: PayrollRunDto[];
  total: number;
};

export type CreatePayrollRunRequest = {
  adjustment_of_run_id?: string;
  currency: string;
  period_end: string;
  period_start: string;
};

export type PayslipLineDto = {
  amount_minor: number;
  calculation_basis: Record<string, unknown>;
  component_code: string;
  currency: string;
  id: string;
  label: string;
  line_kind: string;
  sort_order: number;
};

export type PayslipDto = {
  created_at: string;
  currency: string;
  deductions_minor: number;
  employee_id: string;
  gross_minor: number;
  id: string;
  issued_at?: string;
  lines: PayslipLineDto[];
  net_minor: number;
  run_id: string;
  status: string;
  version: number;
};

export type PayslipListResponse = {
  items: PayslipDto[];
};

export type DecidePayrollRequest = {
  approve: boolean;
  note?: string;
};

export type JournalLineInput = {
  account_code: string;
  credit_minor: number;
  debit_minor: number;
  memo?: string;
};

export type PostJournalRequest = {
  currency: string;
  /** Document date `YYYY-MM-DD`; defaults to today. */
  entry_date?: string;
  lines: JournalLineInput[];
  memo?: string;
  /** Public id of the journal being reversed (`jrn_…`). */
  reverses_of?: string;
  /** Internal UUID of the source document (payroll run id).
Empty / omitted for manual → generated UUID. */
  source_id?: string;
  /** `payroll` or `manual`. */
  source_type: string;
};

export type JournalEntryDto = {
  currency: string;
  entry_date: string;
  id: string;
  lines: JournalLineInput[];
  memo: string;
  period_id?: string;
  source_id: string;
  source_type: string;
};

export type JournalListResponse = {
  items: JournalEntryDto[];
  total: number;
};

export type LedgerAccountDto = {
  account_type: string;
  code: string;
  description?: string;
  id: string;
  is_active: boolean;
  name: string;
  normal_balance: string;
  parent_id?: string;
  sort_order: number;
};

export type LedgerAccountNode = {
  account: LedgerAccountDto;
  children: LedgerAccountNode[];
};

export type LedgerAccountTreeResponse = {
  roots: LedgerAccountNode[];
};

export type CreateLedgerAccountRequest = {
  /** `asset` | `liability` | `equity` | `revenue` | `income` | `expense` */
  account_type: string;
  code: string;
  description?: string;
  name: string;
  normal_balance?: string;
  parent_id?: string;
  sort_order?: string;
};

export type UpdateLedgerAccountRequest = {
  description?: string;
  is_active?: string;
  name?: string;
  parent_id?: string;
  sort_order?: string;
};

export type FiscalPeriodDto = {
  checklist: Record<string, unknown>;
  closed_at?: string;
  code: string;
  end_date: string;
  id: string;
  name: string;
  reopen_reason?: string;
  reopened_at?: string;
  start_date: string;
  status: string;
};

export type FiscalPeriodListResponse = {
  items: FiscalPeriodDto[];
  total: number;
};

export type CreateFiscalPeriodRequest = {
  code: string;
  end_date: string;
  name: string;
  start_date: string;
};

export type ClosePeriodRequest = {
  /** Optional checklist override applied before close. */
  checklist?: string;
};

export type ReopenPeriodRequest = {
  reason: string;
};

export type TrialBalanceRow = {
  account_code: string;
  account_name: string;
  account_type: string;
  credit_minor: number;
  debit_minor: number;
};

export type TrialBalanceResponse = {
  balanced: boolean;
  currency: string;
  period_id?: string;
  rows: TrialBalanceRow[];
  total_credit_minor: number;
  total_debit_minor: number;
};

export type ProfitAndLossResponse = {
  currency: string;
  expense_total_minor: number;
  expenses: ReportLine[];
  from?: string;
  net_income_minor: number;
  period_id?: string;
  revenue: ReportLine[];
  revenue_total_minor: number;
  to?: string;
};

export type BalanceSheetResponse = {
  as_of: string;
  assets: ReportLine[];
  assets_total_minor: number;
  currency: string;
  equity: ReportLine[];
  equity_total_minor: number;
  liabilities: ReportLine[];
  liabilities_total_minor: number;
  period_id?: string;
};

export type ReportLine = {
  account_code: string;
  account_name: string;
  amount_minor: number;
};

export type BankAccountDto = {
  account_number_mask?: string;
  currency: string;
  id: string;
  institution?: string;
  is_active: boolean;
  ledger_account_id: string;
  name: string;
};

export type CreateBankAccountRequest = {
  account_number_mask?: string;
  currency: string;
  institution?: string;
  /** Ledger account public id (`acc_…`) or code (e.g. `1000`). */
  ledger_account_id: string;
  name: string;
};

export type BankStatementDto = {
  bank_account_id: string;
  closing_minor: number;
  currency: string;
  id: string;
  line_count: number;
  opening_minor: number;
  source: string;
  statement_date: string;
};

export type ImportStatementRequest = {
  closing_minor?: string;
  csv: string;
  opening_minor?: string;
  statement_date?: string;
};

export type ImportStatementResponse = {
  lines_imported: number;
  statement: BankStatementDto;
};

export type ReconcileResponse = {
  match_rate: number;
  matched: number;
  reconciliations: ReconciliationDto[];
  unmatched: number;
};

export type ReconciliationDto = {
  amount_minor: number;
  auto_matched: boolean;
  bank_account_id: string;
  id: string;
  match_kind: string;
  matched_payment_id?: string;
  statement_line_id: string;
};

export type ExpensePolicyDto = {
  auto_approve_under_minor: number;
  category_limits: CategoryLimitDto[];
  id: string;
  is_active: boolean;
  mileage_rate_minor: number;
  mileage_unit: string;
  name: string;
  over_limit_action: string;
  per_diem_minor: number;
  require_receipt_over_minor: number;
};

export type UpsertExpensePolicyRequest = {
  auto_approve_under_minor?: string;
  category_limits?: string;
  mileage_rate_minor?: string;
  mileage_unit?: string;
  name?: string;
  over_limit_action?: string;
  per_diem_minor?: string;
  require_receipt_over_minor?: string;
};

export type CategoryLimitDto = {
  category_code: string;
  currency: string;
  max_amount_minor: number;
};

export type ReimbursementBatchDto = {
  approval_id?: string;
  created_at: string;
  currency: string;
  expense_ids: string[];
  id: string;
  status: string;
  total_minor: number;
};

export type CreateReimbursementBatchRequest = {
  currency?: string;
  expense_ids: string[];
};

export type CardTransactionDto = {
  amount_minor: number;
  currency: string;
  description?: string;
  id: string;
  matched_expense_id?: string;
  merchant?: string;
  reference?: string;
  status: string;
  txn_date: string;
};

export type WarehouseDto = {
  code: string;
  created_at: string;
  id: string;
  is_active: boolean;
  location?: string;
  name: string;
  updated_at: string;
  version: number;
};

export type WarehouseListResponse = {
  items: WarehouseDto[];
  total: number;
};

export type CreateWarehouseRequest = {
  code: string;
  location?: string;
  name: string;
};

export type UpdateWarehouseRequest = {
  is_active?: string;
  location?: string;
  name?: string;
};

export type InventoryItemDto = {
  allow_negative_stock: boolean;
  created_at: string;
  currency: string;
  description?: string;
  id: string;
  is_active: boolean;
  name: string;
  reorder_point_qty: number;
  sku: string;
  uom: string;
  updated_at: string;
  version: number;
};

export type InventoryItemListResponse = {
  items: InventoryItemDto[];
  total: number;
};

export type CreateInventoryItemRequest = {
  allow_negative_stock?: string;
  currency: string;
  description?: string;
  name: string;
  reorder_point_qty?: string;
  sku: string;
  uom?: string;
};

export type UpdateInventoryItemRequest = {
  allow_negative_stock?: string;
  description?: string;
  is_active?: string;
  name?: string;
  reorder_point_qty?: string;
};

export type StockLevelDto = {
  avg_unit_cost_minor: number;
  item_id: string;
  last_movement_at?: string;
  qty_on_hand: number;
  updated_at: string;
  warehouse_id: string;
};

export type StockLevelListResponse = {
  items: StockLevelDto[];
};

export type StockMovementDto = {
  avg_unit_cost_minor_after?: number;
  /** Present only when this movement was an issue/transfer-out and a COGS
journal was posted to finance-service. */
  cogs_journal_public_id?: string;
  created_at: string;
  currency: string;
  id: string;
  item_id: string;
  low_stock?: boolean;
  memo?: string;
  movement_type: string;
  qty_delta: number;
  qty_on_hand_after?: number;
  source_id?: string;
  source_type?: string;
  unit_cost_minor: number;
  warehouse_id: string;
};

export type StockMovementListResponse = {
  items: StockMovementDto[];
  total: number;
};

export type CreateStockMovementRequest = {
  item_id: string;
  memo?: string;
  movement_type: string;
  /** Signed quantity delta. Positive for receipt/return/transfer_in,
negative for issue/transfer_out. `adjustment` may be either sign. */
  qty_delta: number;
  source_id?: string;
  source_type?: string;
  unit_cost_minor?: string;
  warehouse_id: string;
};

export type ReconcileStockRequest = {
  item_id?: string;
  warehouse_id?: string;
};

export type ReconcileStockResponse = {
  alerts: DriftAlertDto[];
  checked: number;
  drift_count: number;
};

export type DriftAlertDto = {
  cached_qty: number;
  detected_at: string;
  id: string;
  item_id: string;
  movement_sum_qty: number;
  warehouse_id: string;
};

export type SupplierDto = {
  created_at: string;
  currency: string;
  email?: string;
  id: string;
  name: string;
  payment_terms?: string;
  phone?: string;
  updated_at: string;
  version: number;
};

export type SupplierListResponse = {
  items: SupplierDto[];
  total: number;
};

export type CreateSupplierRequest = {
  currency: string;
  email?: string;
  name: string;
  payment_terms?: string;
  phone?: string;
};

export type UpdateSupplierRequest = {
  email?: string;
  name?: string;
  payment_terms?: string;
  phone?: string;
};

export type PurchaseRequestLineDto = {
  id: string;
  item_id: string;
  line_amount_minor: number;
  qty: number;
  unit_cost_estimate_minor: number;
};

export type PurchaseRequestDto = {
  approval_id?: string;
  budget_account_code?: string;
  created_at: string;
  currency: string;
  id: string;
  lines: PurchaseRequestLineDto[];
  notes?: string;
  requester_user_id: string;
  status: string;
  total_amount_minor: number;
  updated_at: string;
  version: number;
};

export type PurchaseRequestListResponse = {
  items: PurchaseRequestDto[];
  total: number;
};

export type CreatePurchaseRequestLineRequest = {
  item_id: string;
  qty: number;
  unit_cost_estimate_minor: number;
};

export type CreatePurchaseRequestRequest = {
  budget_account_code?: string;
  currency: string;
  lines: CreatePurchaseRequestLineRequest[];
  notes?: string;
};

export type DecidePurchaseRequestRequest = {
  approve: boolean;
  note?: string;
};

export type PurchaseOrderLineDto = {
  id: string;
  item_id: string;
  line_amount_minor: number;
  qty_ordered: number;
  qty_received: number;
  unit_cost_minor: number;
  warehouse_id: string;
};

export type PurchaseOrderDto = {
  created_at: string;
  currency: string;
  id: string;
  issued_at?: string;
  lines: PurchaseOrderLineDto[];
  purchase_request_id?: string;
  status: string;
  supplier_id: string;
  total_amount_minor: number;
  updated_at: string;
  version: number;
};

export type PurchaseOrderListResponse = {
  items: PurchaseOrderDto[];
  total: number;
};

export type CreatePurchaseOrderLineRequest = {
  item_id: string;
  qty_ordered: number;
  unit_cost_minor: number;
  warehouse_id: string;
};

export type CreatePurchaseOrderRequest = {
  currency: string;
  lines: CreatePurchaseOrderLineRequest[];
  purchase_request_id?: string;
  supplier_id: string;
};

export type GoodsReceiptLineDto = {
  id: string;
  item_id: string;
  po_line_id: string;
  qty_received: number;
  unit_cost_minor: number;
  warehouse_id: string;
};

export type GoodsReceiptDto = {
  created_at: string;
  id: string;
  journal_public_id?: string;
  lines: GoodsReceiptLineDto[];
  purchase_order_id: string;
  received_at?: string;
  status: string;
  updated_at: string;
  version: number;
};

export type GoodsReceiptListResponse = {
  items: GoodsReceiptDto[];
  total: number;
};

export type CreateGoodsReceiptLineRequest = {
  po_line_id: string;
  qty_received: number;
  /** Defaults to the PO line's `unit_cost_minor` when omitted. */
  unit_cost_minor?: string;
};

export type CreateGoodsReceiptRequest = {
  lines: CreateGoodsReceiptLineRequest[];
  purchase_order_id: string;
};

export type InventoryAssetDto = {
  accumulated_depreciation_minor: number;
  acquired_at?: string;
  acquisition_cost_minor: number;
  asset_tag?: string;
  created_at: string;
  currency: string;
  id: string;
  item_id?: string;
  last_depreciated_at?: string;
  name: string;
  salvage_minor: number;
  status: string;
  updated_at: string;
  useful_life_months: number;
  version: number;
};

export type InventoryAssetListResponse = {
  items: InventoryAssetDto[];
  total: number;
};

export type CreateInventoryAssetRequest = {
  acquired_at?: string;
  acquisition_cost_minor: number;
  asset_tag?: string;
  currency: string;
  item_id?: string;
  name: string;
  salvage_minor?: string;
  useful_life_months?: string;
};

export type UpdateInventoryAssetRequest = {
  asset_tag?: string;
  name?: string;
  salvage_minor?: string;
  useful_life_months?: string;
};

export type AssetAssignmentDto = {
  asset_id: string;
  assigned_at: string;
  assignee_employee_public_id: string;
  id: string;
  notes?: string;
  returned_at?: string;
};

export type AssignAssetRequest = {
  assignee_employee_public_id: string;
  notes?: string;
};

export type ReturnAssetRequest = {
  notes?: string;
};

export type DepreciateAssetRequest = {
  /** ISO date to depreciate through (defaults to today). */
  as_of_date?: string;
};

export type DepreciateAssetResponse = {
  asset: InventoryAssetDto;
  depreciation_expense_minor: number;
  journal_public_id?: string;
};

export type MaintenanceScheduleDto = {
  asset_id: string;
  id: string;
  interval_days: number;
  last_completed_at?: string;
  next_due_at: string;
  notes?: string;
  title: string;
};

export type MaintenanceScheduleListResponse = {
  items: MaintenanceScheduleDto[];
};

export type CreateMaintenanceScheduleRequest = {
  asset_id: string;
  interval_days: number;
  next_due_at: string;
  notes?: string;
  title: string;
};

export type CreateVendorBillFromReceiptRequest = {
  goods_receipt_id: string;
  memo?: string;
  supplier_ref: string;
};

export type VendorBillProxyDto = {
  amount_minor: number;
  amount_paid_minor: number;
  currency: string;
  id: string;
  payment_journal_public_id?: string;
  source_id?: string;
  source_type: string;
  status: string;
  supplier_ref: string;
};

export type VendorBillDto = {
  amount_minor: number;
  amount_paid_minor: number;
  created_at: string;
  currency: string;
  id: string;
  memo?: string;
  payment_journal_public_id?: string;
  source_id?: string;
  source_type: string;
  status: string;
  supplier_ref: string;
  updated_at: string;
  version: number;
};

export type VendorBillListResponse = {
  items: VendorBillDto[];
  total: number;
};

export type CreateVendorBillRequest = {
  amount_minor: number;
  currency: string;
  memo?: string;
  source_id?: string;
  source_type?: string;
  supplier_ref: string;
};

export type PayVendorBillRequest = {
  /** Defaults to the full outstanding balance when omitted. */
  amount_minor?: string;
  memo?: string;
};

export type AccessReviewQuery = {
  /** RFC3339 timestamp. */
  period_end: string;
  /** RFC3339 timestamp. */
  period_start: string;
  permission_id: string;
};

export type EntitlementRow = {
  effective_from: string;
  effective_to?: string;
  email: string;
  permission_id: string;
  role_key: string;
  user_id: string;
};

export type WhoCouldSeeResponse = {
  items: EntitlementRow[];
};

export type AuditReadRow = {
  action: string;
  created_at: string;
  email: string;
  metadata: Record<string, unknown>;
  resource_id: string;
  resource_type: string;
  user_id: string;
};

export type WhoDidSeeResponse = {
  items: AuditReadRow[];
};

export type AccessReviewKickoffRequest = {
  /** RFC3339 timestamp. */
  period_end: string;
  /** RFC3339 timestamp. */
  period_start: string;
  permission_id: string;
};

export type AccessReviewRunView = {
  completed_at?: string;
  created_at: string;
  id: string;
  period_end: string;
  period_start: string;
  permission_id: string;
  status: string;
  summary: Record<string, unknown>;
};

export type AuditVerifyRequest = {
  /** `YYYY-MM`, or `None` to verify all partitions for the org. */
  partition_key?: string;
};

export type AuditVerifyResponse = {
  first_break?: string;
  ok: boolean;
  partitions_checked: number;
  rows_checked: number;
};

export type RetentionConfigView = {
  default_retention_days: number;
  overrides: Record<string, unknown>;
  updated_at: string;
  version: number;
};

export type UpdateRetentionRequest = {
  default_retention_days?: string;
  overrides?: Record<string, unknown>;
};

export type RetentionDryRunResponse = {
  cutoff_date: string;
  partitions: string[];
  would_affect_estimate: number;
};

export type ApiKeyView = {
  created_at: string;
  expires_at?: string;
  id: string;
  key_prefix: string;
  last_used_at?: string;
  name: string;
  revoked_at?: string;
  scopes: string[];
};

export type ApiKeyListResponse = {
  items: ApiKeyView[];
};

export type CreateApiKeyRequest = {
  /** RFC3339 timestamp, or `None` for no expiry. */
  expires_at?: string;
  name: string;
  scopes: string[];
};

export type CreateApiKeyResponse = {
  key: ApiKeyView;
  /** Raw secret — returned only once, at creation time. */
  secret: string;
};

export type RotateApiKeyResponse = {
  key: ApiKeyView;
  /** Raw secret — returned only once, at rotation time. */
  secret: string;
};

export type WorkflowDefinitionDto = {
  created_at: string;
  created_by: string;
  current_published_version?: string;
  description: string;
  graph?: string;
  id: string;
  latest_version_id?: string;
  name: string;
  status: string;
  updated_at: string;
};

export type WorkflowDefinitionListResponse = {
  items: WorkflowDefinitionDto[];
};

export type CreateWorkflowDefinitionRequest = {
  description?: string;
  graph: WorkflowGraph;
  name: string;
};

export type UpdateWorkflowDefinitionRequest = {
  description?: string;
  graph?: string;
  name?: string;
};

export type WorkflowVersionDto = {
  created_at: string;
  definition_id: string;
  graph: WorkflowGraph;
  id: string;
  published_at?: string;
  published_by?: string;
  required_permissions: string[];
  version: number;
};

export type WorkflowVersionListResponse = {
  items: WorkflowVersionDto[];
};

export type PublishWorkflowRequest = {
  /** Optional note; publish always creates a new immutable version from current draft graph. */
  note?: string;
};

export type StartWorkflowRequest = {
  /** When true, start is rejected — use /simulate instead (defense in depth). */
  dry_run?: boolean;
  payload?: Record<string, unknown>;
};

export type WorkflowInstanceDto = {
  actor_user_id: string;
  completed_at?: string;
  current_node_id?: string;
  definition_id: string;
  error_message?: string;
  id: string;
  sla_deadline?: string;
  started_at: string;
  status: string;
  step_count: number;
  temporal_workflow_id: string;
  updated_at: string;
  version_id: string;
  version_number: number;
  waiting_until?: string;
};

export type WorkflowInstanceListResponse = {
  items: WorkflowInstanceDto[];
};

export type WorkflowGraph = {
  /** Entry node id after the trigger fires. */
  entry: string;
  nodes: WorkflowNode[];
  /** Optional SLA deadline from start (seconds). Soft signal for monitor. */
  sla_seconds?: string;
  trigger: WorkflowTrigger;
};

export type WorkflowTrigger = {

};

export type WorkflowNode = {

};

export type BranchArm = {
  equals: Record<string, unknown>;
  next: string;
  path: string;
};

export type HumanStepKind = "approval" | "inbox";

export type SimulateRequest = {
  graph: WorkflowGraph;
  /** Optional override; defaults to org max_steps_per_instance. */
  max_steps?: string;
  payload?: Record<string, unknown>;
};

export type SimulateResult = {
  error?: string;
  ok: boolean;
  /** Always true — documents the dry-run contract for clients/tests. */
  side_effects: boolean;
  steps: SimulateStepResult[];
};

export type SimulateStepResult = {
  action?: string;
  detail?: string;
  node_id: string;
  node_type: string;
  permission?: string;
  permission_allowed?: string;
  status: string;
  step_index: number;
};

export type TriggerCatalogueEntry = {
  aggregate: string;
  context: string;
  description: string;
  event_key: string;
  event_type: string;
  /** Subject suffix after org: `{context}.{aggregate}.{event}.v1` */
  subject_suffix: string;
};

export type ActionCatalogueEntry = {
  description: string;
  /** High-risk actions (journals) stay in catalogue but Member cannot use them. */
  high_risk: boolean;
  /** Relative HTTP path under the owning service (for documentation / activities). */
  http_method: string;
  http_path: string;
  key: string;
  /** Permission checked at save time and at run time (deny by default). */
  required_permission: string;
};

export type TriggerCatalogueResponse = {
  items: TriggerCatalogueEntry[];
};

export type ActionCatalogueResponse = {
  items: ActionCatalogueEntry[];
};

export type FixtureWorkflowDto = {
  description: string;
  graph: WorkflowGraph;
  name: string;
};

export type FixtureListResponse = {
  items: FixtureWorkflowDto[];
};

export type OrgBoundsDto = {
  max_concurrent: number;
  max_steps_per_instance: number;
};

export type UpdateOrgBoundsRequest = {
  max_concurrent: number;
  max_steps_per_instance: number;
};

export type MonitorSummaryDto = {
  cancelled: number;
  completed: number;
  failed: number;
  running: number;
  sla_breached: number;
  waiting: number;
};

export type MonitorResponse = {
  instances: WorkflowInstanceDto[];
  summary: MonitorSummaryDto;
};

export type MigrateInstanceRequest = {
  /** Stub: keep-old-version is the safe default; explicit migrate later. */
  target_version: number;
};

export type InvoiceIssuedFact = {
  amount_minor?: string;
  currency?: string;
  event_id: string;
  invoice_id: string;
  issued_at: string;
  org_id: string;
};

export type FactsResponse = {
  facts: InvoiceIssuedFact[];
};

export type AnalyticsIngestResponse = {
  accepted: boolean;
  duplicate: boolean;
  fact?: string;
};

export type AnalyticsReconcileResponse = {
  expected_count: number;
  matched: boolean;
  mirror_count: number;
};

export type FactSource = "deal_stage_change" | "invoice_lifecycle" | "payment" | "expense" | "task_lifecycle" | "ai_usage" | "api_request" | "invoice_issued";

export type MeasureKind = "sum" | "count" | "avg";

export type MetricUnit = "money_minor" | "count" | "tokens";

export type MetricDefinition = {
  description: string;
  dimensions: string[];
  display_name: string;
  drill_route: string;
  fact: FactSource;
  /** Flagship metrics appear in benchmark/trend views. */
  flagship: boolean;
  measure: MeasureKind;
  /** Column used for sum/avg (ignored for count). */
  measure_field: string;
  /** Stable machine name — unique across the catalogue. */
  name: string;
  /** Permission required to see this metric's values (same as fact source). */
  required_permission: string;
  unit: MetricUnit;
};

export type MetricListResponse = {
  metrics: MetricDefinition[];
};

export type QueryFilter = {
  field: string;
  op: string;
  value: Record<string, unknown>;
};

export type QueryRow = {
  dimensions: Record<string, unknown>;
  drill_links: string[];
  record_ids: string[];
  value: number;
};

export type QueryResult = {
  dry_run: boolean;
  elapsed_ms: number;
  eventually_consistent: boolean;
  filtered_by_permission: boolean;
  freshness_as_of?: string;
  metric: string;
  permission_denied_empty: boolean;
  rows: QueryRow[];
};

export type ReportDefinition = {
  dimensions?: string[];
  filters?: QueryFilter[];
  group_by?: string[];
  metric: string;
  /** Must be present — query guard rejects missing org_id. */
  org_id?: string;
  visualization?: string;
};

export type ReportDto = {
  created_at: string;
  created_by: string;
  definition: ReportDefinition;
  description: string;
  id: string;
  name: string;
  org_id: string;
  updated_at: string;
  updated_by: string;
  visualization: string;
};

export type CreateReportRequest = {
  definition: ReportDefinition;
  description?: string;
  name: string;
};

export type UpdateReportRequest = {
  definition?: string;
  description?: string;
  name?: string;
};

export type ReportListResponse = {
  reports: ReportDto[];
};

export type RunReportRequest = {
  dry_run?: boolean;
};

export type RunReportResponse = {
  report_id?: string;
  result: QueryResult;
  run_id: string;
};

export type SimulateQueryRequest = {
  definition: ReportDefinition;
};

export type DashboardDto = {
  created_at: string;
  created_by: string;
  description: string;
  id: string;
  layout: Record<string, unknown>;
  name: string;
  org_id: string;
  updated_at: string;
  updated_by: string;
  widgets: WidgetDto[];
};

export type CreateDashboardRequest = {
  description?: string;
  layout?: Record<string, unknown>;
  name: string;
};

export type UpdateDashboardRequest = {
  description?: string;
  layout?: Record<string, unknown>;
  name?: string;
};

export type DashboardListResponse = {
  dashboards: DashboardDto[];
};

export type WidgetDto = {
  config: Record<string, unknown>;
  created_at: string;
  dashboard_id: string;
  id: string;
  metric_name: string;
  position: number;
  title: string;
  visualization: string;
};

export type UpsertWidgetRequest = {
  config?: Record<string, unknown>;
  id?: string;
  metric_name: string;
  position?: number;
  title: string;
  visualization?: string;
};

export type ForecastMethod = "trailing_average" | "linear_trend";

export type ForecastRequest = {
  history_periods?: number;
  horizon_periods?: number;
  method?: string;
  org_id: string;
  /** One of: revenue, cash_flow, pipeline, headcount (maps to governed metrics). */
  series: string;
};

export type ForecastPoint = {
  period_index: number;
  period_label: string;
  value: number;
};

export type ForecastInputs = {
  history_periods: number;
  history_values: number[];
  horizon_periods: number;
  method_params: Record<string, unknown>;
};

export type ForecastResponse = {
  explainability: string;
  forecast: ForecastPoint[];
  history: ForecastPoint[];
  /** Explicit inputs used — DoD: every forecast exposes inputs + method. */
  inputs: ForecastInputs;
  method: ForecastMethod;
  metric: string;
  series: string;
  unit: MetricUnit;
};

export type ExportRequest = {
  format?: string;
};

export type ExportResponse = {
  content: string;
  content_type: string;
  file_id: string;
  format: string;
  report_id: string;
  row_count: number;
  run_id: string;
};

export type ScheduleDto = {
  channel: string;
  created_at: string;
  cron: string;
  enabled: boolean;
  export_format: string;
  id: string;
  last_run_at?: string;
  next_run_at?: string;
  recipients: string[];
  report_id: string;
  timezone: string;
  updated_at: string;
};

export type CreateScheduleRequest = {
  channel?: string;
  cron: string;
  enabled?: boolean;
  export_format?: string;
  recipients?: string[];
  report_id: string;
  timezone?: string;
};

export type UpdateScheduleRequest = {
  channel?: string;
  cron?: string;
  enabled?: string;
  export_format?: string;
  recipients?: string;
  timezone?: string;
};

export type FireScheduleRequest = {
  channel?: string;
  export_format?: string;
};

export type FireScheduleResponse = {
  export: ExportResponse;
  run_id: string;
  schedule_id: string;
  state: string;
  workflow_id: string;
  workflow_type: string;
};

export type FreshnessResponse = {
  eventually_consistent: boolean;
  lag_seconds: number;
  last_event_at?: string;
  last_ingest_at?: string;
  org_id: string;
};

export type BenchmarkMetric = {
  current_value: number;
  display_name: string;
  metric: string;
  previous_value: number;
  trend_percent?: string;
  unit: MetricUnit;
};

export type BenchmarkResponse = {
  benchmarks: BenchmarkMetric[];
  org_id: string;
  window_days: number;
};

export type DrillRequest = {
  definition: ReportDefinition;
  limit?: string;
};

export type DrillRecord = {
  link: string;
  record_id: string;
};

export type DrillResponse = {
  filtered_by_permission: boolean;
  metric: string;
  records: DrillRecord[];
};
