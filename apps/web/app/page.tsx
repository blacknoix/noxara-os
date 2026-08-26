'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Select,
  Widget,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../lib/auth-client';
import { fetchInsights } from '../lib/ai-api';
import { citationHref, type InsightObservation } from '../lib/ai-types';

type ChecklistItem = {
  id: string;
  label: string;
  done: boolean;
  member_count?: number;
};

type PipelineStageSummary = {
  stage_id: string;
  stage_name: string;
  open_deal_count: number;
  open_amount_minor: number;
  currency: string;
};

type DashboardWidget = {
  id: string;
  title: string;
  kind: string;
  status: string;
  reason_code?: string | null;
  stale: boolean;
  range_label?: string | null;
  payload: Record<string, unknown>;
};

type DashboardResponse = {
  as_of: string;
  period: string;
  role_layout: string;
  widgets: DashboardWidget[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; data: DashboardResponse };

const PERIODS = [
  { value: '7d', label: 'Last 7 days' },
  { value: '30d', label: 'Last 30 days' },
  { value: '90d', label: 'Last 90 days' },
];

export default function DashboardPage() {
  const [period, setPeriod] = useState('30d');
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [insights, setInsights] = useState<{
    status: 'idle' | 'loading' | 'ready' | 'empty' | 'denied';
    observations: InsightObservation[];
    emptyReason?: string;
  }>({ status: 'idle', observations: [] });

  const load = useCallback(async (selectedPeriod: string) => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    try {
      const res = await authFetch(`/api/v1/dashboard?period=${encodeURIComponent(selectedPeriod)}`);
      const requestId = res.headers.get('x-request-id') ?? undefined;
      if (res.status === 401) {
        setState({ status: 'signed_out' });
        return;
      }
      if (res.status === 403) {
        setState({ status: 'denied' });
        return;
      }
      if (!res.ok) {
        let message = 'Could not load the dashboard.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setState({ status: 'error', message, requestId });
        return;
      }
      const data = (await res.json()) as DashboardResponse;
      setState({ status: 'ready', data });
    } catch {
      setState({ status: 'error', message: 'Dashboard request failed.' });
    }
  }, []);

  const loadInsights = useCallback(async () => {
    if (!getAccessToken()) {
      setInsights({ status: 'idle', observations: [] });
      return;
    }
    setInsights((prev) => ({ ...prev, status: 'loading' }));
    const data = await fetchInsights();
    if (!data) {
      setInsights({ status: 'empty', observations: [], emptyReason: 'Could not load insights' });
      return;
    }
    if (data.empty_reason) {
      setInsights({
        status: data.empty_reason.includes('denied') ? 'denied' : 'empty',
        observations: [],
        emptyReason: data.empty_reason,
      });
      return;
    }
    setInsights({ status: 'ready', observations: data.observations.slice(0, 5) });
  }, []);

  useEffect(() => {
    void load(period);
  }, [load, period]);

  useEffect(() => {
    void loadInsights();
  }, [loadInsights]);

  return (
    <section>
      <header
        style={{
          marginBottom: '1.25rem',
          display: 'flex',
          flexWrap: 'wrap',
          gap: '1rem',
          alignItems: 'flex-end',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <p style={eyebrow}>Work</p>
          <h1 style={h1}>Dashboard</h1>
          <p style={muted}>
            {state.status === 'ready'
              ? `Layout: ${state.data.role_layout}`
              : 'Org-scoped home — no invented metrics.'}
          </p>
        </div>
        <div style={{ minWidth: 180 }}>
          <Select
            label="Period"
            value={period}
            onChange={(e) => setPeriod(e.target.value)}
            options={PERIODS}
          />
        </div>
      </header>

      {state.status === 'loading' ? (
        <div
          style={{
            display: 'grid',
            gap: '1.25rem',
            gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))',
          }}
        >
          {[0, 1, 2, 3].map((i) => (
            <div key={i} style={widgetFrame}>
              <Widget title="Loading…" loading />
            </div>
          ))}
          <LoadingState label="Loading dashboard" rows={3} />
        </div>
      ) : null}

      {state.status === 'signed_out' ? (
        <ErrorState
          title="Sign in required"
          message="Open login to load your workspace dashboard."
        />
      ) : null}

      {state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="workspace.dashboard.read" />
      ) : null}

      {state.status === 'error' ? (
        <ErrorState message={state.message} requestId={state.requestId} />
      ) : null}

      {insights.status !== 'idle' ? (
        <div style={{ ...widgetFrame, marginBottom: '1.25rem' }}>
          <AiInsightsWidget
            status={insights.status === 'loading' ? 'loading' : insights.status}
            observations={insights.observations}
            emptyReason={insights.emptyReason}
            onRefresh={() => void loadInsights()}
          />
        </div>
      ) : null}

      {state.status === 'ready' ? (
        <div
          style={{
            display: 'grid',
            gap: '1.25rem',
            gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))',
          }}
        >
          {state.data.widgets.map((w) => (
            <div key={w.id} style={widgetFrame}>
              <DashboardWidgetCard widget={w} asOf={state.data.as_of} onRefresh={() => void load(period)} />
            </div>
          ))}
        </div>
      ) : null}
    </section>
  );
}

