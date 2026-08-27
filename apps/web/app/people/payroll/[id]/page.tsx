'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type PayslipLine = {
  id: string;
  line_kind: string;
  component_code: string;
  label: string;
  amount_minor: number;
  currency: string;
  calculation_basis: Record<string, unknown>;
};

type Payslip = {
  id: string;
  employee_id: string;
  gross_minor: number;
  deductions_minor: number;
  net_minor: number;
  status: string;
  currency: string;
  lines: PayslipLine[];
};

type PayrollRun = {
  id: string;
  status: string;
  period_start: string;
  period_end: string;
  currency: string;
  employee_count: number;
  gross_minor: number;
  deductions_minor: number;
  net_minor: number;
  journal_public_id: string | null;
  adjustment_of_run_id: string | null;
  version: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; run: PayrollRun; payslips: Payslip[] };

function money(minor: number, currency: string) {
  return `${(minor / 100).toFixed(2)} ${currency}`;
}

function basisText(basis: Record<string, unknown>) {
  try {
    return JSON.stringify(basis);
  } catch {
    return '—';
  }
}

export default function PayrollRunDetailPage() {
  const params = useParams<{ id: string }>();
  const runId = params.id;
  const { can, loading: capsLoading } = useCapabilities();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [selectedSlip, setSelectedSlip] = useState<Payslip | null>(null);

  const canRead = can('hr.payroll.read');
  const canRun = can('hr.payroll.run');
  const canApprove = can('hr.payroll.approve');
  const canWrite = can('hr.payroll.write');

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const [runRes, slipsRes] = await Promise.all([
      authFetch(`/api/v1/people/payroll/runs/${runId}`),
      authFetch(`/api/v1/people/payroll/runs/${runId}/payslips`),
    ]);
    const requestId = runRes.headers.get('x-request-id') ?? undefined;
    if (runRes.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (runRes.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!runRes.ok) {
      setState({ status: 'error', message: 'Could not load payroll run.', requestId });
      return;
    }
    const run = (await runRes.json()) as PayrollRun;
    const slipsBody = slipsRes.ok
      ? ((await slipsRes.json()) as { items: Payslip[] })
      : { items: [] as Payslip[] };
    setState({ status: 'ready', run, payslips: slipsBody.items ?? [] });
  }, [runId]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  async function postAction(path: string, needsIdempotency: boolean) {
    setActionError(null);
    setBusy(true);
    const headers: Record<string, string> = { 'content-type': 'application/json' };
    if (needsIdempotency) {
      headers['idempotency-key'] = crypto.randomUUID();
    }
    const res = await authFetch(path, { method: 'POST', headers, body: '{}' });
    setBusy(false);
    if (!res.ok) {
      setActionError(`Action failed (${res.status}).`);
      return;
    }
    await load();
  }

  if (capsLoading || state.status === 'loading') {
    return (
      <main style={page}>
        <LoadingState label="Loading run…" />
      </main>
    );
  }
  if (state.status === 'signed_out') {
    return (
      <main style={page}>
        <EmptyState title="Sign in required" description="Sign in to view payroll." />
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
        <ErrorState title="Payroll run" message={state.message} requestId={state.requestId} />
      </main>
    );
  }

  const { run, payslips } = state;

  return (
    <main style={page}>
      <p style={{ marginBottom: 8 }}>
        <Link href="/people/payroll">← Payroll</Link>
      </p>
      <header style={{ marginBottom: 16 }}>
        <h1 style={{ margin: 0, fontSize: 28 }}>
          {run.period_start} → {run.period_end}
        </h1>
        <p style={{ margin: '8px 0 0', display: 'flex', gap: 12, alignItems: 'center' }}>
          <StatusCell status={run.status} />
          <span>
            Gross {money(run.gross_minor, run.currency)} · Net {money(run.net_minor, run.currency)}
          </span>
          {run.journal_public_id ? <span>Journal {run.journal_public_id}</span> : null}
        </p>
      </header>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 24 }}>
        {canRun && (run.status === 'draft' || run.status === 'calculated') ? (
          <Button
            type="button"
            disabled={busy}
            onClick={() => postAction(`/api/v1/people/payroll/runs/${run.id}/calculate`, true)}
          >
            Calculate
          </Button>
        ) : null}
        {canWrite && run.status === 'calculated' ? (
          <Button
            type="button"
            disabled={busy}
            onClick={() => postAction(`/api/v1/people/payroll/runs/${run.id}/submit`, false)}
          >
            Submit for review
          </Button>
        ) : null}
        {canApprove && (run.status === 'calculated' || run.status === 'in_review') ? (
          <Button
            type="button"
            disabled={busy}
            onClick={() => postAction(`/api/v1/people/payroll/runs/${run.id}/approve`, true)}
          >
            Approve
          </Button>
        ) : null}
        {canRun && run.status === 'approved' ? (
          <Button
            type="button"
            disabled={busy}
            onClick={() => postAction(`/api/v1/people/payroll/runs/${run.id}/pay`, true)}
          >
            Pay &amp; post journal
          </Button>
        ) : null}
        {canWrite && (run.status === 'approved' || run.status === 'paid') ? (
          <Button
            type="button"
            disabled={busy}
            onClick={() => postAction(`/api/v1/people/payroll/runs/${run.id}/adjust`, false)}
          >
            Create adjustment
          </Button>
        ) : null}
        {(run.status === 'approved' || run.status === 'paid' || run.status === 'calculated') ? (
          <Button
            type="button"
            disabled={busy}
            onClick={async () => {
              const res = await authFetch(`/api/v1/people/payroll/runs/${run.id}/export`);
              if (!res.ok) {
                setActionError('Export failed.');
                return;
              }
              const text = await res.text();
              const blob = new Blob([text], { type: 'text/csv' });
              const url = URL.createObjectURL(blob);
              const a = document.createElement('a');
              a.href = url;
              a.download = `payroll-${run.id}.csv`;
              a.click();
              URL.revokeObjectURL(url);
            }}
          >
            Export payment CSV
          </Button>
        ) : null}
      </div>
      {actionError ? (
        <p role="alert" style={{ color: 'crimson' }}>
          {actionError}
        </p>
      ) : null}

      <h2 style={{ fontSize: 18 }}>Payslips</h2>
      {payslips.length === 0 ? (
        <EmptyState title="No payslips" description="Calculate the run to generate payslips." />
      ) : (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            {
              key: 'employee',
              header: 'Employee',
              cell: (r) => (
                <button type="button" style={linkBtn} onClick={() => setSelectedSlip(r)}>
                  {r.employee_id}
                </button>
              ),
            },
            {
              key: 'gross',
              header: 'Gross',
              cell: (r) => money(r.gross_minor, r.currency),
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
          ]}
          rows={payslips}
        />
      )}

      {selectedSlip ? (
        <section style={slipPanel} aria-label="Payslip detail">
          <h3 style={{ marginTop: 0 }}>Payslip {selectedSlip.id}</h3>
          <p>
            Gross {money(selectedSlip.gross_minor, selectedSlip.currency)} · Deductions{' '}
            {money(selectedSlip.deductions_minor, selectedSlip.currency)} · Net{' '}
            {money(selectedSlip.net_minor, selectedSlip.currency)}
          </p>
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
                header: 'Calculation basis',
                cell: (l) => <code style={{ fontSize: 12 }}>{basisText(l.calculation_basis)}</code>,
              },
            ]}
            rows={selectedSlip.lines}
          />
          <Button type="button" onClick={() => setSelectedSlip(null)}>
            Close
          </Button>
        </section>
      ) : null}
    </main>
  );
}

const page: CSSProperties = { padding: 24, maxWidth: 1100 };
const slipPanel: CSSProperties = {
  marginTop: 24,
  padding: 16,
  borderTop: '1px solid var(--color-border, #ddd)',
};
const linkBtn: CSSProperties = {
  background: 'none',
  border: 'none',
  color: 'var(--color-accent, #0b5)',
  cursor: 'pointer',
  padding: 0,
  textDecoration: 'underline',
};
