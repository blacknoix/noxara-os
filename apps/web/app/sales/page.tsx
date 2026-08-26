'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  EmptyState,
  ErrorState,
  KanbanBoard,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  useToast,
  type KanbanColumn,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';
import { AiSuggestionChips } from '../../components/AiSuggestionChips';

type StageDto = {
  id: string;
  pipeline_id: string;
  name: string;
  position: number;
  probability: number;
  is_won: boolean;
  is_lost: boolean;
};

type DealDto = {
  id: string;
  pipeline_id: string;
  stage_id: string;
  customer_id: string | null;
  lead_id: string | null;
  name: string;
  amount_minor: number;
  currency: string;
  probability: number | null;
  expected_close_date: string | null;
  owner_user_id: string | null;
  status: string;
  won_reason: string | null;
  lost_reason: string | null;
  won_at: string | null;
  lost_at: string | null;
  created_at: string;
  updated_at: string;
  version: number;
};

type BoardStage = { stage: StageDto; deals: DealDto[] };

type BoardResponse = {
  pipeline: { id: string; name: string; is_default: boolean };
  stages: BoardStage[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; data: BoardResponse };

const SALES_NAV = [
  { href: '/sales/deals', label: 'Deals', perm: 'sales.deal.read' },
  { href: '/sales/leads', label: 'Leads', perm: 'sales.lead.read' },
  { href: '/sales/customers', label: 'Customers', perm: 'sales.customer.read' },
  { href: '/sales/quotes', label: 'Quotes', perm: 'sales.quote.read' },
  { href: '/sales/reports', label: 'Reports', perm: 'sales.report.read' },
];

export default function SalesPipelinePage() {
  const router = useRouter();
  const { can, loading: capsLoading } = useCapabilities();
  const { toast } = useToast();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    try {
      const res = await authFetch('/api/v1/sales/pipelines/default/board');
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
        let message = 'Could not load the pipeline board.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setState({ status: 'error', message, requestId });
        return;
      }
      const data = (await res.json()) as BoardResponse;
      setState({ status: 'ready', data });
    } catch {
      setState({ status: 'error', message: 'Pipeline board request failed.' });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const moveDeal = useCallback(
    async (cardId: string, fromColumnId: string, toColumnId: string) => {
      if (fromColumnId === toColumnId || state.status !== 'ready') return;

      const previousSnapshot = state.data;
      let dealBeingMoved: DealDto | undefined;
      const stagesWithoutCard = previousSnapshot.stages.map((s) => {
        if (s.stage.id !== fromColumnId) return s;
        const deal = s.deals.find((d) => d.id === cardId);
        if (deal) dealBeingMoved = deal;
        return { ...s, deals: s.deals.filter((d) => d.id !== cardId) };
      });
      if (!dealBeingMoved) return;

      const updatedDeal = { ...dealBeingMoved, stage_id: toColumnId };
      const optimisticStages = stagesWithoutCard.map((s) =>
        s.stage.id === toColumnId ? { ...s, deals: [updatedDeal, ...s.deals] } : s,
      );
      setState({ status: 'ready', data: { ...previousSnapshot, stages: optimisticStages } });

      try {
        const res = await authFetch(`/api/v1/sales/deals/${cardId}`, {
          method: 'PATCH',
          headers: { 'If-Match': String(dealBeingMoved.version) },
          body: JSON.stringify({ stage_id: toColumnId }),
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          const message = typeof body.detail === 'string' ? body.detail : 'Could not move deal';
          throw new Error(message);
        }
        const updated = (await res.json()) as DealDto;
        setState((current) => {
          if (current.status !== 'ready') return current;
          return {
            status: 'ready',
            data: {
              ...current.data,
              stages: current.data.stages.map((s) => ({
                ...s,
                deals: s.deals.map((d) => (d.id === updated.id ? updated : d)),
              })),
            },
          };
        });
      } catch (err) {
        setState({ status: 'ready', data: previousSnapshot });
        toast({
          title: 'Could not move deal',
          description: err instanceof Error ? err.message : 'Please try again.',
        });
      }
    },
    [state, toast],
  );

  const selectDeal = useCallback(
    (cardId: string, columnId: string) => {
      if (state.status !== 'ready') return;
      const stage = state.data.stages.find((s) => s.stage.id === columnId);
      const deal = stage?.deals.find((d) => d.id === cardId);
      if (deal?.customer_id && can('sales.customer.read')) {
        router.push(`/sales/customers/${deal.customer_id}`);
        return;
      }
      router.push(`/sales/deals?q=${encodeURIComponent(deal?.name ?? '')}`);
    },
    [state, can, router],
  );

  const canReadDeals = can('sales.deal.read');
  const canReadPipeline = can('sales.pipeline.read');

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '1rem',
          alignItems: 'flex-end',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <p style={eyebrow}>Sales</p>
          <h1 style={h1}>Pipeline</h1>
          <p style={muted}>
            {state.status === 'ready'
              ? `${state.data.pipeline.name} — drag deals between stages to update.`
              : 'Your default sales pipeline.'}
          </p>
        </div>
        <nav aria-label="Sales" style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          {SALES_NAV.filter((item) => capsLoading || can(item.perm)).map((item) => (
            <Link key={item.href} href={item.href} style={navLink}>
              {item.label}
            </Link>
          ))}
        </nav>
      </header>

      <AiSuggestionChips pageScope="deal" />

      {capsLoading ? (
        <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>
      ) : !canReadPipeline || !canReadDeals ? (
        <PermissionDeniedState requiredPermission="sales.deal.read" />
      ) : state.status === 'loading' ? (
        <LoadingState label="Loading pipeline board" rows={5} />
      ) : state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Open /login to view the sales pipeline." />
      ) : state.status === 'denied' ? (
        <PermissionDeniedState requiredPermission="sales.deal.read" />
      ) : state.status === 'error' ? (
        <ErrorState message={state.message} requestId={state.requestId} />
      ) : state.data.stages.every((s) => s.deals.length === 0) ? (
        <EmptyState
          title="No open deals yet"
          description="Deals will appear here as they're created. Convert a lead or create a deal to get started."
          action={
            can('sales.lead.read') ? (
              <Link href="/sales/leads" style={{ color: 'var(--cos-color-accent)' }}>
                Go to leads
              </Link>
            ) : undefined
          }
        />
      ) : (
        <KanbanBoard
          columns={state.data.stages.map(
            (s): KanbanColumn => ({
              id: s.stage.id,
              title: s.stage.name,
              cards: s.deals.map((d) => ({
                id: d.id,
                title: d.name,
                meta: <MoneyCell amount={d.amount_minor / 100} currency={d.currency} />,
              })),
            }),
          )}
          onCardSelect={selectDeal}
          onCardMove={moveDeal}
        />
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

const navLink: CSSProperties = {
  padding: '0.4rem 0.75rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  color: 'var(--cos-color-fg)',
  fontSize: '0.875rem',
  fontWeight: 550,
  textDecoration: 'none',
};