function AiInsightsWidget({
  status,
  observations,
  emptyReason,
  onRefresh,
}: {
  status: 'loading' | 'ready' | 'empty' | 'denied';
  observations: InsightObservation[];
  emptyReason?: string;
  onRefresh: () => void;
}) {
  if (status === 'loading') {
    return <Widget title="AI Insights" loading />;
  }

  if (status === 'denied') {
    return (
      <Widget
        title="AI Insights"
        empty={
          <EmptyState
            title="Insights unavailable"
            description="You need ai.insights.read to view observations."
          />
        }
      />
    );
  }

  if (status === 'empty' || observations.length === 0) {
    return (
      <Widget
        title="AI Insights"
        menu={
          <button type="button" onClick={onRefresh} style={linkBtn}>
            Refresh
          </button>
        }
        empty={
          <EmptyState
            title="No insights yet"
            description={emptyReason ?? 'When the insights module finds patterns, they appear here.'}
          />
        }
      />
    );
  }

  return (
    <Widget
      title="AI Insights"
      menu={
        <button type="button" onClick={onRefresh} style={linkBtn}>
          Refresh
        </button>
      }
    >
      <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: 12 }}>
        {observations.map((obs) => (
          <li
            key={obs.id}
            style={{
              padding: '0.65rem 0.75rem',
              border: '1px solid var(--cos-color-border)',
              borderRadius: 'var(--cos-radius-sm)',
              background: 'var(--cos-color-bg)',
            }}
          >
            <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
              <strong style={{ fontSize: '0.95rem' }}>{obs.title}</strong>
              {obs.estimate ? <Badge tone="neutral">Estimate</Badge> : null}
            </div>
            <p style={{ margin: '0.35rem 0', fontSize: '0.88rem', color: 'var(--cos-color-fg-muted)' }}>
              {obs.body}
            </p>
            {obs.evidence.length > 0 ? (
              <ul style={{ listStyle: 'none', margin: '0.35rem 0 0', padding: 0, fontSize: '0.8rem' }}>
                {obs.evidence.map((e) => (
                  <li key={`${e.record_type}-${e.record_id}`}>
                    <Link href={citationHref(e)} style={linkBtn}>
                      {e.title}
                    </Link>
                  </li>
                ))}
              </ul>
            ) : null}
            {obs.suggested_action ? (
              <p style={{ margin: '0.5rem 0 0', fontSize: '0.8rem', color: 'var(--cos-color-fg-muted)' }}>
                Suggested: <code>{obs.suggested_action}</code>
              </p>
            ) : null}
          </li>
        ))}
      </ul>
    </Widget>
  );
}

