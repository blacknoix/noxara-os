'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatTile,
  Widget,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type StageSummary = {
  stage_id: string;
  stage_name: string;
  open_deal_count: number;
  open_amount_minor: number;
  currency: string;
};

type ReportSummary = {
  pipeline_by_stage: StageSummary[];
  win_rate: { won_count: number; lost_count: number; win_rate_pct: number };
  activity_volume: { kind: string; count: number }[];
  weighted_forecast: { amount_minor: number; currency: string };
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; data: ReportSummary };

export default function SalesReportsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    try {
      const res = await authFetch('/api/v1/sales/reports/summary');
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
        let message = 'Could not load sales reports.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setState({ status: 'error', message, requestId });
        return;
      }
      const data = (await res.json()) as ReportSummary;
      setState({ status: 'ready', data });
    } catch {
      setState({ status: 'error', message: 'Sales report request failed.' });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (!can('sales.report.read')) {
    return <PermissionDeniedState requiredPermission="sales.report.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={h1}>Reports</h1>
        <p style={muted}>Honest aggregations computed directly from your CRM data — no fancy charts.</p>
      </header>

      {state.status === 'loading' ? (
        <LoadingState label="Loading sales reports" rows={5} />
      ) : state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Open /login to view sales reports." />
      ) : state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="sales.report.read" />
      ) : state.status === 'error' ? (
        <ErrorState message={state.message} requestId={state.requestId} />
      ) : (
        <div
          style={{
            display: 'grid',
            gap: '1.25rem',
            gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
          }}
        >
          <div style={widgetFrame}>
            <Widget title="Pipeline by stage">
              {state.data.pipeline_by_stage.length === 0 ? (
                <EmptyState title="No open deals" description="Open deals grouped by stage appear here." />
              ) : (
                <div style={{ display: 'grid', gap: 10 }}>
                  {state.data.pipeline_by_stage.map((s) => (
                    <div
                      key={s.stage_id}
                      style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.9rem' }}
                    >
                      <span>{s.stage_name}</span>
                      <span style={{ display: 'flex', gap: 10, alignItems: 'baseline' }}>
                        <span style={{ color: 'var(--cos-color-fg-muted)', fontVariantNumeric: 'tabular-nums' }}>
                          {s.open_deal_count} deal{s.open_deal_count === 1 ? '' : 's'}
                        </span>
                        <MoneyCell amount={s.open_amount_minor / 100} currency={s.currency} />
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </Widget>
          </div>

          <div style={widgetFrame}>
            <Widget title="Win rate">
              <div style={{ display: 'flex', gap: '1.5rem', flexWrap: 'wrap' }}>
                <StatTile label="Win rate" value={`${state.data.win_rate.win_rate_pct.toFixed(1)}%`} />
                <StatTile label="Won" value={state.data.win_rate.won_count} />
                <StatTile label="Lost" value={state.data.win_rate.lost_count} />
              </div>
            </Widget>
          </div>

          <div style={widgetFrame}>
            <Widget title="Weighted forecast">
              <StatTile
                label={`Open pipeline (${state.data.weighted_forecast.currency})`}
                value={
                  <MoneyCell
                    amount={state.data.weighted_forecast.amount_minor / 100}
                    currency={state.data.weighted_forecast.currency}
                  />
                }
                hint="Sum of open deal amounts weighted by stage probability. USD-only in this phase."
              />
            </Widget>
          </div>

          <div style={widgetFrame}>
            <Widget title="Activity volume">
              {state.data.activity_volume.length === 0 ? (
                <EmptyState title="No activity yet" description="Calls, meetings, emails, and notes appear here." />
              ) : (
                <div style={{ display: 'flex', gap: '1.5rem', flexWrap: 'wrap' }}>
                  {state.data.activity_volume.map((a) => (
                    <StatTile key={a.kind} label={a.kind} value={a.count} />
                  ))}
                </div>
              )}
            </Widget>
          </div>
        </div>
      )}
    </div>
  );
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
  maxWidth: 560,
};

const widgetFrame: CSSProperties = {
  padding: '1rem 1.1rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg-elevated)',
  minHeight: 160,
};
