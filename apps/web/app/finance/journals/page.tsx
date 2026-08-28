'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type JournalLine = {
  account_code: string;
  debit_minor: number;
  credit_minor: number;
  memo?: string | null;
};

type Journal = {
  id: string;
  memo: string;
  source_type: string;
  source_id: string;
  currency: string;
  lines: JournalLine[];
  entry_date: string;
  period_id?: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Journal[] };

function toMinor(raw: string): number {
  return Math.round(parseFloat(raw || '0') * 100);
}

export default function JournalsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [memo, setMemo] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [debitAccount, setDebitAccount] = useState('5100');
  const [creditAccount, setCreditAccount] = useState('1000');
  const [amount, setAmount] = useState('10.00');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/journals?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load journals.' });
      return;
    }
    const body = (await res.json()) as { items: Journal[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      const amountMinor = toMinor(amount);
      if (
        !debitAccount.trim() ||
        !creditAccount.trim() ||
        Number.isNaN(amountMinor) ||
        amountMinor <= 0
      ) {
        setFormError('Account codes and a positive amount are required.');
        return;
      }
      const res = await authFetch('/api/v1/finance/journals', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          source_type: 'manual',
          currency,
          memo: memo.trim() || null,
          lines: [
            {
              account_code: debitAccount.trim(),
              debit_minor: amountMinor,
              credit_minor: 0,
              memo: memo.trim() || null,
            },
            {
              account_code: creditAccount.trim(),
              debit_minor: 0,
              credit_minor: amountMinor,
              memo: memo.trim() || null,
            },
          ],
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not post journal.');
        return;
      }
      setMemo('');
      setAmount('10.00');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading journals…" />;
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
    return <PermissionDeniedState requiredPermission="finance.ledger.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Journals unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Journals</h1>
          <p style={muted}>Posted ledger entries. Manual journals must balance.</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Back
          </Button>
        </Link>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Post manual journal</h2>
        <label style={labelStyle}>
          Memo
          <input value={memo} onChange={(e) => setMemo(e.target.value)} style={inputStyle} />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Debit account
            <input
              value={debitAccount}
              onChange={(e) => setDebitAccount(e.target.value)}
              style={inputStyle}
              required
            />
          </label>
          <label style={labelStyle}>
            Credit account
            <input
              value={creditAccount}
              onChange={(e) => setCreditAccount(e.target.value)}
              style={inputStyle}
              required
            />
          </label>
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Amount
            <input
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              style={inputStyle}
              inputMode="decimal"
              required
            />
          </label>
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
        </div>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Posting…' : 'Post journal'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No journals" description="Posted journals appear here." />
      ) : (
        <Table
          getRowKey={(r: Journal) => r.id}
          columns={[
            { key: 'date', header: 'Date', cell: (r: Journal) => r.entry_date },
            { key: 'memo', header: 'Memo', cell: (r: Journal) => r.memo },
            {
              key: 'source',
              header: 'Source',
              cell: (r: Journal) => <Badge>{r.source_type}</Badge>,
            },
            {
              key: 'lines',
              header: 'Lines',
              cell: (r: Journal) =>
                (r.lines ?? [])
                  .map((l) => {
                    const side =
                      l.debit_minor > 0
                        ? `Dr ${l.debit_minor / 100}`
                        : `Cr ${l.credit_minor / 100}`;
                    return `${l.account_code} ${side}`;
                  })
                  .join('; '),
            },
            {
              key: 'amount',
              header: 'Debits',
              align: 'right',
              cell: (r: Journal) => {
                const total = (r.lines ?? []).reduce((s, l) => s + l.debit_minor, 0);
                return <MoneyCell amount={total / 100} currency={r.currency} />;
              },
            },
          ]}
          rows={state.items}
        />
      )}
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
