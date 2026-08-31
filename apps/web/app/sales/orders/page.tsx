'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Order = {
  id: string;
  customer_id: string;
  deal_id: string | null;
  quote_id: string | null;
  status: string;
  currency: string;
  total_minor: number;
  created_at: string;
};

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'fulfilled':
      return 'success';
    case 'confirmed':
      return 'info';
    case 'cancelled':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function SalesOrdersPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [orders, setOrders] = useState<Order[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const res = await authFetch('/api/v1/sales/orders?limit=100');
    if (res.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!res.ok) {
      setError('Could not load orders');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setOrders(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view orders." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading orders" />;
  if (denied || !can('sales.order.read')) {
    return <PermissionDeniedState requiredPermission="sales.order.read" />;
  }
  if (error) return <ErrorState title="Orders unavailable" message={error} />;

  return (
    <div style={page}>
      <header style={header}>
        <div>
          <p style={eyebrow}>Sales</p>
          <h1 style={title}>Orders</h1>
          <p style={muted}>Orders created from accepted quotes or won deals (minor-unit money).</p>
        </div>
        <Link href="/sales/quotes" style={link}>
          From quotes →
        </Link>
      </header>
      {orders.length === 0 ? (
        <EmptyState
          title="No orders yet"
          description="Accept a quote, then create an order via from-quote."
        />
      ) : (
        <Table
          getRowKey={(o: Order) => o.id}
          columns={[
            { key: 'id', header: 'Order', cell: (o: Order) => o.id },
            {
              key: 'status',
              header: 'Status',
              cell: (o: Order) => <StatusCell status={o.status} tone={statusTone(o.status)} />,
            },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (o: Order) => (
                <MoneyCell amount={o.total_minor / 100} currency={o.currency} />
              ),
            },
            { key: 'quote', header: 'Quote', cell: (o: Order) => o.quote_id ?? '—' },
            { key: 'deal', header: 'Deal', cell: (o: Order) => o.deal_id ?? '—' },
            {
              key: 'created',
              header: 'Created',
              cell: (o: Order) => new Date(o.created_at).toLocaleDateString(),
            },
          ]}
          rows={orders}
        />
      )}
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.25rem' };
const header: CSSProperties = {
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-end',
  gap: '1rem',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const link: CSSProperties = { color: 'var(--cos-color-accent)' };
