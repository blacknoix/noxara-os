'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type Supplier = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  currency: string;
  payment_terms: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; items: Supplier[] };

export default function SuppliersPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [phone, setPhone] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [paymentTerms, setPaymentTerms] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/inventory/suppliers?limit=50');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load suppliers.' });
      return;
    }
    const body = (await res.json()) as { items: Supplier[] };
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
      if (!name.trim() || !currency.trim()) {
        setFormError('Name and currency are required.');
        return;
      }
      const res = await authFetch('/api/v1/inventory/suppliers', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({
          name: name.trim(),
          email: email.trim() || null,
          phone: phone.trim() || null,
          currency: currency.trim().toUpperCase(),
          payment_terms: paymentTerms.trim() || null,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create supplier.');
        return;
      }
      setName('');
      setEmail('');
      setPhone('');
      setPaymentTerms('');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading suppliers…" />;
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
    return <PermissionDeniedState requiredPermission="inventory.supplier.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Suppliers unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Inventory</p>
          <h1 style={title}>Suppliers</h1>
          <p style={muted}>Vendor master data for purchase orders.</p>
        </div>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create supplier</h2>
        <label style={labelStyle}>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} required />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Email
            <input value={email} onChange={(e) => setEmail(e.target.value)} style={inputStyle} />
          </label>
          <label style={labelStyle}>
            Phone
            <input value={phone} onChange={(e) => setPhone(e.target.value)} style={inputStyle} />
          </label>
        </div>
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
            Payment terms
            <input
              value={paymentTerms}
              onChange={(e) => setPaymentTerms(e.target.value)}
              style={inputStyle}
              placeholder="Net 30"
            />
          </label>
        </div>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Creating…' : 'Create supplier'}
        </Button>
      </form>

      {state.items.length === 0 ? (
        <EmptyState title="No suppliers" description="Create a supplier to procure from." />
      ) : (
        <Table
          getRowKey={(r: Supplier) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r: Supplier) => r.name },
            { key: 'email', header: 'Email', cell: (r: Supplier) => r.email ?? '—' },
            { key: 'phone', header: 'Phone', cell: (r: Supplier) => r.phone ?? '—' },
            { key: 'ccy', header: 'CCY', cell: (r: Supplier) => r.currency },
            {
              key: 'terms',
              header: 'Terms',
              cell: (r: Supplier) => r.payment_terms ?? '—',
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
  maxWidth: 520,
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
