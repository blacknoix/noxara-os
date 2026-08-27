'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type Balance = {
  leave_type_code: string;
  leave_type_name: string;
  balance_days: string;
  as_of: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Balance[] };

export default function LeaveBalancesPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/leave/balances');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load balances.' });
      return;
    }
    const body = (await res.json()) as { items: Balance[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading) return <LoadingState label="Loading permissions…" />;
  if (!can('hr.leave.read')) return <PermissionDeniedState requiredPermission="hr.leave.read" />;

  return (
    <main style={pageStyle}>
      <header>
        <p style={eyebrowStyle}>
          <Link href="/people/leave">Leave</Link>
        </p>
        <h1 style={h1Style}>Leave balances</h1>
        <p style={leadStyle}>Balances are always derived from the leave ledger.</p>
      </header>
      {state.status === 'loading' ? <LoadingState label="Loading balances…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to view balances." />
      ) : null}
      {state.status === 'denied' ? <PermissionDeniedState requiredPermission="hr.leave.read" /> : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} />
      ) : null}
      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState title="No leave types" description="Ask an admin to configure leave types." />
      ) : null}
      {state.status === 'ready' && state.items.length > 0 ? (
        <Table
          getRowKey={(r) => r.leave_type_code}
          columns={[
            { key: 'code', header: 'Type', cell: (r) => r.leave_type_code },
            { key: 'name', header: 'Name', cell: (r) => r.leave_type_name },
            { key: 'balance', header: 'Balance (days)', cell: (r) => r.balance_days },
            { key: 'as_of', header: 'As of', cell: (r) => r.as_of },
          ]}
          rows={state.items}
        />
      ) : null}
    </main>
  );
}

const pageStyle: CSSProperties = { display: 'grid', gap: 24, padding: '8px 0 48px' };
const eyebrowStyle: CSSProperties = { margin: 0, fontSize: 13, opacity: 0.7 };
const h1Style: CSSProperties = { margin: '4px 0 8px', fontSize: 28, fontWeight: 650 };
const leadStyle: CSSProperties = { margin: 0, maxWidth: 52 * 8, lineHeight: 1.5, opacity: 0.85 };
