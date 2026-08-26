'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Table,
  Widget,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type AgeingBucket = { label: string; amount_minor: number };
type CategoryAmount = { category: string; amount_minor: number };
type CashFlowPoint = { period: string; inflow_minor: number; outflow_minor: number };

type ReportSummary = {
  as_of: string;
  currency: string;
  revenue_minor: number;
  expenses_minor: number;
  cash_minor: number;
  receivables_minor: number;
  ageing: AgeingBucket[];
  expenses_by_category: CategoryAmount[];
  cash_flow: CashFlowPoint[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; report: ReportSummary };

export default function FinanceReportsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/reports/summary');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load report.' });
      return;
    }
    setState({ status: 'ready', report: (await res.json()) as ReportSummary });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (state.status === 'loading') return <LoadingState label="Loading reports…" />;
  if (state.status === 'signed_out') {
    return (
      <EmptyState
        title="Sign in required"
        action={
          <Link href="/login" style={{ textDecoration: 'none' }}><Button type="button" variant="primary">Sign in</Button></Link>
        }
      />
    );
  }
  if (state.status === 'denied') return <PermissionDeniedState requiredPermission="finance.report.read" />;
  if (state.status === 'error') {
    return <ErrorState title="Reports unavailable" message={state.message} />;
  }

  const r = state.report;
  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Reports</h1>
          <p style={subtitle}>As of {r.as_of}</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Back</Button></Link>
      </header>

      <div style={statGrid}>
        <Widget title="Revenue">
          <MoneyCell amount={r.revenue_minor / 100} currency={r.currency} />
        </Widget>
        <Widget title="Expenses">
          <MoneyCell amount={r.expenses_minor / 100} currency={r.currency} />
        </Widget>
        <Widget title="Cash">
          <MoneyCell amount={r.cash_minor / 100} currency={r.currency} />
        </Widget>
        <Widget title="Receivables">
          <MoneyCell amount={r.receivables_minor / 100} currency={r.currency} />
        </Widget>
      </div>

      <h2 style={h2}>Receivables ageing</h2>
      {(r.ageing ?? []).length === 0 ? (
        <EmptyState title="No open receivables" />
      ) : (
        <Table
          getRowKey={(b: AgeingBucket) => b.label}
          columns={[
            { key: 'label', header: 'Bucket', cell: (b: AgeingBucket) => b.label },
            {
              key: 'amount',
              header: 'Amount',
              align: 'right',
              cell: (b: AgeingBucket) => (
                <MoneyCell amount={b.amount_minor / 100} currency={r.currency} />
              ),
            },
          ]}
          rows={r.ageing}
        />
      )}

      <h2 style={h2}>Expenses by category</h2>
      {(r.expenses_by_category ?? []).length === 0 ? (
        <EmptyState title="No posted expenses" />
      ) : (
        <Table
          getRowKey={(c: CategoryAmount) => c.category}
          columns={[
            { key: 'cat', header: 'Category', cell: (c: CategoryAmount) => c.category },
            {
              key: 'amount',
              header: 'Amount',
              align: 'right',
              cell: (c: CategoryAmount) => (
                <MoneyCell amount={c.amount_minor / 100} currency={r.currency} />
              ),
            },
          ]}
          rows={r.expenses_by_category}
        />
      )}

      <h2 style={h2}>Cash flow (simple)</h2>
      {(r.cash_flow ?? []).length === 0 ? (
        <EmptyState title="No cash movements yet" />
      ) : (
        <Table
          getRowKey={(p: CashFlowPoint) => p.period}
          columns={[
            { key: 'period', header: 'Period', cell: (p: CashFlowPoint) => p.period },
            {
              key: 'in',
              header: 'Inflow',
              align: 'right',
              cell: (p: CashFlowPoint) => (
                <MoneyCell amount={p.inflow_minor / 100} currency={r.currency} />
              ),
            },
            {
              key: 'out',
              header: 'Outflow',
              align: 'right',
              cell: (p: CashFlowPoint) => (
                <MoneyCell amount={p.outflow_minor / 100} currency={r.currency} />
              ),
            },
          ]}
          rows={r.cash_flow}
        />
      )}
    </section>
  );
}

const headerStyle: CSSProperties = {
  marginBottom: '1.25rem',
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-end',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const subtitle: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const h2: CSSProperties = { fontSize: '1.05rem', margin: '1.5rem 0 0.75rem' };
const statGrid: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(11rem, 1fr))',
  gap: '0.75rem',
};
