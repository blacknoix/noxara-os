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
import { useCapabilities } from '../../../lib/capabilities';
import { AiSuggestionChips } from '../../../components/AiSuggestionChips';

type Invoice = {
  id: string;
  invoice_number: string | null;
  status: string;
  customer_id: string;
  currency: string;
  total_minor: number;
  balance_minor: number;
  issue_date: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Invoice[] };

export default function FinanceInvoicesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/invoices?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load invoices.' });
      return;
    }
    const body = (await res.json()) as { items: Invoice[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading || state.status === 'loading') return <LoadingState label="Loading invoices…" />;
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
  if (state.status === 'denied') return <PermissionDeniedState requiredPermission="finance.invoice.read" />;
  if (state.status === 'error') {
    return <ErrorState title="Invoices unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Invoices</h1>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Back</Button></Link>
      </header>
      <AiSuggestionChips pageScope="invoice" />
      {!can('finance.invoice.read') ? (
        <PermissionDeniedState requiredPermission="finance.invoice.read" />
      ) : state.items.length === 0 ? (
        <EmptyState title="No invoices" description="Issued and draft invoices appear here." />
      ) : (
        <Table
          getRowKey={(r: Invoice) => r.id}
          columns={[
            {
              key: 'number',
              header: 'Number',
              cell: (r: Invoice) => (
                <Link href={`/finance/invoices/${r.id}`}>{r.invoice_number ?? 'Draft'}</Link>
              ),
            },
            { key: 'status', header: 'Status', cell: (r: Invoice) => <Badge>{r.status}</Badge> },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (r: Invoice) => <MoneyCell amount={r.total_minor / 100} currency={r.currency} />,
            },
            {
              key: 'balance',
              header: 'Balance',
              align: 'right',
              cell: (r: Invoice) => (
                <MoneyCell amount={r.balance_minor / 100} currency={r.currency} />
              ),
            },
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
