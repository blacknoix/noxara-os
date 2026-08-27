'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type Entry = {
  leave_request_id: string;
  employee_id: string;
  employee_display_name: string;
  leave_type_code: string;
  status: string;
  start_date: string;
  end_date: string;
  units_milli: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Entry[] };

export default function TeamLeaveCalendarPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/leave/calendar');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load team calendar.' });
      return;
    }
    const body = (await res.json()) as { items: Entry[] };
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
        <h1 style={h1Style}>Team leave calendar</h1>
        <p style={leadStyle}>Approved and pending leave visible at your permission scope.</p>
      </header>
      {state.status === 'loading' ? <LoadingState label="Loading calendar…" /> : null}
      {state.status === 'signed_out' ? (
        <ErrorState title="Sign in required" message="Sign in to view the calendar." />
      ) : null}
      {state.status === 'denied' ? <PermissionDeniedState requiredPermission="hr.leave.read" /> : null}
      {state.status === 'error' ? (
        <ErrorState title="Could not load" message={state.message} />
      ) : null}
      {state.status === 'ready' && state.items.length === 0 ? (
        <EmptyState title="No leave in range" description="Nothing scheduled for this period." />
      ) : null}
      {state.status === 'ready' && state.items.length > 0 ? (
        <Table
          getRowKey={(r) => r.leave_request_id}
          columns={[
            { key: 'name', header: 'Employee', cell: (r) => r.employee_display_name },
            { key: 'type', header: 'Type', cell: (r) => r.leave_type_code },
            { key: 'start', header: 'Start', cell: (r) => r.start_date },
            { key: 'end', header: 'End', cell: (r) => r.end_date },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
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
