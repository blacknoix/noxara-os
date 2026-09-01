/** Phase 4.3 agent types */

export type AgentPolicyDoc = {
  name: string;
  agent_types: string[];
  allowed_tools: string[];
  allowed_permissions: string[];
  spend_budget_tokens: number;
  max_steps: number;
  require_human_above: Record<string, unknown>;
  allowed_resource_scopes: string[];
};

export type PolicyView = {
  id: string;
  public_id: string;
  version: number;
  policy: AgentPolicyDoc;
};

export type KillSwitchView = {
  org_wide: boolean;
  agent_type: string;
  engaged: boolean;
  reason?: string | null;
  engaged_at?: string | null;
};

export type AgentRunView = {
  id: string;
  public_id: string;
  agent_type: string;
  status: string;
  policy_version: number;
  temporal_workflow_id: string;
  steps_taken: number;
  tokens_used: number;
  cost_estimate_minor: number;
  last_actions: unknown;
  error_message?: string | null;
  started_at: string;
  finished_at?: string | null;
};

export type AgentRunOutcome = {
  run: AgentRunView;
  tool_trace: unknown[];
  action_ids: string[];
};

export type PromptPackDoc = {
  name: string;
  allowed_models: string[];
  temperature: number;
  tool_subset: string[];
  system_preamble: string;
};

export type NlWorkflowDraft = {
  id: string;
  status: string;
  prompt: string;
  definition: Record<string, unknown>;
  filtered_actions: string[];
  note: string;
};

export type AgentReviewReport = {
  period_start: string;
  period_end: string;
  total_actions: number;
  failures: number;
  reversals: number;
  error_rate: number;
  max_error_rate: number;
  within_threshold: boolean;
  by_agent_type: { agent_type: string; total: number; failures: number; reversals: number }[];
};
