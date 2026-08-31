'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  InlineAlert,
  PermissionDeniedState,
} from '@companyos/design-system';
import {
  acceptMeetingSummary,
  fetchMeetingSummaries,
  rejectMeetingSummary,
} from '../../../lib/ai-api';
import type { MeetingSummaryView } from '../../../lib/ai-types';
import { getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

export default function MeetingSummariesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [items, setItems] = useState<MeetingSummaryView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const canRead = can('ai.meeting_summary.read');
  const canAccept = can('ai.meeting_summary.accept');

  const load = useCallback(async () => {
    if (!getAccessToken()) return;
    const data = await fetchMeetingSummaries();
    setItems(data);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onAccept(id: string) {
    setBusyId(id);
    setError(null);
    const updated = await acceptMeetingSummary(id);
    setBusyId(null);
    if (!updated) {
      setError('Accept failed — confirmation is required and does not create tasks automatically.');
      return;
    }
    setItems((prev) => prev.map((m) => (m.id === id ? updated : m)));
  }

  async function onReject(id: string) {
    setBusyId(id);
    setError(null);
    const ok = await rejectMeetingSummary(id);
    setBusyId(null);
    if (!ok) {
      setError('Reject failed.');
      return;
    }
    await load();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to continue." />;
  }

  if (capsLoading) {
    return <p style={muted}>Loading…</p>;
  }

  if (!canRead) {
    return <PermissionDeniedState requiredPermission="ai.meeting_summary.read" />;
  }

  return (
    <section style={{ maxWidth: 720 }}>
      <p style={eyebrow}>
        <Link href="/settings/ai" style={{ color: 'inherit' }}>
          AI settings
        </Link>
        {' / '}
        Meeting summaries
      </p>
      <h1 style={h1}>Meeting summaries</h1>
      <p style={muted}>
        Suggestions from the calendar connector (calendar.microsoft). AI proposes; humans accept.
        Accepting does not auto-create tasks.
      </p>

      {error ? <InlineAlert tone="danger" title={error} /> : null}

      {items.length === 0 ? (
        <EmptyState
          title="No meeting summaries"
          description="Summaries appear after a calendar connector sync proposes them."
        />
      ) : (
        <ul style={{ listStyle: 'none', margin: '1.25rem 0 0', padding: 0, display: 'grid', gap: 16 }}>
          {items.map((m) => (
            <li key={m.id} style={card}>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                <strong>{m.calendar_event_id}</strong>
                <Badge tone={m.status === 'accepted' ? 'success' : 'neutral'}>{m.status}</Badge>
                <Badge tone="neutral">{m.calendar_connector}</Badge>
              </div>
              <pre style={summary}>{m.summary_markdown}</pre>
              {m.status === 'suggested' && canAccept ? (
                <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
                  <Button
                    type="button"
                    disabled={busyId === m.id}
                    onClick={() => void onAccept(m.id)}
                  >
                    Accept
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={busyId === m.id}
                    onClick={() => void onReject(m.id)}
                  >
                    Reject
                  </Button>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.8rem',
  color: 'var(--cos-color-fg-muted)',
  textTransform: 'uppercase',
  letterSpacing: '0.04em',
};
const h1: CSSProperties = { margin: '0.35rem 0 0.5rem', fontSize: '1.6rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)', fontSize: '0.95rem' };
const card: CSSProperties = {
  padding: '1rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  background: 'var(--cos-color-bg)',
};
const summary: CSSProperties = {
  margin: '0.75rem 0 0',
  whiteSpace: 'pre-wrap',
  fontFamily: 'inherit',
  fontSize: '0.9rem',
  lineHeight: 1.45,
};
