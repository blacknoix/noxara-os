import { authFetch } from './auth-client';
import type {
  AgentPolicyDoc,
  AgentReviewReport,
  AgentRunOutcome,
  AgentRunView,
  KillSwitchView,
  NlWorkflowDraft,
  PolicyView,
  PromptPackDoc,
} from './agent-types';

export async function fetchAgentPolicy(): Promise<PolicyView | null> {
  const res = await authFetch('/api/v1/ai/agents/policy');
  if (!res.ok) return null;
  return (await res.json()) as PolicyView;
}

export async function publishAgentPolicy(doc: AgentPolicyDoc): Promise<PolicyView | null> {
  const res = await authFetch('/api/v1/ai/agents/policy', {
    method: 'POST',
    body: JSON.stringify(doc),
  });
  if (!res.ok) return null;
  return (await res.json()) as PolicyView;
}

export async function fetchKillSwitch(): Promise<KillSwitchView | null> {
  const res = await authFetch('/api/v1/ai/agents/kill-switch');
  if (!res.ok) return null;
  return (await res.json()) as KillSwitchView;
}

export async function setKillSwitch(payload: {
  engaged: boolean;
  agent_type?: string;
  reason?: string;
}): Promise<KillSwitchView | null> {
  const res = await authFetch('/api/v1/ai/agents/kill-switch', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  if (!res.ok) return null;
  return (await res.json()) as KillSwitchView;
}

export async function fetchAgentRuns(): Promise<AgentRunView[]> {
  const res = await authFetch('/api/v1/ai/agents/runs');
  if (!res.ok) return [];
  const body = (await res.json()) as { items: AgentRunView[] };
  return body.items ?? [];
}

export async function startAgentRun(agentType: string): Promise<AgentRunOutcome | null> {
  const res = await authFetch('/api/v1/ai/agents/runs', {
    method: 'POST',
    body: JSON.stringify({ agent_type: agentType }),
  });
  if (!res.ok) return null;
  return (await res.json()) as AgentRunOutcome;
}

export async function fetchPromptPack(): Promise<PromptPackDoc | null> {
  const res = await authFetch('/api/v1/ai/agents/prompt-pack');
  if (!res.ok) return null;
  return (await res.json()) as PromptPackDoc;
}

export async function savePromptPack(doc: PromptPackDoc): Promise<PromptPackDoc | null> {
  const res = await authFetch('/api/v1/ai/agents/prompt-pack', {
    method: 'POST',
    body: JSON.stringify(doc),
  });
  if (!res.ok) return null;
  return (await res.json()) as PromptPackDoc;
}

export async function proposeNlWorkflow(prompt: string): Promise<NlWorkflowDraft | null> {
  const res = await authFetch('/api/v1/ai/agents/workflows/propose', {
    method: 'POST',
    body: JSON.stringify({ prompt }),
  });
  if (!res.ok) return null;
  return (await res.json()) as NlWorkflowDraft;
}

export async function fetchAgentReview(): Promise<AgentReviewReport | null> {
  const res = await authFetch('/api/v1/ai/agents/review');
  if (!res.ok) return null;
  return (await res.json()) as AgentReviewReport;
}
