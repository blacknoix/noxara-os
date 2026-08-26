'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Button,
  Card,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatusCell,
  Table,
  useToast,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type QuoteLine = {
  id: string;
  position: number;
  description: string;
  quantity: number;
  unit_price_minor: number;
  discount_minor: number;
  tax_rate_bps: number;
  tax_minor: number;
  line_total_minor: number;
};

type Quote = {
  id: string;
  deal_id: string | null;
  customer_id: string;
  quote_number: string;
  status: string;
  currency: string;
  subtotal_minor: number;
  discount_minor: number;
  tax_minor: number;
  total_minor: number;
  notes: string | null;
  valid_until: string | null;
  accepted_at: string | null;
  version: number;
  lines: QuoteLine[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; quote: Quote };

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'accepted':
      return 'success';
    case 'sent':
      return 'info';
    case 'rejected':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function QuoteDetailPage() {
  const params = useParams<{ id: string }>();
  const quoteId = params.id;
  const { can, loading: capsLoading } = useCapabilities();
  const { toast } = useToast();

  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [busy, setBusy] = useState(false);
  const [invoiceActionAvailable, setInvoiceActionAvailable] = useState(false);
  const [invoiceActionReason, setInvoiceActionReason] = useState<string | null>(null);
  const [creatingInvoice, setCreatingInvoice] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch(`/api/v1/sales/quotes/${quoteId}`);
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
      let message = 'Could not load this quote.';
      try {
        const body = await res.json();
        if (typeof body.detail === 'string') message = body.detail;
      } catch {
        /* ignore */
      }
      setState({ status: 'error', message, requestId });
      return;
    }
    const quote = (await res.json()) as Quote;
    setState({ status: 'ready', quote });
  }, [quoteId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (state.status !== 'ready' || state.quote.status !== 'accepted') return;
    void (async () => {
      const res = await authFetch(`/api/v1/sales/quotes/${quoteId}/invoice-action`);
      if (!res.ok) return;
      const body = (await res.json()) as { available: boolean; reason: string };
      setInvoiceActionAvailable(body.available);
      setInvoiceActionReason(body.available ? null : body.reason);
    })();
  }, [state, quoteId]);

  const createInvoiceFromQuote = useCallback(async () => {
    if (state.status !== 'ready') return;
    const quote = state.quote;
    setCreatingInvoice(true);
    try {
      const res = await authFetch('/api/v1/finance/invoices/from-quote', {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'idempotency-key': `qte-inv-${quote.id}-${Date.now()}`,
        },
        body: JSON.stringify({
          quote_id: quote.id,
          customer_id: quote.customer_id,
          customer_name: quote.customer_id,
          currency: quote.currency,
          terms: null,
          notes: quote.notes,
          total_minor: quote.total_minor,
          lines: quote.lines.map((l) => ({
            description: l.description,
            quantity: l.quantity,
            unit_price_minor: l.unit_price_minor,
            discount_minor: l.discount_minor,
            tax_rate_bps: l.tax_rate_bps,
          })),
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(typeof body.detail === 'string' ? body.detail : 'Could not create invoice');
      }
      const invoice = (await res.json()) as { id: string };
      toast({ title: 'Invoice draft created' });
      window.location.href = `/finance/invoices/${invoice.id}`;
    } catch (err) {
      toast({
        title: 'Could not create invoice',
        description: err instanceof Error ? err.message : undefined,
      });
    } finally {
      setCreatingInvoice(false);
    }
  }, [state, toast]);

  const send = useCallback(async () => {
    setBusy(true);
    try {
      const res = await authFetch(`/api/v1/sales/quotes/${quoteId}/send`, { method: 'POST' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(typeof body.detail === 'string' ? body.detail : 'Could not send quote');
      }
      await load();
      toast({ title: 'Quote sent' });
    } catch (err) {
      toast({ title: 'Could not send quote', description: err instanceof Error ? err.message : undefined });
    } finally {
      setBusy(false);
    }
  }, [quoteId, load, toast]);

  const accept = useCallback(async () => {
    setBusy(true);
    try {
      const res = await authFetch(`/api/v1/sales/quotes/${quoteId}/accept`, { method: 'POST' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(typeof body.detail === 'string' ? body.detail : 'Could not accept quote');
      }
      await load();
      toast({ title: 'Quote accepted' });
    } catch (err) {
      toast({ title: 'Could not accept quote', description: err instanceof Error ? err.message : undefined });
    } finally {
      setBusy(false);
    }
  }, [quoteId, load, toast]);

  const reject = useCallback(async () => {
    setBusy(true);
    try {
      const res = await authFetch(`/api/v1/sales/quotes/${quoteId}/reject`, {
        method: 'POST',
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(typeof body.detail === 'string' ? body.detail : 'Could not reject quote');
      }
      await load();
      toast({ title: 'Quote rejected' });
    } catch (err) {
      toast({ title: 'Could not reject quote', description: err instanceof Error ? err.message : undefined });
    } finally {
      setBusy(false);
    }
  }, [quoteId, load, toast]);

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (!can('sales.quote.read')) {
    return <PermissionDeniedState requiredPermission="sales.quote.read" />;
  }

  if (state.status === 'loading') {
    return <LoadingState label="Loading quote" rows={5} />;
  }

  if (state.status === 'signed_out') {
    return <ErrorState title="Sign in required" message="Open /login to view this quote." />;
  }

  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="sales.quote.read" />;
  }

  if (state.status === 'error') {
    return <ErrorState message={state.message} requestId={state.requestId} />;
  }

  const { quote } = state;
  const canAct = (quote.status === 'draft' || quote.status === 'sent') && !busy;

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '1rem',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <p style={eyebrow}>
            <Link href="/sales/quotes" style={{ color: 'inherit', textDecoration: 'none' }}>
              Sales / Quotes
            </Link>
          </p>
          <h1 style={h1}>{quote.quote_number}</h1>
          <p style={muted}>
            <StatusCell status={quote.status} tone={statusTone(quote.status)} />{' '}
            {quote.valid_until ? `· valid until ${quote.valid_until}` : null}{' '}
            {can('sales.customer.read') ? (
              <>
                ·{' '}
                <Link href={`/sales/customers/${quote.customer_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                  View customer
                </Link>
              </>
            ) : null}
          </p>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          {can('sales.quote.update') && quote.status === 'draft' ? (
            <Button type="button" variant="secondary" disabled={!canAct} onClick={() => void send()}>
              Send
            </Button>
          ) : null}
          {can('sales.quote.accept') && canAct ? (
            <Button type="button" onClick={() => void accept()} disabled={!canAct}>
              Accept
            </Button>
          ) : null}
          {can('sales.quote.update') && canAct ? (
            <Button type="button" variant="ghost" onClick={() => void reject()} disabled={!canAct}>
              Reject
            </Button>
          ) : null}
          {quote.status === 'accepted' ? (
            invoiceActionAvailable && can('finance.invoice.create') ? (
              <Button
                type="button"
                variant="secondary"
                disabled={creatingInvoice}
                onClick={() => void createInvoiceFromQuote()}
              >
                Create invoice from quote
              </Button>
            ) : (
              <span title={invoiceActionReason ?? 'Invoicing unavailable'}>
                <Button type="button" variant="secondary" disabled>
                  Create invoice from quote
                </Button>
              </span>
            )
          ) : null}
        </div>
      </header>

      {quote.status === 'accepted' && invoiceActionReason ? (
        <p style={{ fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)' }}>
          Invoicing unavailable ({invoiceActionReason}).
        </p>
      ) : null}

      <Card>
        <Table
          getRowKey={(l: QuoteLine) => l.id}
          columns={[
            { key: 'description', header: 'Description', cell: (l: QuoteLine) => l.description || '—' },
            { key: 'quantity', header: 'Qty', align: 'right', cell: (l: QuoteLine) => l.quantity },
            {
              key: 'unit_price',
              header: 'Unit price',
              align: 'right',
              cell: (l: QuoteLine) => <MoneyCell amount={l.unit_price_minor / 100} currency={quote.currency} />,
            },
            {
              key: 'discount',
              header: 'Discount',
              align: 'right',
              cell: (l: QuoteLine) => <MoneyCell amount={l.discount_minor / 100} currency={quote.currency} />,
            },
            {
              key: 'tax',
              header: 'Tax',
              align: 'right',
              cell: (l: QuoteLine) => <MoneyCell amount={l.tax_minor / 100} currency={quote.currency} />,
            },
            {
              key: 'line_total',
              header: 'Line total',
              align: 'right',
              cell: (l: QuoteLine) => <MoneyCell amount={l.line_total_minor / 100} currency={quote.currency} />,
            },
          ]}
          rows={quote.lines}
        />

        <div style={{ marginTop: '1rem', display: 'grid', gap: '0.35rem', maxWidth: 280, marginLeft: 'auto' }}>
          <TotalsRow label="Subtotal" amount={quote.subtotal_minor} currency={quote.currency} />
          <TotalsRow label="Discount" amount={quote.discount_minor} currency={quote.currency} />
          <TotalsRow label="Tax" amount={quote.tax_minor} currency={quote.currency} />
          <TotalsRow label="Total" amount={quote.total_minor} currency={quote.currency} strong />
        </div>
      </Card>

      {quote.notes ? (
        <Card>
          <h2 style={sectionHeading}>Notes</h2>
          <p style={{ margin: '0.5rem 0 0', whiteSpace: 'pre-wrap', color: 'var(--cos-color-fg)' }}>
            {quote.notes}
          </p>
        </Card>
      ) : null}
    </div>
  );
}

function TotalsRow({
  label,
  amount,
  currency,
  strong,
}: {
  label: string;
  amount: number;
  currency: string;
  strong?: boolean;
}) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', fontWeight: strong ? 700 : 500 }}>
      <span style={{ color: strong ? 'var(--cos-color-fg)' : 'var(--cos-color-fg-muted)' }}>{label}</span>
      <MoneyCell amount={amount / 100} currency={currency} />
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
  display: 'flex',
  gap: '0.35rem',
  alignItems: 'center',
  flexWrap: 'wrap',
};

const sectionHeading: CSSProperties = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.05rem',
  fontWeight: 600,
};
