import { authFetch } from './auth-client';
import type {
  AiSettings,
  AskResponse,
  ChatResponse,
  DocumentReview,
  InsightsResponse,
  ProposalView,
  SessionDetail,
  SessionSummary,
  SuggestionChip,
} from './ai-types';

export async function confirmProposal(id: string): Promise<ProposalView | null> {
  const res = await authFetch(`/api/v1/ai/proposals/${encodeURIComponent(id)}/confirm`, {
    method: 'POST',
    body: JSON.stringify({}),
  });
  if (!res.ok) return null;
  return (await res.json()) as ProposalView;
}

export async function cancelProposal(id: string): Promise<boolean> {
  const res = await authFetch(`/api/v1/ai/proposals/${encodeURIComponent(id)}/cancel`, {
    method: 'POST',
    body: JSON.stringify({}),
  });
  return res.ok;
}

export async function fetchSessions(): Promise<SessionSummary[]> {
  const res = await authFetch('/api/v1/ai/sessions');
  if (!res.ok) return [];
  const body = (await res.json()) as { items: SessionSummary[] };
  return body.items ?? [];
}

export async function fetchSession(id: string): Promise<SessionDetail | null> {
  const res = await authFetch(`/api/v1/ai/sessions/${encodeURIComponent(id)}`);
  if (!res.ok) return null;
  return (await res.json()) as SessionDetail;
}

export async function sendChat(payload: {
  message: string;
  page_scope?: string;
  session_id?: string;
}): Promise<ChatResponse | null> {
  const res = await authFetch('/api/v1/ai/chat', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  if (!res.ok) return null;
  return (await res.json()) as ChatResponse;
}

export type StreamHandlers = {
  onToken: (token: string) => void;
  onCitation: (citation: import('./ai-types').Citation) => void;
  onProposal: (proposal: ProposalView) => void;
  onDone: (meta: { session_id: string; interaction_id: string }) => void;
};

export async function streamChat(
  payload: { message: string; page_scope?: string; session_id?: string },
  handlers: StreamHandlers,
): Promise<boolean> {
  const res = await authFetch('/api/v1/ai/chat/stream', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  if (!res.ok || !res.body) return false;

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let currentEvent = 'message';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';

    for (const line of lines) {
      if (line.startsWith('event:')) {
        currentEvent = line.slice(6).trim();
        continue;
      }
      if (!line.startsWith('data:')) continue;
      const data = line.slice(5).trim();
      if (!data) continue;

      switch (currentEvent) {
        case 'token':
          handlers.onToken(data);
          break;
        case 'citation':
          try {
            handlers.onCitation(JSON.parse(data));
          } catch {
            /* ignore malformed */
          }
          break;
        case 'proposal':
          try {
            handlers.onProposal(JSON.parse(data));
          } catch {
            /* ignore malformed */
          }
          break;
        case 'done':
          try {
            handlers.onDone(JSON.parse(data));
          } catch {
            /* ignore malformed */
          }
          break;
        default:
          break;
      }
    }
  }

  return true;
}

export async function askAi(query: string, pageScope?: string): Promise<AskResponse | null> {
  const res = await authFetch('/api/v1/ai/ask', {
    method: 'POST',
    body: JSON.stringify({ query, page_scope: pageScope }),
  });
  if (!res.ok) return null;
  return (await res.json()) as AskResponse;
}

export async function fetchInsights(): Promise<InsightsResponse | null> {
  const res = await authFetch('/api/v1/ai/insights');
  if (res.status === 403) {
    return { observations: [], empty_reason: 'Permission denied' };
  }
  if (!res.ok) return null;
  return (await res.json()) as InsightsResponse;
}

export async function fetchAiSettings(): Promise<AiSettings | null> {
  const res = await authFetch('/api/v1/ai/settings');
  if (!res.ok) return null;
  return (await res.json()) as AiSettings;
}

export async function patchAiSettings(
  patch: Partial<{
    modules_enabled: AiSettings['modules_enabled'];
    model_preference: string;
    auto_execute_allow_list: string[];
    data_sharing: AiSettings['data_sharing'];
    monthly_token_budget: number;
  }>,
): Promise<AiSettings | null> {
  const res = await authFetch('/api/v1/ai/settings', {
    method: 'PATCH',
    body: JSON.stringify(patch),
  });
  if (!res.ok) return null;
  return (await res.json()) as AiSettings;
}

export async function extractDocument(payload: {
  text?: string;
  file_id?: string;
  kind: 'expense' | 'invoice';
}): Promise<DocumentReview | null> {
  const res = await authFetch('/api/v1/ai/documents/extract', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  if (!res.ok) return null;
  return (await res.json()) as DocumentReview;
}

export async function fetchSuggestions(
  pageScope?: string,
  recordId?: string,
): Promise<SuggestionChip[]> {
  const params = new URLSearchParams();
  if (pageScope) params.set('page_scope', pageScope);
  if (recordId) params.set('record_id', recordId);
  const qs = params.toString();
  const res = await authFetch(`/api/v1/ai/suggestions${qs ? `?${qs}` : ''}`);
  if (!res.ok) return [];
  const body = (await res.json()) as { chips: SuggestionChip[] };
  return body.chips ?? [];
}
