'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type BankAccount = {
  id: string;
  name: string;
  currency: string;
  ledger_account_id: string;
  account_number_mask?: string | null;
  institution?: string | null;
  is_active: boolean;
};

type ImportStatementResponse = {
  statement: { id: string; bank_account_id: string; line_count: number };
  lines_imported: number;
};

type ReconcileResponse = {
  matched: number;
  unmatched: number;
  match_rate: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: BankAccount[] };

export default function BankPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [name, setName] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [ledgerCode, setLedgerCode] = useState('1000');
  const [selectedId, setSelectedId] = useState('');
  const [csv, setCsv] = useState('txn_date,amount,reference,description\n2026-03-01,100.00,PAY-1,Sample inflow');
  const [lastStatementId, setLastStatementId] = useState<string | null>(null);
  const [matchRate, setMatchRate] = useState<number | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/bank/accounts');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load bank accounts.' });
      return;
    }
    const items = (await res.json()) as BankAccount[];
    const list = Array.isArray(items) ? items : [];
    setState({ status: 'ready', items: list });
    setSelectedId((prev) => prev || list[0]?.id || '');
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      if (!name.trim() || !ledgerCode.trim()) {
        setFormError('Name and ledger account code are required.');
        return;
      }
      const res = await authFetch('/api/v1/finance/bank/accounts', {
        method: 'POST',
        body: JSON.stringify({
          name: name.trim(),
          currency,
          ledger_account_id: ledgerCode.trim(),
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create bank account.');
        return;
      }
      const created = (await res.json()) as BankAccount;
      setName('');
      setSelectedId(created.id);
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onImport() {
    setFormError(null);
    setActionMsg(null);
    setMatchRate(null);
    if (!selectedId) {
      setFormError('Select a bank account first.');
      return;
    }
    if (!csv.trim()) {
      setFormError('CSV content is required.');
      return;
    }
    setSubmitting(true);
    try {
      const res = await authFetch(
        `/api/v1/finance/bank/accounts/${encodeURIComponent(selectedId)}/statements/import`,
        {
          method: 'POST',
          headers: { 'Idempotency-Key': crypto.randomUUID() },
          body: JSON.stringify({ csv }),
        },
      );
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not import statement.');
        return;
      }
      const body = (await res.json()) as ImportStatementResponse;
      setLastStatementId(body.statement?.id ?? null);
      setActionMsg(`Imported ${body.lines_imported} line(s).`);
    } finally {
      setSubmitting(false);
    }
  }

  async function onAutoMatch() {
    setFormError(null);
    setActionMsg(null);
    if (!lastStatementId) {
      setFormError('Import a statement first, then auto-match.');
      return;
    }
    setSubmitting(true);
    try {
      const res = await authFetch(
        `/api/v1/finance/bank/statements/${encodeURIComponent(lastStatementId)}/auto-match`,
        {
          method: 'POST',
          body: JSON.stringify({}),
        },
      );
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Auto-match failed.');
        return;
      }
      const body = (await res.json()) as ReconcileResponse;
      setMatchRate(body.match_rate);
      setActionMsg(`Matched ${body.matched}, unmatched ${body.unmatched}.`);
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading bank accounts…" />;
  if (state.status === 'signed_out') {
    return (
      <EmptyState
        title="Sign in required"
        action={
          <Link href="/login" style={{ textDecoration: 'none' }}>
            <Button type="button" variant="primary">
              Sign in
            </Button>
          </Link>
        }
      />
    );
  }
  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="finance.bank.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Bank unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Bank</h1>
          <p style={muted}>Bank accounts, CSV import, and auto-match reconciliation.</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Back
          </Button>
        </Link>
      </header>

      <form onSubmit={(e) => void onCreate(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create bank account</h2>
        <label style={labelStyle}>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} required />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Currency
            <input
              value={currency}
              onChange={(e) => setCurrency(e.target.value.toUpperCase())}
              style={{ ...inputStyle, width: 88 }}
              maxLength={3}
              required
            />
          </label>
          <label style={labelStyle}>
            Ledger account code
            <input
              value={ledgerCode}
              onChange={(e) => setLedgerCode(e.target.value)}
              style={inputStyle}
              placeholder="1000"
              required
            />
          </label>
        </div>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Saving…' : 'Create account'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No bank accounts" description="Create a bank account to import statements." />
      ) : (
        <Table
          getRowKey={(r: BankAccount) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r: BankAccount) => r.name },
            { key: 'currency', header: 'Currency', cell: (r: BankAccount) => r.currency },
            {
              key: 'ledger',
              header: 'Ledger account',
              cell: (r: BankAccount) => r.ledger_account_id,
            },
            {
              key: 'active',
              header: 'Active',
              cell: (r: BankAccount) => (
                <Badge tone={r.is_active ? 'success' : 'neutral'}>
                  {r.is_active ? 'active' : 'inactive'}
                </Badge>
              ),
            },
          ]}
          rows={state.items}
        />
      )}

      <div style={{ ...formStyle, marginTop: '1.5rem', maxWidth: 640 }}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Import statement CSV</h2>
        <label style={labelStyle}>
          Bank account
          <select
            value={selectedId}
            onChange={(e) => setSelectedId(e.target.value)}
            style={inputStyle}
          >
            <option value="">Select…</option>
            {state.items.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name} ({a.currency})
              </option>
            ))}
          </select>
        </label>
        <label style={labelStyle}>
          CSV
          <textarea
            value={csv}
            onChange={(e) => setCsv(e.target.value)}
            rows={6}
            style={{ ...inputStyle, fontFamily: 'var(--cos-font-mono, monospace)', resize: 'vertical' }}
          />
        </label>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
          <Button type="button" variant="secondary" disabled={submitting} onClick={() => void onImport()}>
            Import CSV
          </Button>
          <Button type="button" variant="primary" disabled={submitting} onClick={() => void onAutoMatch()}>
            Auto-match
          </Button>
          {matchRate !== null ? (
            <Badge tone="info">Match rate {(matchRate * 100).toFixed(0)}%</Badge>
          ) : null}
        </div>
        {actionMsg ? <p style={muted}>{actionMsg}</p> : null}
        {lastStatementId ? (
          <p style={muted}>Last statement: {lastStatementId}</p>
        ) : null}
      </div>
    </section>
  );
}

const headerStyle: CSSProperties = {
  marginBottom: '1.25rem',
  display: 'flex',
  justifyContent: 'space-between',
  alignItems: 'flex-end',
};
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = {
  margin: '0.25rem 0 0',
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const formStyle: CSSProperties = {
  display: 'grid',
  gap: 12,
  marginBottom: '1.5rem',
  maxWidth: 480,
};
const labelStyle: CSSProperties = {
  display: 'grid',
  gap: 4,
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const inputStyle: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.45rem 0.6rem',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
