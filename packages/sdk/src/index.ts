/** TypeScript SDK for CompanyOS Phase 1.2. */

export type {
  Hello,
  CreateHelloRequest,
  HelloListResponse,
  RegisterRequest,
  RegisterResponse,
  LoginRequest,
  TokenResponse,
  MfaChallengeResponse,
  SwitchOrgRequest,
  MeResponse,
  MembershipView,
  MembershipListResponse,
  SessionView,
  SessionListResponse,
  MessageResponse,
  CreateOrgRequest,
  OrgResponse,
  UpdateOrgSettingsRequest,
  MemberView,
  MemberListResponse,
  InviteMemberRequest,
  InviteResponse,
  RoleView,
  RoleListResponse,
  UpsertRoleRequest,
  CapabilityPreviewResponse,
  PermissionCatalogueResponse,
  TeamView,
  TeamListResponse,
  DepartmentView,
  DepartmentListResponse,
  MyCapabilitiesResponse,
} from './generated';

import type {
  Hello,
  CreateHelloRequest,
  HelloListResponse,
  LoginRequest,
  TokenResponse,
  SwitchOrgRequest,
  MeResponse,
  MembershipListResponse,
  SessionListResponse,
  CreateOrgRequest,
  OrgResponse,
  MemberListResponse,
  RoleListResponse,
  MyCapabilitiesResponse,
} from './generated';

export type CompanyOsClientOptions = {
  baseUrl: string;
  /** In-memory access token getter (preferred). */
  getAccessToken?: () => string | null | undefined;
  /** LOCAL-ONLY: org public id for X-CompanyOS-Dev-Org-Id (requires COMPANYOS_LOCAL_AUTH=1). */
  orgId?: string;
  /** LOCAL-ONLY: user public id or uuid for X-CompanyOS-Dev-User-Id. */
  userId?: string;
  getHeaders?: () => Record<string, string>;
};

/**
 * Thin TypeScript SDK. Prefer Bearer access tokens; keep them out of localStorage.
 */
export class CompanyOsClient {
  constructor(private readonly opts: CompanyOsClientOptions) {}

  private headers(): Record<string, string> {
    const h: Record<string, string> = {
      Accept: 'application/json',
      'Content-Type': 'application/json',
      ...(this.opts.getHeaders?.() ?? {}),
    };
    const token = this.opts.getAccessToken?.();
    if (token) h.Authorization = `Bearer ${token}`;
    if (this.opts.orgId) h['X-CompanyOS-Dev-Org-Id'] = this.opts.orgId;
    if (this.opts.userId) h['X-CompanyOS-Dev-User-Id'] = this.opts.userId;
    return h;
  }

  async login(body: LoginRequest): Promise<TokenResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/auth/login`, {
      method: 'POST',
      headers: this.headers(),
      credentials: 'include',
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`login failed: ${res.status}`);
    return (await res.json()) as TokenResponse;
  }

  async switchOrg(body: SwitchOrgRequest): Promise<TokenResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/auth/switch-org`, {
      method: 'POST',
      headers: this.headers(),
      credentials: 'include',
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`switchOrg failed: ${res.status}`);
    return (await res.json()) as TokenResponse;
  }

  async me(): Promise<MeResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/auth/me`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`me failed: ${res.status}`);
    return (await res.json()) as MeResponse;
  }

  async listMemberships(): Promise<MembershipListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/auth/memberships`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`listMemberships failed: ${res.status}`);
    return (await res.json()) as MembershipListResponse;
  }

  async listSessions(): Promise<SessionListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/auth/sessions`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`listSessions failed: ${res.status}`);
    return (await res.json()) as SessionListResponse;
  }

  async getOrganization(): Promise<OrgResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/workspace/organizations`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`getOrganization failed: ${res.status}`);
    return (await res.json()) as OrgResponse;
  }

  async createOrganization(body: CreateOrgRequest): Promise<OrgResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/workspace/organizations`, {
      method: 'POST',
      headers: this.headers(),
      credentials: 'include',
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`createOrganization failed: ${res.status}`);
    return (await res.json()) as OrgResponse;
  }

  async listMembers(): Promise<MemberListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/workspace/members`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`listMembers failed: ${res.status}`);
    return (await res.json()) as MemberListResponse;
  }

  async listRoles(): Promise<RoleListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/workspace/roles`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`listRoles failed: ${res.status}`);
    return (await res.json()) as RoleListResponse;
  }

  async myCapabilities(): Promise<MyCapabilitiesResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/workspace/me/capabilities`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) throw new Error(`myCapabilities failed: ${res.status}`);
    return (await res.json()) as MyCapabilitiesResponse;
  }

  async listHello(): Promise<HelloListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/hello`, {
      method: 'GET',
      headers: this.headers(),
      credentials: 'include',
    });
    if (!res.ok) {
      throw new Error(`listHello failed: ${res.status}`);
    }
    return (await res.json()) as HelloListResponse;
  }

  async createHello(body: CreateHelloRequest): Promise<Hello> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/hello`, {
      method: 'POST',
      headers: this.headers(),
      credentials: 'include',
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`createHello failed: ${res.status}`);
    }
    return (await res.json()) as Hello;
  }
}
