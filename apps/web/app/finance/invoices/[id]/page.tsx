'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Table,
  useToast,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type Line = {
  id: string;
  description: string;
  quantity: number;
  unit_price_minor: number;
  tax_minor: number;
  line_total_minor: number;
};

type Invoice = {
  id: string;
  invoice_number: string | null;
  status: string;
  customer_id: string;
  currency: string;
  subtotal_minor: number;
  tax_minor: number;
  total_minor: number;
  balance_minor: number;
  amount_paid_minor: number;
  payment_url: string | null;
  version: number;
  lines: Line[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; invoice: Invoice };

export default function InvoiceDetailPage() {
  const params = useParams<{ id: string }>();
  const { can, loading: capsLoading } = useCapabilities();
  const { toast } = useToast();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch(`/api/v1/finance/invoices/${params.id}`);
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load invoice.' });
      return;
    }
    setState({ status: 'ready', invoice: (await res.json()) as Invoice });
  }, [params.id]);

  useEffect(() => {
    void load();
  }, [load]);

  async function issue() {
    if (state.status !== 'ready') return;
    setBusy(true);
    const res = await authFetch(`/api/v1/finance/invoices/${state.invoice.id}/issue`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'idempotency-key': `issue-${state.invoice.id}-${Date.now()}`,
      },
      body: JSON.stringify({ fx_rate_num: 1, fx_rate_den: 1 }),
    });
    setBusy(false);
    if (!res.ok) {
      toast({ title: 'Issue failed' });
      return;
    }
    toast({ title: 'Invoice issued' });
    void load();
  }

  async function send() {
    if (state.status !== 'ready') return;
    setBusy(true);
    const res = await authFetch(`/api/v1/finance/invoices/${state.invoice.id}/send`, {
      method: 'POST',
    });
    setBusy(false);
    if (!res.ok) {
      toast({ title: 'Send failed' });
      return;
    }
    const body = (await res.json()) as Invoice;
    if (body.payment_url) {
      // Local: payment link is logged / returned; PDF/email are cut.
      console.info('Payment URL', body.payment_url);
      toast({ title: 'Sent (payment link logged)' });
    } else {
      toast({ title: 'Marked sent' });
    }
    void load();
  }

  if (capsLoading || state.status === 'loading') return <LoadingState label="Loading invoice…" />;
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
    return <ErrorState title="Invoice unavailable" message={state.message} />;
  }

  const inv = state.invoice;

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>{inv.invoice_number ?? 'Draft invoice'}</h1>
          <p style={subtitle}>
            <Badge>{inv.status}</Badge> · Balance{' '}
            <MoneyCell amount={inv.balance_minor / 100} currency={inv.currency} />
          </p>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem' }}>
          {can('finance.invoice.issue') && inv.status === 'draft' ? (
            <Button type="button" onClick={() => void issue()} disabled={busy}>
              Issue
            </Button>
          ) : null}
          {can('finance.invoice.send') &&
          (inv.status === 'issued' || inv.status === 'sent' || inv.status === 'partially_paid') ? (
            <Button type="button" variant="secondary" onClick={() => void send()} disabled={busy}>
              Send / log payment link
            </Button>
          ) : null}
          <Link href="/finance/invoices" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Back</Button></Link>
        </div>
      </header>

      <Table
        getRowKey={(l: Line) => l.id}
        columns={[
          { key: 'description', header: 'Description', cell: (l: Line) => l.description },
          { key: 'qty', header: 'Qty', align: 'right', cell: (l: Line) => l.quantity },
          {
            key: 'price',
            header: 'Unit',
            align: 'right',
            cell: (l: Line) => (
              <MoneyCell amount={l.unit_price_minor / 100} currency={inv.currency} />
            ),
          },
          {
            key: 'total',
            header: 'Line total',
            align: 'right',
            cell: (l: Line) => (
              <MoneyCell amount={l.line_total_minor / 100} currency={inv.currency} />
            ),
          },
        ]}
        rows={inv.lines}
      />
      <p style={{ textAlign: 'right', marginTop: '1rem' }}>
        Total <MoneyCell amount={inv.total_minor / 100} currency={inv.currency} />
      </p>
    </section>
  );
}

const headerStyle: CSSProperties = {
  marginBottom: '1.25rem',
  display: 'flex',
  flexWrap: 'wrap',
  gap: '1rem',
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
