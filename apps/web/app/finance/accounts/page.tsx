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

type LedgerAccount = {
  id: string;
  code: string;
  name: string;
  account_type: string;
  normal_balance: string;
  parent_id: string | null;
  is_active: boolean;
  description: string | null;
  sort_order: number;
};

type LedgerAccountNode = {
  account: LedgerAccount;
  children: LedgerAccountNode[];
};

type FlatRow = LedgerAccount & { depth: number };

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; rows: FlatRow[] };

const ACCOUNT_TYPES = ['asset', 'liability', 'equity', 'revenue', 'income', 'expense'] as const;

function flattenTree(nodes: LedgerAccountNode[], depth = 0): FlatRow[] {
  const out: FlatRow[] = [];
  for (const node of nodes) {
    out.push({ ...node.account, depth });
    if (node.children?.length) {
      out.push(...flattenTree(node.children, depth + 1));
    }
  }
  return out;
}

export default function ChartOfAccountsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [code, setCode] = useState('');
  const [name, setName] = useState('');
  const [accountType, setAccountType] = useState<string>('asset');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/accounts');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load chart of accounts.' });
      return;
    }
    const body = (await res.json()) as { roots?: LedgerAccountNode[]; items?: LedgerAccountNode[] };
    const roots = body.roots ?? body.items ?? [];
    setState({ status: 'ready', rows: flattenTree(roots) });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSubmitting(true);
    try {
      if (!code.trim() || !name.trim()) {
        setFormError('Code and name are required.');
        return;
      }
      const res = await authFetch('/api/v1/finance/accounts', {
        method: 'POST',
        body: JSON.stringify({
          code: code.trim(),
          name: name.trim(),
          account_type: accountType,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create account.');
        return;
      }
      setCode('');
      setName('');
      setAccountType('asset');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading accounts…" />;
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
    return <ErrorState title="Accounts unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Chart of accounts</h1>
          <p style={muted}>Ledger accounts used for journals and bank mapping.</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Back
          </Button>
        </Link>
      </header>

      <form onSubmit={(e) => void onSubmit(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Create account</h2>
        <label style={labelStyle}>
          Code
          <input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            style={inputStyle}
            required
          />
        </label>
        <label style={labelStyle}>
          Name
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            style={inputStyle}
            required
          />
        </label>
        <label style={labelStyle}>
          Type
          <select
            value={accountType}
            onChange={(e) => setAccountType(e.target.value)}
            style={inputStyle}
          >
            {ACCOUNT_TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
        {formError ? <ErrorState message={formError} /> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Creating…' : 'Create account'}
        </Button>
      </form>

      {state.rows.length === 0 ? (
        <EmptyState title="No accounts" description="Create an account to get started." />
      ) : (
        <Table
          getRowKey={(r: FlatRow) => r.id}
          columns={[
            {
              key: 'code',
              header: 'Code',
              cell: (r: FlatRow) => (
                <span style={{ paddingLeft: `${r.depth * 1.25}rem` }}>{r.code}</span>
              ),
            },
            { key: 'name', header: 'Name', cell: (r: FlatRow) => r.name },
            { key: 'type', header: 'Type', cell: (r: FlatRow) => r.account_type },
            {
              key: 'active',
              header: 'Active',
              cell: (r: FlatRow) => (
                <Badge tone={r.is_active ? 'success' : 'neutral'}>
                  {r.is_active ? 'active' : 'inactive'}
                </Badge>
              ),
            },
          ]}
          rows={state.rows}
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
