'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';

type ApprovalStep = {
  order: number;
  status: string;
  approver_role?: string | null;
  assignee_user_ids: string[];
  sla_seconds?: number | null;
};

type RoutingSnapshot = {
  policy_public_id: string;
  policy_name: string;
  policy_version: number;
  mode: string;
  rationale: string;
  steps: ApprovalStep[];
};

type Approval = {
  id: string;
  subject_type: string;
  subject_id: string;
  status: string;
  title: string;
  summary?: string | null;
  amount_minor?: number | null;
  currency?: string | null;
  policy_version: number;
  current_step: number;
  routing_snapshot: RoutingSnapshot;
  decision_note?: string | null;
  created_at: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Approval[] };

const pageStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: '1rem',
  maxWidth: '52rem',
};

const cardStyle: CSSProperties = {
  borderTop: '1px solid var(--cos-color-border)',
  padding: '1rem 0',
  display: 'flex',
  flexDirection: 'column',
  gap: '0.65rem',
};

const rationaleStyle: CSSProperties = {
  fontSize: '0.875rem',
  color: 'var(--cos-color-muted)',
  lineHeight: 1.45,
  background: 'var(--cos-color-surface-muted, color-mix(in srgb, var(--cos-color-border) 35%, transparent))',
  padding: '0.75rem 0.9rem',
};

const actionsStyle: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: '0.5rem',
  alignItems: 'center',
};

export default function ApprovalsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [comment, setComment] = useState('');

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    try {
      const res = await authFetch(
        '/api/v1/operations/approvals?pending_for_me=true&status=pending',
      );
      if (res.status === 401) {
        setState({ status: 'signed_out' });
        return;
      }
      if (res.status === 403) {
        setState({ status: 'denied' });
        return;
      }
      if (!res.ok) {
        setState({ status: 'error', message: 'Could not load approvals.' });
        return;
      }
      const body = await res.json();
      setState({ status: 'ready', items: body.items ?? [] });
    } catch {
      setState({ status: 'error', message: 'Approvals request failed.' });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function decide(id: string, approve: boolean) {
    setBusy(id);
    try {
      const res = await authFetch(`/api/v1/operations/approvals/${id}/decide`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Idempotency-Key': `web-${id}-${approve}-${Date.now()}`,
        },
        body: JSON.stringify({ approve, comment: comment || null }),
      });
      if (!res.ok) {
        setState({ status: 'error', message: 'Decision failed.' });
        return;
      }
      setComment('');
      await load();
    } finally {
      setBusy(null);
    }
  }

  async function bulkDecide(approve: boolean) {
    if (selected.size === 0) return;
    setBusy('bulk');
    try {
      const res = await authFetch('/api/v1/operations/approvals/bulk-decide', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          ids: Array.from(selected),
          approve,
          comment: comment || null,
        }),
      });
      if (!res.ok) {
        setState({ status: 'error', message: 'Bulk decide failed.' });
        return;
      }
      setSelected(new Set());
      setComment('');
      await load();
    } finally {
      setBusy(null);
    }
  }

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <section style={pageStyle} aria-labelledby="approvals-heading">
      <header>
        <h1
          id="approvals-heading"
          style={{
            margin: 0,
            fontFamily: 'var(--cos-font-display)',
            fontSize: '1.75rem',
            fontWeight: 650,
          }}
        >
          Approvals
        </h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-muted)' }}>
          Pending items assigned to you. Approve or reject with an optional comment.
        </p>
      </header>

      {state.status === 'loading' ? <LoadingState label="Loading approvals" /> : null}
      {state.status === 'signed_out' ? (
        <EmptyState title="Sign in required" description="Sign in to review approvals." />
      ) : null}
      {state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="operations.approval.read" />
      ) : null}
      {state.status === 'error' ? (
        <ErrorState title="Something went wrong" message={state.message} />
      ) : null}

      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState
          title="Nothing to approve"
          description="When expenses or quote discounts need a decision, they appear here."
        />
      ) : null}

      {state.status === 'ready' && state.items.length > 0 ? (
        <>
          <div style={actionsStyle}>
            <label htmlFor="approval-comment" style={{ fontSize: '0.875rem' }}>
              Comment
            </label>
            <input
              id="approval-comment"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              placeholder="Optional note"
              style={{
                flex: '1 1 12rem',
                minWidth: 0,
                padding: '0.45rem 0.6rem',
                border: '1px solid var(--cos-color-border)',
                background: 'var(--cos-color-surface)',
                color: 'inherit',
              }}
            />
            <Button
              type="button"
              variant="secondary"
              disabled={selected.size === 0 || busy === 'bulk'}
              onClick={() => void bulkDecide(true)}
            >
              Bulk approve ({selected.size})
            </Button>
            <Button
              type="button"
              variant="ghost"
              disabled={selected.size === 0 || busy === 'bulk'}
              onClick={() => void bulkDecide(false)}
            >
              Bulk reject
            </Button>
          </div>

          <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
            {state.items.map((item) => (
              <li key={item.id} style={cardStyle}>
                <div style={{ display: 'flex', gap: '0.75rem', alignItems: 'flex-start' }}>
                  <input
                    type="checkbox"
                    aria-label={`Select ${item.title}`}
                    checked={selected.has(item.id)}
                    onChange={() => toggle(item.id)}
                  />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      style={{
                        display: 'flex',
                        flexWrap: 'wrap',
                        gap: '0.5rem',
                        alignItems: 'center',
                      }}
                    >
                      <strong style={{ fontSize: '1.05rem' }}>{item.title}</strong>
                      <Badge>{item.subject_type}</Badge>
                      <Badge>v{item.policy_version}</Badge>
                    </div>
                    {item.summary ? (
                      <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-muted)' }}>
                        {item.summary}
                      </p>
                    ) : null}
                    {typeof item.amount_minor === 'number' ? (
                      <p style={{ margin: '0.25rem 0 0', fontSize: '0.9rem' }}>
                        {(item.amount_minor / 100).toFixed(2)} {item.currency ?? ''}
                      </p>
                    ) : null}
                    <div style={rationaleStyle} aria-label="Routing rationale">
                      <strong style={{ display: 'block', marginBottom: '0.25rem' }}>
                        Why you
                      </strong>
                      {item.routing_snapshot?.rationale || 'Routed by policy'}
                      <div style={{ marginTop: '0.35rem', fontSize: '0.8rem' }}>
                        Policy {item.routing_snapshot?.policy_name} · step {item.current_step} ·{' '}
                        {item.routing_snapshot?.mode}
                      </div>
                    </div>
                    <div style={{ ...actionsStyle, marginTop: '0.5rem' }}>
                      <Button
                        type="button"
                        disabled={busy === item.id}
                        onClick={() => void decide(item.id, true)}
                      >
                        Approve
                      </Button>
                      <Button
                        type="button"
                        variant="secondary"
                        disabled={busy === item.id}
                        onClick={() => void decide(item.id, false)}
                      >
                        Reject
                      </Button>
                    </div>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}
