'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';

type PayslipLine = {
  id: string;
  line_kind: string;
  label: string;
  amount_minor: number;
  currency: string;
  calculation_basis: Record<string, unknown>;
};

type Payslip = {
  id: string;
  run_id: string;
  gross_minor: number;
  deductions_minor: number;
  net_minor: number;
  status: string;
  currency: string;
  issued_at: string | null;
  lines: PayslipLine[];
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; items: Payslip[] };

function money(minor: number, currency: string) {
  return `${(minor / 100).toFixed(2)} ${currency}`;
}

export default function MyPayslipsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [selected, setSelected] = useState<Payslip | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/me/payslips');
    const requestId = res.headers.get('x-request-id') ?? undefined;
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load your payslips.', requestId });
      return;
    }
    const body = (await res.json()) as { items: Payslip[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function openSlip(id: string) {
    const res = await authFetch(`/api/v1/people/me/payslips/${id}`);
    if (!res.ok) return;
    setSelected((await res.json()) as Payslip);
  }

  if (state.status === 'loading') {
    return (
      <main style={page}>
        <LoadingState label="Loading payslips…" />
      </main>
    );
  }
  if (state.status === 'signed_out') {
    return (
      <main style={page}>
        <EmptyState title="Sign in required" description="Sign in to view your payslips." />
      </main>
    );
  }
  if (state.status === 'error') {
    return (
      <main style={page}>
        <ErrorState title="My payslips" message={state.message} requestId={state.requestId} />
      </main>
    );
  }

  return (
    <main style={page}>
      <header style={{ marginBottom: 24 }}>
        <h1 style={{ margin: 0, fontSize: 28 }}>My payslips</h1>
        <p style={{ margin: '8px 0 0', color: 'var(--color-muted, #666)' }}>
          Your own issued payslips only. Every figure includes its calculation basis.
        </p>
      </header>

      {state.items.length === 0 ? (
        <EmptyState title="No payslips yet" description="Payslips appear after payroll is paid." />
      ) : (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            {
              key: 'id',
              header: 'Payslip',
              cell: (r) => (
                <button type="button" style={linkBtn} onClick={() => openSlip(r.id)}>
                  {r.id}
                </button>
              ),
            },
            {
              key: 'net',
              header: 'Net',
              cell: (r) => money(r.net_minor, r.currency),
            },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            {
              key: 'issued',
              header: 'Issued',
              cell: (r) => r.issued_at ?? '—',
            },
          ]}
          rows={state.items}
        />
      )}

      {selected ? (
        <section style={{ marginTop: 24 }} aria-label="Payslip lines">
          <h2 style={{ fontSize: 18 }}>Lines</h2>
          <Table
            getRowKey={(l) => l.id}
            columns={[
              { key: 'kind', header: 'Kind', cell: (l) => l.line_kind },
              { key: 'label', header: 'Label', cell: (l) => l.label },
              {
                key: 'amount',
                header: 'Amount',
                cell: (l) => money(l.amount_minor, l.currency),
              },
              {
                key: 'basis',
                header: 'Basis',
                cell: (l) => (
                  <code style={{ fontSize: 12 }}>{JSON.stringify(l.calculation_basis)}</code>
                ),
              },
            ]}
            rows={selected.lines}
          />
        </section>
      ) : null}
    </main>
  );
}

const page: CSSProperties = { padding: 24, maxWidth: 960 };
const linkBtn: CSSProperties = {
  background: 'none',
  border: 'none',
  color: 'var(--color-accent, #0b5)',
  cursor: 'pointer',
  padding: 0,
  textDecoration: 'underline',
};
