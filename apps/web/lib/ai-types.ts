/** TypeScript mirrors of services/ai/ai-service/src/types.rs */

export type Citation = {
  record_type: string;
  record_id: string;
  title: string;
  href?: string | null;
  snippet?: string | null;
};

export type ProposalView = {
  id: string;
  tool_name: string;
  action_type: string;
  status: string;
  command: Record<string, unknown>;
  rendered_diff: string;
  citations: Citation[];
  created_at: string;
};

export type ChatResponse = {
  session_id: string;
  interaction_id: string;
  role: string;
  content: string;
  citations: Citation[];
  follow_ups: string[];
  proposals: ProposalView[];
  tool_trace: unknown[];
  usage: {
    model: string;
    prompt_template_version: string;
    input_tokens: number;
    output_tokens: number;
    latency_ms: number;
    cost_estimate_minor: number;
    currency: string;
  };
};

export type AskFormField = {
  name: string;
  label: string;
  value: string;
  type: string;
};

export type AskForm = {
  action_type: string;
  fields: AskFormField[];
  proposal_preview?: string | null;
};

export type AskResponse = {
  kind: string;
  message?: string | null;
  form?: AskForm | null;
  citations?: Citation[] | null;
  tool_trace?: unknown[] | null;
};

export type ModulesEnabled = {
  copilot: boolean;
  insights: boolean;
  document_ai: boolean;
  ask_mode: boolean;
};

export type DataSharingSettings = {
  share_with_provider: boolean;
  allow_training: boolean;
};

export type AiSettings = {
  modules_enabled: ModulesEnabled;
  model_preference: string;
  auto_execute_allow_list: string[];
  data_sharing: DataSharingSettings;
  monthly_token_budget: number;
  tokens_used_this_month: number;
  budget_month: string;
};

export type InsightObservation = {
  id: string;
  title: string;
  body: string;
  evidence: Citation[];
  suggested_action?: string | null;
  estimate: boolean;
  insight_type?: string | null;
  status?: string | null;
  suggested_action_detail?: Record<string, unknown> | null;
  proposal_id?: string | null;
};

export type InsightsResponse = {
  observations: InsightObservation[];
  empty_reason?: string | null;
};

export type InsightsRefreshResponse = {
  created: number;
  observations: InsightObservation[];
  pending_proposals: string[];
};

export type MeetingSummaryView = {
  id: string;
  public_id: string;
  calendar_event_id: string;
  calendar_connector: string;
  transcript?: string | null;
  summary_markdown: string;
  action_items: unknown;
  status: string;
  accepted_at?: string | null;
  accepted_by?: string | null;
  created_at: string;
};

export type DocumentReview = {
  id: string;
  kind: string;
  confidence: number;
  extracted: Record<string, unknown>;
  proposal_id?: string | null;
  status: string;
};

export type SessionSummary = {
  id: string;
  title: string;
  page_scope?: string | null;
  updated_at: string;
};

export type SessionDetail = {
  id: string;
  title: string;
  page_scope?: string | null;
  updated_at: string;
  interactions: ChatResponse[];
};

export type SuggestionChip = {
  id: string;
  label: string;
  action_type: string;
  proposal_id?: string | null;
};

export type ChatMessage = {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  citations?: Citation[];
  follow_ups?: string[];
  proposals?: ProposalView[];
  streaming?: boolean;
};

const SCOPE_LABELS: Record<string, string> = {
  dashboard: 'dashboard',
  sales: 'sales',
  finance: 'finance',
  operations: 'operations',
  ops: 'operations',
  settings: 'settings',
  inbox: 'inbox',
};

export function pageScopeFromPathname(pathname: string | null): string {
  if (!pathname || pathname === '/') return 'dashboard';
  const segment = pathname.split('/').filter(Boolean)[0] ?? 'dashboard';
  if (segment === 'ops') return 'operations';
  return SCOPE_LABELS[segment] ?? segment;
}

export function pageScopeLabel(scope: string): string {
  return SCOPE_LABELS[scope] ?? scope;
}

export function citationHref(c: Citation): string {
  if (c.href) return c.href;
  switch (c.record_type) {
    case 'customer':
      return `/sales/customers/${c.record_id}`;
    case 'deal':
      return `/sales/deals?q=${encodeURIComponent(c.record_id)}`;
    case 'invoice':
      return `/finance/invoices/${c.record_id}`;
    case 'expense':
      return `/finance/expenses`;
    case 'task':
      return `/ops/tasks`;
    case 'project':
      return `/ops/projects/${c.record_id}`;
    default:
      return `/sales?q=${encodeURIComponent(c.title)}`;
  }
}

export function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}
