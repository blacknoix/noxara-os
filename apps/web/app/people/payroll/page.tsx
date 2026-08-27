'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type PayrollRun = {
  id: string;
  status: string;
  period_start: string;
  period_end: string;
  currency: string;
  employee_count: number;
  gross_minor: number;
  net_minor: number;
  adjustment_of_run_id: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; items: PayrollRun[] };

function money(minor: number, currency: string) {
  return `${(minor / 100).toFixed(2)} ${currency}`;
}

export default function PayrollRunsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [periodStart, setPeriodStart] = useState('');
  const [periodEnd, setPeriodEnd] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const canRead = can('hr.payroll.read');
  const canWrite = can('hr.payroll.write');

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/payroll/runs?limit=50');
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
      setState({ status: 'error', message: 'Could not load payroll runs.', requestId });
      return;
    }
    const body = (await res.json()) as { items: PayrollRun[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setBusy(true);
    const res = await authFetch('/api/v1/people/payroll/runs', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        period_start: periodStart,
        period_end: periodEnd,
        currency,
      }),
    });
    setBusy(false);
    if (!res.ok) {
      setFormError('Could not create payroll run.');
      return;
    }
    setPeriodStart('');
    setPeriodEnd('');
    await load();
  }

  if (capsLoading || state.status === 'loading') {
    return (
      <main style={page}>
        <LoadingState label="Loading payroll…" />
      </main>
    );
  }
  if (state.status === 'signed_out') {
    return (
      <main style={page}>
        <EmptyState title="Sign in required" description="Sign in to manage payroll." />
      </main>
    );
  }
  if (state.status === 'denied' || !canRead) {
    return (
      <main style={page}>
        <PermissionDeniedState requiredPermission="hr.payroll.read" />
      </main>
    );
  }
  if (state.status === 'error') {
    return (
      <main style={page}>
        <ErrorState title="Payroll" message={state.message} requestId={state.requestId} />
      </main>
    );
  }

  return (
    <main style={page}>
      <header style={{ marginBottom: 24 }}>
        <h1 style={{ margin: 0, fontSize: 28 }}>Payroll</h1>
        <p style={{ margin: '8px 0 0', color: 'var(--color-muted, #666)' }}>
          Draft, calculate, approve, and pay runs. Approved runs are immutable.
        </p>
      </header>

      {canWrite ? (
        <form onSubmit={onCreate} style={formRow}>
          <label>
            Period start
            <input
              type="date"
              required
              value={periodStart}
              onChange={(e) => setPeriodStart(e.target.value)}
            />
          </label>
          <label>
            Period end
            <input
              type="date"
              required
              value={periodEnd}
              onChange={(e) => setPeriodEnd(e.target.value)}
            />
          </label>
          <label>
            Currency
            <input
              value={currency}
              onChange={(e) => setCurrency(e.target.value.toUpperCase())}
              maxLength={3}
              style={{ width: 72 }}
            />
          </label>
          <Button type="submit" disabled={busy}>
            Create draft
          </Button>
          {formError ? <p role="alert">{formError}</p> : null}
        </form>
      ) : null}

      {state.items.length === 0 ? (
        <EmptyState title="No payroll runs" description="Create a draft for a pay period." />
      ) : (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            {
              key: 'period',
              header: 'Period',
              cell: (r) => (
                <Link href={`/people/payroll/${r.id}`}>
                  {r.period_start} → {r.period_end}
                </Link>
              ),
            },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            {
              key: 'employees',
              header: 'Employees',
              cell: (r) => String(r.employee_count),
            },
            {
              key: 'net',
              header: 'Net',
              cell: (r) => money(r.net_minor, r.currency),
            },
            {
              key: 'adj',
              header: 'Adjustment',
              cell: (r) => (r.adjustment_of_run_id ? 'Yes' : '—'),
            },
          ]}
          rows={state.items}
        />
      )}
    </main>
  );
}

const page: CSSProperties = { padding: 24, maxWidth: 960 };
const formRow: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 12,
  alignItems: 'end',
  marginBottom: 24,
};
