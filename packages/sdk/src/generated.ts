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
