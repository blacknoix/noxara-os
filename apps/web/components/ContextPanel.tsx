'use client';

import Link from 'next/link';
import { useCallback, useEffect, useRef, useState, type CSSProperties, type FormEvent } from 'react';
import { usePathname } from 'next/navigation';
import { Badge, Button, EmptyState, Textarea } from '@companyos/design-system';
import {
  cancelProposal,
  confirmProposal,
  fetchSession,
  fetchSessions,
  sendChat,
  streamChat,
} from '../lib/ai-api';
import {
  citationHref,
  pageScopeFromPathname,
  pageScopeLabel,
  type ChatMessage,
  type Citation,
  type ProposalView,
  type SessionSummary,
} from '../lib/ai-types';
import { getAccessToken } from '../lib/auth-client';

function newId(): string {
  return `msg_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

export function ContextPanel({ open }: { open: boolean }) {
  const pathname = usePathname();
  const pageScope = pageScopeFromPathname(pathname);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [sessionId, setSessionId] = useState<string | undefined>();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [sending, setSending] = useState(false);
  const [signedIn, setSignedIn] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);

  const loadSessions = useCallback(async () => {
    if (!getAccessToken()) {
      setSignedIn(false);
      setSessions([]);
      return;
    }
    setSignedIn(true);
    const items = await fetchSessions();
    setSessions(items);
  }, []);

  useEffect(() => {
    if (!open) return;
    void loadSessions();
  }, [open, loadSessions]);

  useEffect(() => {
    scrollToBottom();
  }, [messages, scrollToBottom]);

  const loadSession = useCallback(async (id: string) => {
    const detail = await fetchSession(id);
    if (!detail) return;
    setSessionId(detail.id);
    const restored: ChatMessage[] = detail.interactions.flatMap((interaction) => [
      {
        id: `asst_${interaction.interaction_id}`,
        role: 'assistant' as const,
        content: interaction.content,
        citations: interaction.citations,
        follow_ups: interaction.follow_ups,
        proposals: interaction.proposals,
      },
    ]);
    setMessages(restored);
  }, []);

  const handleProposalAction = useCallback(
    async (proposalId: string, action: 'confirm' | 'cancel') => {
      if (action === 'confirm') {
        const updated = await confirmProposal(proposalId);
        if (updated) {
          setMessages((prev) =>
            prev.map((m) => ({
              ...m,
              proposals: m.proposals?.map((p) => (p.id === proposalId ? updated : p)),
            })),
          );
        }
      } else {
        const ok = await cancelProposal(proposalId);
        if (ok) {
          setMessages((prev) =>
            prev.map((m) => ({
              ...m,
              proposals: m.proposals?.map((p) =>
                p.id === proposalId ? { ...p, status: 'cancelled' } : p,
              ),
            })),
          );
        }
      }
    },
    [],
  );

  const sendMessage = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed || sending || !getAccessToken()) return;

      const userMsg: ChatMessage = { id: newId(), role: 'user', content: trimmed };
      const assistantId = newId();
      const assistantMsg: ChatMessage = {
        id: assistantId,
        role: 'assistant',
        content: '',
        citations: [],
        follow_ups: [],
        proposals: [],
        streaming: true,
      };

      setMessages((prev) => [...prev, userMsg, assistantMsg]);
      setInput('');
      setSending(true);

      let activeSessionId = sessionId;
      let citations: Citation[] = [];
      let proposals: ProposalView[] = [];

      const streamed = await streamChat(
        { message: trimmed, page_scope: pageScope, session_id: sessionId },
        {
          onToken: (token) => {
            setMessages((prev) =>
              prev.map((m) =>
                m.id === assistantId ? { ...m, content: m.content + token } : m,
              ),
            );
          },
          onCitation: (c) => {
            citations = [...citations, c];
            setMessages((prev) =>
              prev.map((m) => (m.id === assistantId ? { ...m, citations: [...citations] } : m)),
            );
          },
          onProposal: (p) => {
            proposals = [...proposals, p];
            setMessages((prev) =>
              prev.map((m) => (m.id === assistantId ? { ...m, proposals: [...proposals] } : m)),
            );
          },
          onDone: (meta) => {
            activeSessionId = meta.session_id;
          },
        },
      );

      if (!streamed) {
        const response = await sendChat({
          message: trimmed,
          page_scope: pageScope,
          session_id: sessionId,
        });
        if (response) {
          activeSessionId = response.session_id;
          citations = response.citations;
          proposals = response.proposals;
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    content: response.content,
                    citations: response.citations,
                    follow_ups: response.follow_ups,
                    proposals: response.proposals,
                    streaming: false,
                  }
                : m,
            ),
          );
        } else {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    content: 'Could not reach Copilot. Sign in or try again.',
                    streaming: false,
                  }
                : m,
            ),
          );
        }
      } else {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? { ...m, streaming: false, citations, proposals }
              : m,
          ),
        );
      }

      if (activeSessionId) setSessionId(activeSessionId);
      setSending(false);
      void loadSessions();
    },
    [sending, sessionId, pageScope, loadSessions],
  );

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void sendMessage(input);
  }

  function startNewSession() {
    setSessionId(undefined);
    setMessages([]);
  }

  return (
    <aside
      aria-label="Copilot"
      aria-hidden={!open}
      style={{
        borderLeft: open ? '1px solid var(--cos-color-border)' : 'none',
        background: 'var(--cos-color-bg-elevated)',
        overflow: 'hidden',
        opacity: open ? 1 : 0,
        transition: 'opacity 200ms ease',
        minWidth: 0,
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
      }}
    >
      {open ? (
        <>
          <header style={headerStyle}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <h2 style={titleStyle}>Copilot</h2>
              <Badge tone="neutral">{pageScopeLabel(pageScope)}</Badge>
            </div>
            <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
              {sessions.length > 0 ? (
                <select
                  aria-label="Chat session"
                  value={sessionId ?? ''}
                  onChange={(e) => {
                    const id = e.target.value;
                    if (!id) startNewSession();
                    else void loadSession(id);
                  }}
                  style={selectStyle}
                >
                  <option value="">New chat</option>
                  {sessions.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.title}
                    </option>
                  ))}
                </select>
              ) : (
                <button type="button" onClick={startNewSession} style={ghostBtn}>
                  New chat
                </button>
              )}
            </div>
          </header>

          <div ref={listRef} role="log" aria-live="polite" aria-relevant="additions" style={listStyle}>
            {!signedIn ? (
              <EmptyState
                title="Sign in to use Copilot"
                description="Open login to ask questions and review proposals."
              />
            ) : messages.length === 0 ? (
              <EmptyState
                title="Ask anything"
                description={`Copilot is scoped to ${pageScopeLabel(pageScope)}. Try a question or suggested follow-up.`}
              />
            ) : (
              messages.map((m) => (
                <MessageBubble
                  key={m.id}
                  message={m}
                  onFollowUp={(q) => void sendMessage(q)}
                  onProposalAction={handleProposalAction}
                />
              ))
            )}
          </div>

          <form onSubmit={onSubmit} style={formStyle}>
            <Textarea
              label="Message Copilot"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              rows={2}
              disabled={!signedIn || sending}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  void sendMessage(input);
                }
              }}
            />
            <Button type="submit" size="sm" disabled={!signedIn || sending || !input.trim()}>
              {sending ? 'Sending…' : 'Send'}
            </Button>
          </form>
        </>
      ) : null}
    </aside>
  );
}

function MessageBubble({
  message,
  onFollowUp,
  onProposalAction,
}: {
  message: ChatMessage;
  onFollowUp: (text: string) => void;
  onProposalAction: (id: string, action: 'confirm' | 'cancel') => void;
}) {
  const isUser = message.role === 'user';
  return (
    <div
      style={{
        alignSelf: isUser ? 'flex-end' : 'flex-start',
        maxWidth: '100%',
        padding: '0.55rem 0.7rem',
        borderRadius: 'var(--cos-radius-sm)',
        background: isUser ? 'var(--cos-color-bg-muted)' : 'transparent',
        border: isUser ? 'none' : '1px solid var(--cos-color-border)',
        fontSize: '0.88rem',
        lineHeight: 1.5,
        color: 'var(--cos-color-fg)',
      }}
    >
      <p style={{ margin: 0, whiteSpace: 'pre-wrap' }}>
        {message.content}
        {message.streaming ? <span aria-hidden> …</span> : null}
      </p>

      {message.citations && message.citations.length > 0 ? (
        <ul style={citationList}>
          {message.citations.map((c) => (
            <li key={`${c.record_type}-${c.record_id}`}>
              <Link href={citationHref(c)} style={citationLink}>
                {c.title}
              </Link>
              {c.snippet ? (
                <span style={{ color: 'var(--cos-color-fg-muted)' }}> — {c.snippet}</span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}

      {message.proposals && message.proposals.length > 0 ? (
        <div style={{ marginTop: 8, display: 'grid', gap: 8 }}>
          {message.proposals.map((p) => (
            <ProposalCard key={p.id} proposal={p} onAction={onProposalAction} />
          ))}
        </div>
      ) : null}

      {!isUser && message.follow_ups && message.follow_ups.length > 0 ? (
        <div style={{ marginTop: 8, display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          {message.follow_ups.map((f) => (
            <button key={f} type="button" onClick={() => onFollowUp(f)} style={chipBtn}>
              {f}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ProposalCard({
  proposal,
  onAction,
}: {
  proposal: ProposalView;
  onAction: (id: string, action: 'confirm' | 'cancel') => void;
}) {
  const pending = proposal.status === 'pending';
  return (
    <div style={proposalStyle}>
      <pre style={diffStyle}>{proposal.rendered_diff}</pre>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
        <Badge tone={pending ? 'warning' : 'neutral'}>{proposal.status}</Badge>
        {pending ? (
          <>
            <Button type="button" size="sm" onClick={() => void onAction(proposal.id, 'confirm')}>
              Confirm
            </Button>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() => void onAction(proposal.id, 'cancel')}
            >
              Cancel
            </Button>
          </>
        ) : null}
      </div>
    </div>
  );
}

const headerStyle: CSSProperties = {
  padding: '1rem 1.1rem 0.75rem',
  borderBottom: '1px solid var(--cos-color-border)',
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.05rem',
  fontWeight: 550,
};

const selectStyle: CSSProperties = {
  fontSize: '0.8rem',
  padding: '0.3rem 0.45rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
  maxWidth: 160,
};

const ghostBtn: CSSProperties = {
  all: 'unset',
  cursor: 'pointer',
  fontSize: '0.8rem',
  color: 'var(--cos-color-accent)',
};

const listStyle: CSSProperties = {
  flex: 1,
  overflow: 'auto',
  padding: '0.75rem 1.1rem',
  display: 'flex',
  flexDirection: 'column',
  gap: 10,
  minHeight: 0,
};

const formStyle: CSSProperties = {
  padding: '0.75rem 1.1rem 1rem',
  borderTop: '1px solid var(--cos-color-border)',
  display: 'grid',
  gap: 8,
};

const citationList: CSSProperties = {
  listStyle: 'none',
  margin: '0.5rem 0 0',
  padding: 0,
  fontSize: '0.8rem',
};

const citationLink: CSSProperties = {
  color: 'var(--cos-color-accent)',
  textDecoration: 'underline',
  textUnderlineOffset: 2,
};

const chipBtn: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
  fontSize: '0.78rem',
  padding: '0.25rem 0.5rem',
  cursor: 'pointer',
};

const proposalStyle: CSSProperties = {
  padding: '0.5rem 0.6rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg)',
};

const diffStyle: CSSProperties = {
  margin: '0 0 0.5rem',
  fontSize: '0.78rem',
  fontFamily: 'var(--cos-font-mono, monospace)',
  whiteSpace: 'pre-wrap',
  color: 'var(--cos-color-fg-muted)',
};
