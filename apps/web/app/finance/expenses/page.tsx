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
import { apiUrl, authFetch, getAccessToken } from '../../../lib/auth-client';

type Expense = {
  id: string;
  status: string;
  currency: string;
  amount_minor: number;
  description: string;
  incurred_at: string;
  receipt_url?: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Expense[] };

export default function ExpensesPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [description, setDescription] = useState('');
  const [amount, setAmount] = useState('12.00');
  const [currency, setCurrency] = useState('USD');
  const [file, setFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/expenses?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load expenses.' });
      return;
    }
    const body = (await res.json()) as { items: Expense[] };
    setState({ status: 'ready', items: body.items ?? [] });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function uploadReceipt(selected: File): Promise<string | null> {
    const presign = await authFetch('/api/v1/files/presign-upload', {
      method: 'POST',
      body: JSON.stringify({
        filename: selected.name,
        content_type: selected.type || 'application/octet-stream',
        size_bytes: selected.size,
      }),
    });
    if (!presign.ok) return null;
    const body = await presign.json();
    const uploadUrl: string = body.upload_url;
    const fileId: string = body.file_id;
    const headers: Record<string, string> = body.headers ?? {};

    // Prefer absolute gateway path when local-upload URL is returned.
    const putUrl = uploadUrl.startsWith('http')
      ? uploadUrl.includes('/api/v1/files/local-upload/')
        ? apiUrl(`/api/v1/files/local-upload/${encodeURIComponent(fileId)}`)
        : uploadUrl
      : apiUrl(uploadUrl);

    const putHeaders = new Headers(headers);
    const token = getAccessToken();
    if (token && putUrl.includes('/api/v1/files/')) {
      putHeaders.set('Authorization', `Bearer ${token}`);
    }
    await fetch(putUrl, {
      method: 'PUT',
      headers: putHeaders,
      body: selected,
      credentials: 'include',
    });

    await authFetch(`/api/v1/files/${encodeURIComponent(fileId)}/complete`, {
      method: 'POST',
    });

    return apiUrl(`/api/v1/files/${encodeURIComponent(fileId)}`);
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      const amountMinor = Math.round(parseFloat(amount || '0') * 100);
      if (!description.trim() || Number.isNaN(amountMinor) || amountMinor <= 0) {
        setFormError('Description and a positive amount are required.');
        return;
      }
      let receiptUrl: string | null = null;
      if (file) {
        receiptUrl = await uploadReceipt(file);
        if (!receiptUrl) {
          setFormError('Receipt upload failed.');
          return;
        }
      }
      const res = await authFetch('/api/v1/finance/expenses', {
        method: 'POST',
        body: JSON.stringify({
          currency,
          amount_minor: amountMinor,
          description: description.trim(),
          receipt_url: receiptUrl,
          incurred_at: new Date().toISOString().slice(0, 10),
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not submit expense.');
        return;
      }
      setDescription('');
      setAmount('12.00');
      setFile(null);
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading expenses…" />;
  if (state.status === 'signed_out') {
    return (
      <EmptyState
        title="Sign in required"
        action={
          <Link href="/login" style={{ textDecoration: 'none' }}><Button type="button" variant="primary">Sign in</Button></Link>
        }
      />
    );
  }
  if (state.status === 'denied') return <PermissionDeniedState requiredPermission="finance.expense.read" />;
  if (state.status === 'error') {
    return <ErrorState title="Expenses unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Expenses</h1>
          <p style={muted}>
            <Link href="/finance/documents" style={{ color: 'var(--cos-color-accent)' }}>
              Document AI
            </Link>
          </p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}><Button type="button" variant="ghost">Back</Button></Link>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Submit expense</h2>
        <label style={labelStyle}>
          Description
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            style={inputStyle}
            required
          />
        </label>
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
        <label style={labelStyle}>
          Receipt (optional)
          <input
            type="file"
            accept="image/*,application/pdf"
            onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          />
        </label>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Submitting…' : 'Submit expense'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No expenses" description="Submitted expenses appear here." />
      ) : (
        <Table
          getRowKey={(r: Expense) => r.id}
          columns={[
            { key: 'description', header: 'Description', cell: (r: Expense) => r.description },
            { key: 'status', header: 'Status', cell: (r: Expense) => <Badge>{r.status}</Badge> },
            {
              key: 'amount',
              header: 'Amount',
              align: 'right',
              cell: (r: Expense) => (
                <MoneyCell amount={r.amount_minor / 100} currency={r.currency} />
              ),
            },
            { key: 'date', header: 'Incurred', cell: (r: Expense) => r.incurred_at },
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
const muted: CSSProperties = { margin: '0.25rem 0 0', fontSize: '0.85rem', color: 'var(--cos-color-fg-muted)' };
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
