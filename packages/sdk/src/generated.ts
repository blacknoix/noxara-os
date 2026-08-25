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
  /** checklist | stat | module_empty | feed */
  kind: string;
  /** Widget-specific JSON body (checklist items, empty lists, module stubs). */
  payload: Record<string, unknown>;
  range_label?: string;
  /** module_not_enabled | coming_in_later_phase | no_data */
  reason_code?: string;
  /** Always false for honest empties in Phase 1.3 (pattern present for later). */
  stale: boolean;
  /** ready | empty | unavailable | loading */
  status: string;
  title: string;
};