function DashboardWidgetCard({
  widget,
  asOf,
  onRefresh,
}: {
  widget: DashboardWidget;
  asOf: string;
  onRefresh: () => void;
}) {
  const menu = widget.stale ? <Badge tone="warning">Stale</Badge> : <Badge tone="neutral">Live</Badge>;
  const range = widget.range_label ?? undefined;
  const footer = (
    <span>
      As of <time dateTime={asOf}>{formatAsOf(asOf)}</time>
      {widget.stale ? (
        <>
          {' · '}
          <button type="button" onClick={onRefresh} style={linkBtn}>
            Refresh
          </button>
        </>
      ) : null}
    </span>
  );

  if (widget.kind === 'checklist') {
    const items = (widget.payload.items as ChecklistItem[] | undefined) ?? [];
    return (
      <Widget title={widget.title} range={range} menu={menu} footer={footer}>
        <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: 8 }}>
          {items.map((item) => (
            <li
              key={item.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                fontSize: '0.9rem',
                color: 'var(--cos-color-fg)',
              }}
            >
              <span aria-hidden>{item.done ? '✓' : '○'}</span>
              <span>
                {item.label}
                {typeof item.member_count === 'number' ? (
                  <span style={{ color: 'var(--cos-color-fg-muted)' }}> ({item.member_count})</span>
                ) : null}
              </span>
            </li>
          ))}
        </ul>
        {!items.some((i) => i.id === 'members' && i.done) ? (
          <div style={{ marginTop: 12 }}>
            <Link href="/members">
              <Button size="sm" variant="secondary">
                Invite teammates
              </Button>
            </Link>
          </div>
        ) : null}
      </Widget>
    );
  }

  if (widget.kind === 'pipeline' && widget.status === 'ready') {
    const pipelineByStage =
      (widget.payload.pipeline_by_stage as PipelineStageSummary[] | undefined) ?? [];
    return (
      <Widget title={widget.title} range={range} menu={menu} footer={footer}>
        <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: 8 }}>
          {pipelineByStage.map((s) => (
            <li
              key={s.stage_id}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'baseline',
                gap: 8,
                fontSize: '0.9rem',
                color: 'var(--cos-color-fg)',
              }}
            >
              <span>{s.stage_name}</span>
              <span style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
                <span style={{ color: 'var(--cos-color-fg-muted)', fontVariantNumeric: 'tabular-nums' }}>
                  {s.open_deal_count}
                </span>
                <MoneyCell amount={s.open_amount_minor / 100} currency={s.currency} />
              </span>
            </li>
          ))}
        </ul>
        <div style={{ marginTop: 12 }}>
          <Link href="/sales">
            <Button size="sm" variant="secondary">
              View pipeline
            </Button>
          </Link>
        </div>
      </Widget>
    );
  }

  if (widget.id === 'approvals') {
    const count = typeof widget.payload.count === 'number' ? widget.payload.count : 0;
    if (widget.status === 'ready' || widget.status === 'empty') {
      return (
        <Widget title={widget.title} range={range} menu={menu} footer={footer}>
          <p style={{ margin: 0, fontSize: '1.5rem', fontVariantNumeric: 'tabular-nums' }}>{count}</p>
          <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' }}>
            Pending for you
          </p>
          <div style={{ marginTop: 12 }}>
            <Link href="/approvals">
              <Button size="sm" variant="secondary">
                Open inbox
              </Button>
            </Link>
          </div>
        </Widget>
      );
    }
  }

  if (widget.id === 'my_work' && (widget.status === 'ready' || widget.status === 'empty')) {
    const count = typeof widget.payload.count === 'number' ? widget.payload.count : 0;
    return (
      <Widget title={widget.title} range={range} menu={menu} footer={footer}>
        <p style={{ margin: 0, fontSize: '1.5rem', fontVariantNumeric: 'tabular-nums' }}>{count}</p>
        <div style={{ marginTop: 12 }}>
          <Link href="/my-work">
            <Button size="sm" variant="secondary">
              Open My work
            </Button>
          </Link>
        </div>
      </Widget>
    );
  }

  const emptyCopy = reasonCopy(widget.reason_code, widget.payload);
  return (
    <Widget
      title={widget.title}
      range={range}
      menu={menu}
      footer={footer}
      empty={
        <EmptyState
          title={emptyCopy.title}
          description={emptyCopy.description}
          action={
            <Button size="sm" variant="secondary" disabled>
              Coming later
            </Button>
          }
        />
      }
    />
  );
}

function reasonCopy(
  reason: string | null | undefined,
  payload: Record<string, unknown>,
): { title: string; description: string } {
  const message = typeof payload.message === 'string' ? payload.message : undefined;
  switch (reason) {
    case 'module_not_enabled':
      return {
        title: 'Module not enabled',
        description: message ?? 'This module is not available in your workspace yet.',
      };
    case 'coming_in_later_phase':
      return {
        title: 'Coming later',
        description: message ?? 'This surface ships in a later phase. No placeholder records.',
      };
    case 'no_data':
      return {
        title: 'Nothing here yet',
        description: message ?? 'When activity exists, it will show up here.',
      };
    default:
      return {
        title: 'Empty',
        description: message ?? 'No data for this widget.',
      };
  }
}

function formatAsOf(asOf: string) {
  const d = new Date(asOf);
  if (Number.isNaN(d.getTime())) return asOf;
  return d.toLocaleString();
}

const eyebrow: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const h1: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: 'clamp(1.75rem, 2.5vw, 2.25rem)',
  fontWeight: 650,
  letterSpacing: '-0.02em',
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 520,
};

const widgetFrame: CSSProperties = {
  padding: '1rem 1.1rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg-elevated)',
  minHeight: 180,
};

const linkBtn: CSSProperties = {
  all: 'unset',
  cursor: 'pointer',
  color: 'var(--cos-color-accent)',
  textDecoration: 'underline',
  textUnderlineOffset: 2,
};
