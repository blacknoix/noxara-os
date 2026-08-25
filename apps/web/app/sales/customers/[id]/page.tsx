'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Card,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatusCell,
  Table,
  Tabs,
  Timeline,
  type TimelineItem,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type Customer = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  website: string | null;
  billing_address: string | null;
  notes: string | null;
  owner_user_id: string | null;
  created_at: string;
  updated_at: string;
};

type Activity = {
  id: string;
  kind: string;
  subject: string | null;
  body: string | null;
  occurred_at: string;
  customer_id: string | null;
  deal_id: string | null;
};

type Deal = {
  id: string;
  customer_id: string | null;
  name: string;
  amount_minor: number;
  currency: string;
  status: string;
};

type Quote = {
  id: string;
  customer_id: string;
  quote_number: string;
  status: string;
  total_minor: number;
  currency: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; customer: Customer };

function activityLabel(kind: string): string {
  switch (kind) {
    case 'call':
      return 'Call';
    case 'meeting':
      return 'Meeting';
    case 'email':
      return 'Email';
    default:
      return 'Note';
  }
}

export default function CustomerRecordPage() {
  const params = useParams<{ id: string }>();
  const customerId = params.id;
  const { can, loading: capsLoading } = useCapabilities();

  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [tab, setTab] = useState('overview');
  const [activities, setActivities] = useState<Activity[]>([]);
  const [activitiesError, setActivitiesError] = useState<string | null>(null);
  const [deals, setDeals] = useState<Deal[]>([]);
  const [dealsError, setDealsError] = useState<string | null>(null);
  const [quotes, setQuotes] = useState<Quote[]>([]);
  const [quotesError, setQuotesError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch(`/api/v1/sales/customers/${customerId}`);
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
      let message = 'Could not load this customer.';
      try {
        const body = await res.json();
        if (typeof body.detail === 'string') message = body.detail;
      } catch {
        /* ignore */
      }
      setState({ status: 'error', message, requestId });
      return;
    }
    const customer = (await res.json()) as Customer;
    setState({ status: 'ready', customer });
  }, [customerId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (state.status !== 'ready' || !can('sales.activity.read')) return;
    void (async () => {
      const res = await authFetch(`/api/v1/sales/activities?customer_id=${customerId}&limit=100`);
      if (!res.ok) {
        setActivitiesError('Could not load activity timeline');
        return;
      }
      const body = await res.json();
      setActivities(body.items ?? []);
    })();
  }, [state.status, can, customerId]);

  useEffect(() => {
    if (state.status !== 'ready' || !can('sales.deal.read')) return;
    void (async () => {
      const res = await authFetch('/api/v1/sales/deals?limit=200');
      if (!res.ok) {
        setDealsError('Could not load deals');
        return;
      }
      const body = await res.json();
      setDeals((body.items ?? []).filter((d: Deal) => d.customer_id === customerId));
    })();
  }, [state.status, can, customerId]);

  useEffect(() => {
    if (state.status !== 'ready' || !can('sales.quote.read')) return;
    void (async () => {
      const res = await authFetch('/api/v1/sales/quotes?limit=200');
      if (!res.ok) {
        setQuotesError('Could not load quotes');
        return;
      }
      const body = await res.json();
      setQuotes((body.items ?? []).filter((q: Quote) => q.customer_id === customerId));
    })();
  }, [state.status, can, customerId]);

  const timelineItems: TimelineItem[] = useMemo(
    () =>
      activities.map((a) => ({
        id: a.id,
        title: a.subject ?? activityLabel(a.kind),
        description: a.body ?? undefined,
        timestamp: new Date(a.occurred_at).toLocaleString(),
      })),
    [activities],
  );

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (!can('sales.customer.read')) {
    return <PermissionDeniedState requiredPermission="sales.customer.read" />;
  }

  if (state.status === 'loading') {
    return <LoadingState label="Loading customer" rows={5} />;
  }

  if (state.status === 'signed_out') {
    return <ErrorState title="Sign in required" message="Open /login to view this customer." />;
  }

  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="sales.customer.read" />;
  }

  if (state.status === 'error') {
    return <ErrorState message={state.message} requestId={state.requestId} />;
  }

  const { customer } = state;

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>
          <Link href="/sales/customers" style={{ color: 'inherit', textDecoration: 'none' }}>
            Sales / Customers
          </Link>
        </p>
        <h1 style={h1}>{customer.name}</h1>
      </header>

      <Tabs
        items={[
          { id: 'overview', label: 'Overview' },
          { id: 'timeline', label: 'Timeline' },
          { id: 'deals', label: 'Deals' },
          { id: 'quotes', label: 'Quotes' },
        ]}
        value={tab}
        onChange={setTab}
      >
        {tab === 'overview' ? (
          <Card>
            <dl style={dlStyle}>
              <dt style={dtStyle}>Name</dt>
              <dd style={ddStyle}>{customer.name}</dd>
              <dt style={dtStyle}>Email</dt>
              <dd style={ddStyle}>{customer.email ?? '—'}</dd>
              <dt style={dtStyle}>Phone</dt>
              <dd style={ddStyle}>{customer.phone ?? '—'}</dd>
              <dt style={dtStyle}>Website</dt>
              <dd style={ddStyle}>{customer.website ?? '—'}</dd>
              <dt style={dtStyle}>Billing address</dt>
              <dd style={ddStyle}>{customer.billing_address ?? '—'}</dd>
              <dt style={dtStyle}>Notes</dt>
              <dd style={ddStyle}>{customer.notes ?? '—'}</dd>
            </dl>
          </Card>
        ) : null}

        {tab === 'timeline' ? (
          !can('sales.activity.read') ? (
            <PermissionDeniedState requiredPermission="sales.activity.read" />
          ) : activitiesError ? (
            <ErrorState message={activitiesError} />
          ) : timelineItems.length === 0 ? (
            <EmptyState title="No activity yet" description="Calls, meetings, emails, and notes appear here." />
          ) : (
            <Timeline items={timelineItems} />
          )
        ) : null}

        {tab === 'deals' ? (
          !can('sales.deal.read') ? (
            <PermissionDeniedState requiredPermission="sales.deal.read" />
          ) : dealsError ? (
            <ErrorState message={dealsError} />
          ) : deals.length === 0 ? (
            <EmptyState title="No deals for this customer" description="Deals linked to this customer appear here." />
          ) : (
            <Table
              getRowKey={(d: Deal) => d.id}
              columns={[
                {
                  key: 'name',
                  header: 'Name',
                  cell: (d: Deal) => (
                    <Link href={`/sales/deals?q=${encodeURIComponent(d.name)}`} style={{ color: 'var(--cos-color-accent)' }}>
                      {d.name}
                    </Link>
                  ),
                },
                {
                  key: 'amount',
                  header: 'Amount',
                  align: 'right',
                  cell: (d: Deal) => <MoneyCell amount={d.amount_minor / 100} currency={d.currency} />,
                },
                { key: 'status', header: 'Status', cell: (d: Deal) => <StatusCell status={d.status} /> },
              ]}
              rows={deals}
            />
          )
        ) : null}

        {tab === 'quotes' ? (
          !can('sales.quote.read') ? (
            <PermissionDeniedState requiredPermission="sales.quote.read" />
          ) : quotesError ? (
            <ErrorState message={quotesError} />
          ) : quotes.length === 0 ? (
            <EmptyState title="No quotes for this customer" description="Quotes sent to this customer appear here." />
          ) : (
            <Table
              getRowKey={(q: Quote) => q.id}
              columns={[
                {
                  key: 'quote_number',
                  header: 'Quote #',
                  cell: (q: Quote) => (
                    <Link href={`/sales/quotes/${q.id}`} style={{ color: 'var(--cos-color-accent)' }}>
                      {q.quote_number}
                    </Link>
                  ),
                },
                {
                  key: 'total',
                  header: 'Total',
                  align: 'right',
                  cell: (q: Quote) => <MoneyCell amount={q.total_minor / 100} currency={q.currency} />,
                },
                { key: 'status', header: 'Status', cell: (q: Quote) => <StatusCell status={q.status} /> },
              ]}
              rows={quotes}
            />
          )
        ) : null}
      </Tabs>
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

const dlStyle: CSSProperties = {
  margin: 0,
  display: 'grid',
  gridTemplateColumns: '160px 1fr',
  gap: '0.6rem 1rem',
};

const dtStyle: CSSProperties = {
  fontWeight: 600,
  color: 'var(--cos-color-fg-muted)',
  fontSize: '0.8125rem',
};

const ddStyle: CSSProperties = {
  margin: 0,
  color: 'var(--cos-color-fg)',
};
