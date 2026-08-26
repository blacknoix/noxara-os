'use client';

import { Suspense, useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  EmptyState,
  ErrorState,
  FilterBar,
  LinkCell,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Quote = {
  id: string;
  customer_id: string;
  deal_id: string | null;
  quote_number: string;
  status: string;
  total_minor: number;
  currency: string;
  valid_until: string | null;
  created_at: string;
};

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

function QuotesPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const [quotes, setQuotes] = useState<Quote[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [q, setQ] = useState(searchParams?.get('q') ?? '');

  const syncUrl = useCallback(
    (next: string) => {
      router.replace(next ? `/sales/quotes?q=${encodeURIComponent(next)}` : '/sales/quotes', {
        scroll: false,
      });
    },
    [router],
  );

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/sales/quotes?limit=200');
    const rid = res.headers.get('x-request-id') ?? undefined;
    setRequestId(rid);
    if (res.status === 401) {
      setError('Sign in required');
      setLoading(false);
      return;
    }
    if (res.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!res.ok) {
      setError('Could not load quotes');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setQuotes(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return quotes;
    return quotes.filter((quote) => quote.quote_number.toLowerCase().includes(needle));
  }, [quotes, q]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view quotes." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('sales.quote.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="sales.quote.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={h1}>Quotes</h1>
        <p style={muted}>Draft, send, and track customer quotes.</p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <FilterBar
        q={q}
        onQueryChange={(next) => {
          setQ(next);
          syncUrl(next);
        }}
        searchPlaceholder="Search quote number…"
        filters={[]}
        onFiltersChange={() => {}}
      />

      {loading ? (
        <LoadingState label="Loading quotes" rows={4} />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No quotes match"
          description={
            quotes.length === 0
              ? 'Quotes appear here once created for a customer.'
              : 'Try clearing the search.'
          }
        />
      ) : (
        <Table
          getRowKey={(quote: Quote) => quote.id}
          columns={[
            {
              key: 'quote_number',
              header: 'Quote #',
              cell: (quote: Quote) => <LinkCell href={`/sales/quotes/${quote.id}`}>{quote.quote_number}</LinkCell>,
            },
            {
              key: 'customer',
              header: 'Customer',
              cell: (quote: Quote) =>
                can('sales.customer.read') ? (
                  <Link href={`/sales/customers/${quote.customer_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                    {quote.customer_id}
                  </Link>
                ) : (
                  quote.customer_id
                ),
            },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (quote: Quote) => <MoneyCell amount={quote.total_minor / 100} currency={quote.currency} />,
            },
            {
              key: 'status',
              header: 'Status',
              cell: (quote: Quote) => <StatusCell status={quote.status} tone={statusTone(quote.status)} />,
            },
            {
              key: 'valid_until',
              header: 'Valid until',
              cell: (quote: Quote) => quote.valid_until ?? '—',
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function QuotesPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading quotes…</p>}>
      <QuotesPageInner />
    </Suspense>
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
