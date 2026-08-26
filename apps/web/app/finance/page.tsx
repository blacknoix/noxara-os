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
  Widget,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';

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

type ReportSummary = {
  as_of: string;
  currency: string;
  revenue_minor: number;
  expenses_minor: number;
  cash_minor: number;
  receivables_minor: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; report: ReportSummary | null; invoices: Invoice[] };

export default function FinancePage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    try {
      const [reportRes, invRes] = await Promise.all([
        authFetch('/api/v1/finance/reports/summary'),
        authFetch('/api/v1/finance/invoices?limit=20'),
      ]);
      if (reportRes.status === 401 || invRes.status === 401) {
        setState({ status: 'signed_out' });
        return;
      }
      if (reportRes.status === 403 && invRes.status === 403) {
        setState({ status: 'denied' });
        return;
      }
      const report =
        reportRes.ok ? ((await reportRes.json()) as ReportSummary) : null;
      const invoices = invRes.ok
        ? (((await invRes.json()) as { items: Invoice[] }).items ?? [])
        : [];
      setState({ status: 'ready', report, invoices });
    } catch {
      setState({ status: 'error', message: 'Could not load finance data.' });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading || state.status === 'loading') {
    return <LoadingState label="Loading finance…" />;
  }
  if (state.status === 'signed_out') {
    return (
      <EmptyState
        title="Sign in required"
        description="Sign in to view finance."
        action={
          <Link href="/login" style={{ textDecoration: 'none' }}><Button type="button" variant="primary">Sign in</Button></Link>
        }
      />
    );
  }
  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="finance.invoice.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Finance unavailable" message={state.message} />;
  }

  const { report, invoices } = state;
  const currency = report?.currency ?? 'USD';

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Finance</h1>
          <p style={subtitle}>
            Invoices, payments, and ledger aggregates from the finance service.
          </p>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          {can('finance.invoice.create') ? (
            <Link href="/finance/invoices" style={{ textDecoration: 'none' }}><Button type="button" variant="primary">Invoices</Button></Link>
          ) : null}
          {can('finance.expense.create') ? (
            <Link href="/finance/expenses" style={{ textDecoration: 'none' }}><Button type="button" variant="secondary">Expenses</Button></Link>
          ) : null}
          {can('finance.report.read') ? (
            <Link href="/finance/reports" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Reports</Button></Link>
          ) : null}
        </div>
      </header>

      {report ? (
        <div style={statGrid}>
          <Widget title="Revenue" footer={report.as_of ? `As of ${report.as_of}` : undefined}>
            <MoneyCell amount={report.revenue_minor / 100} currency={currency} />
          </Widget>
          <Widget title="Expenses" footer={report.as_of ? `As of ${report.as_of}` : undefined}>
            <MoneyCell amount={report.expenses_minor / 100} currency={currency} />
          </Widget>
          <Widget title="Cash" footer={report.as_of ? `As of ${report.as_of}` : undefined}>
            <MoneyCell amount={report.cash_minor / 100} currency={currency} />
          </Widget>
          <Widget title="Receivables" footer={report.as_of ? `As of ${report.as_of}` : undefined}>
            <MoneyCell amount={report.receivables_minor / 100} currency={currency} />
          </Widget>
        </div>
      ) : (
        <EmptyState
          title="Reports unavailable"
          description="You may not have finance.report.read, or the finance service is down."
        />
      )}

      <div style={{ marginTop: '1.5rem' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
          <h2 style={{ fontSize: '1.05rem', margin: 0 }}>Recent invoices</h2>
          <Link href="/finance/invoices" style={{ fontSize: '0.875rem' }}>
            View all
          </Link>
        </div>
        {invoices.length === 0 ? (
          <EmptyState
            title="No invoices yet"
            description="Create an invoice or convert an accepted quote."
          />
        ) : (
          <Table
            getRowKey={(r: Invoice) => r.id}
            columns={[
              {
                key: 'number',
                header: 'Number',
                cell: (r: Invoice) => (
                  <Link href={`/finance/invoices/${r.id}`}>{r.invoice_number ?? r.id}</Link>
                ),
              },
              {
                key: 'status',
                header: 'Status',
                cell: (r: Invoice) => <Badge>{r.status}</Badge>,
              },
              {
                key: 'total',
                header: 'Total',
                align: 'right',
                cell: (r: Invoice) => (
                  <MoneyCell amount={r.total_minor / 100} currency={r.currency} />
                ),
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
            rows={invoices}
          />
        )}
      </div>
    </section>
  );
}

const headerStyle: CSSProperties = {
  marginBottom: '1.25rem',
  display: 'flex',
  flexWrap: 'wrap',
  gap: '1rem',
  alignItems: 'flex-end',
  justifyContent: 'space-between',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const subtitle: CSSProperties = {
  margin: 0,
  color: 'var(--cos-color-fg-muted)',
  maxWidth: '36rem',
};
const statGrid: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(11rem, 1fr))',
  gap: '0.75rem',
};
