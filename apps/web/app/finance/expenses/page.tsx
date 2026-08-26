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
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type Expense = {
  id: string;
  status: string;
  currency: string;
  amount_minor: number;
  description: string;
  incurred_at: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Expense[] };

export default function ExpensesPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/expenses?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load expenses.' });
      return;
    }
    const body = (await res.json()) as { items: Expense[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (state.status === 'loading') return <LoadingState label="Loading expenses…" />;
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
  if (state.status === 'denied') return <PermissionDeniedState requiredPermission="finance.expense.read" />;
  if (state.status === 'error') {
    return <ErrorState title="Expenses unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Expenses</h1>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Back</Button></Link>
      </header>
      {state.items.length === 0 ? (
        <EmptyState title="No expenses" description="Submitted expenses appear here." />
      ) : (
        <Table
          getRowKey={(r: Expense) => r.id}
          columns={[
            { key: 'description', header: 'Description', cell: (r: Expense) => r.description },
            { key: 'status', header: 'Status', cell: (r: Expense) => <Badge>{r.status}</Badge> },
            {
              key: 'amount',
              header: 'Amount',
              align: 'right',
              cell: (r: Expense) => (
                <MoneyCell amount={r.amount_minor / 100} currency={r.currency} />
              ),
            },
            { key: 'date', header: 'Incurred', cell: (r: Expense) => r.incurred_at },
          ]}
          rows={state.items}
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
