'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import {
  Button,
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

type Contract = {
  id: string;
  customer_id: string;
  title: string;
  status: string;
  start_date: string;
  end_date: string;
  value_minor: number;
  currency: string;
  auto_renew: boolean;
};

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'active':
      return 'success';
    case 'expired':
      return 'warning';
    case 'cancelled':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function SalesContractsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [contracts, setContracts] = useState<Contract[]>([]);
  const [renewals, setRenewals] = useState<Contract[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const [listRes, renewRes] = await Promise.all([
      authFetch('/api/v1/sales/contracts?limit=100'),
      authFetch('/api/v1/sales/contracts/renewals?within_days=90'),
    ]);
    if (listRes.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!listRes.ok) {
      setError('Could not load contracts');
      setLoading(false);
      return;
    }
    const listBody = await listRes.json();
    setContracts(listBody.items ?? []);
    if (renewRes.ok) {
      const renewBody = await renewRes.json();
      setRenewals(renewBody.items ?? renewBody.contracts ?? []);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function publish(id: string) {
    if (!can('sales.contract.publish')) return;
    setBusyId(id);
    await authFetch(`/api/v1/sales/contracts/${encodeURIComponent(id)}/publish`, {
      method: 'POST',
    });
    setBusyId(null);
    await load();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view contracts." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading contracts…" />;
  if (denied || !can('sales.contract.read')) {
    return <PermissionDeniedState requiredPermission="sales.contract.read" />;
  }
  if (error) return <ErrorState title="Contracts unavailable" message={error} />;

  return (
    <div style={page}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={title}>Contracts</h1>
        <p style={muted}>Customer contracts and upcoming renewals (90-day window).</p>
      </header>

      <section style={section}>
        <h2 style={sectionTitle}>Upcoming renewals</h2>
        {renewals.length === 0 ? (
          <p style={muted}>No renewals in the next 90 days.</p>
        ) : (
          <Table
            getRowKey={(c: Contract) => `r-${c.id}`}
            columns={[
              { key: 'title', header: 'Contract', cell: (c: Contract) => c.title },
              { key: 'end', header: 'Ends', cell: (c: Contract) => c.end_date },
              {
                key: 'value',
                header: 'Value',
                align: 'right',
                cell: (c: Contract) => (
                  <MoneyCell amount={c.value_minor / 100} currency={c.currency} />
                ),
              },
            ]}
            rows={renewals}
          />
        )}
      </section>

      <section style={section}>
        <h2 style={sectionTitle}>All contracts</h2>
        {contracts.length === 0 ? (
          <EmptyState title="No contracts" description="Create a draft contract for a customer." />
        ) : (
          <Table
            getRowKey={(c: Contract) => c.id}
            columns={[
              { key: 'title', header: 'Title', cell: (c: Contract) => c.title },
              {
                key: 'status',
                header: 'Status',
                cell: (c: Contract) => (
                  <StatusCell status={c.status} tone={statusTone(c.status)} />
                ),
              },
              {
                key: 'term',
                header: 'Term',
                cell: (c: Contract) => `${c.start_date} → ${c.end_date}`,
              },
              {
                key: 'value',
                header: 'Value',
                align: 'right',
                cell: (c: Contract) => (
                  <MoneyCell amount={c.value_minor / 100} currency={c.currency} />
                ),
              },
              {
                key: 'actions',
                header: '',
                cell: (c: Contract) =>
                  c.status === 'draft' && can('sales.contract.publish') ? (
                    <Button
                      size="sm"
                      disabled={busyId === c.id}
                      onClick={() => void publish(c.id)}
                    >
                      Publish
                    </Button>
                  ) : null,
              },
            ]}
            rows={contracts}
          />
        )}
      </section>
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.5rem' };
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const section: CSSProperties = { display: 'grid', gap: '0.75rem' };
const sectionTitle: CSSProperties = { margin: 0, fontSize: '1.1rem' };
