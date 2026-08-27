'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type ExpensePolicy = {
  id: string;
  name: string;
  is_active: boolean;
  require_receipt_over_minor: number;
  auto_approve_under_minor: number;
  over_limit_action: string;
  mileage_unit: string;
  mileage_rate_minor: number;
  per_diem_minor: number;
  category_limits: Array<{ category_code: string; max_amount_minor: number; currency: string }>;
};

type MileageResponse = {
  amount_minor: number;
  currency: string;
  rate_minor: number;
  miles_or_km: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; policy: ExpensePolicy | null };

function minorToMajor(minor: number): string {
  return (minor / 100).toFixed(2);
}

function majorToMinor(raw: string): number {
  return Math.round(parseFloat(raw || '0') * 100);
}

export default function ExpensePolicyPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [mileageRate, setMileageRate] = useState('0.65');
  const [perDiem, setPerDiem] = useState('75.00');
  const [autoApproveUnder, setAutoApproveUnder] = useState('25.00');
  const [overLimitAction, setOverLimitAction] = useState('require_approval');
  const [mileageUnit, setMileageUnit] = useState('mile');
  const [policyName, setPolicyName] = useState('Default');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);

  const [miles, setMiles] = useState('10');
  const [mileageCurrency, setMileageCurrency] = useState('USD');
  const [mileageResult, setMileageResult] = useState<MileageResponse | null>(null);

  const [expenseIds, setExpenseIds] = useState('');
  const [batchMsg, setBatchMsg] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/expense-policies');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (res.status === 404) {
      setState({ status: 'ready', policy: null });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load expense policy.' });
      return;
    }
    const policy = (await res.json()) as ExpensePolicy;
    setMileageRate(minorToMajor(policy.mileage_rate_minor));
    setPerDiem(minorToMajor(policy.per_diem_minor));
    setAutoApproveUnder(minorToMajor(policy.auto_approve_under_minor));
    setOverLimitAction(policy.over_limit_action);
    setMileageUnit(policy.mileage_unit);
    setPolicyName(policy.name);
    setState({ status: 'ready', policy });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSave(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSaveMsg(null);
    setSubmitting(true);
    try {
      const res = await authFetch('/api/v1/finance/expense-policies', {
        method: 'PUT',
        body: JSON.stringify({
          name: policyName.trim() || 'Default',
          mileage_rate_minor: majorToMinor(mileageRate),
          per_diem_minor: majorToMinor(perDiem),
          auto_approve_under_minor: majorToMinor(autoApproveUnder),
          over_limit_action: overLimitAction,
          mileage_unit: mileageUnit,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not save policy.');
        return;
      }
      setSaveMsg('Policy saved.');
      await load();
    } finally {
      setSubmitting(false);
    }
  }

  async function onMileage(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setMileageResult(null);
    setSubmitting(true);
    try {
      const milesOrKm = parseFloat(miles || '0');
      if (Number.isNaN(milesOrKm) || milesOrKm <= 0) {
        setFormError('Enter a positive distance.');
        return;
      }
      const res = await authFetch('/api/v1/finance/expenses/mileage', {
        method: 'POST',
        body: JSON.stringify({
          miles_or_km: milesOrKm,
          currency: mileageCurrency,
          description: `Mileage ${milesOrKm} ${mileageUnit}`,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Mileage submit failed.');
        return;
      }
      setMileageResult((await res.json()) as MileageResponse);
    } finally {
      setSubmitting(false);
    }
  }

  async function onCreateBatch(e: FormEvent) {
    e.preventDefault();
    setBatchMsg(null);
    setFormError(null);
    const ids = expenseIds
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    if (ids.length === 0) {
      setFormError('Enter one or more expense ids separated by commas.');
      return;
    }
    setSubmitting(true);
    try {
      const res = await authFetch('/api/v1/finance/reimbursements', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({ expense_ids: ids }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setFormError(body.detail ?? 'Could not create reimbursement batch.');
        return;
      }
      const body = (await res.json()) as { id: string; total_minor: number; currency: string };
      setBatchMsg(`Batch ${body.id} created (${(body.total_minor / 100).toFixed(2)} ${body.currency}).`);
      setExpenseIds('');
    } finally {
      setSubmitting(false);
    }
  }

  if (state.status === 'loading') return <LoadingState label="Loading expense policy…" />;
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
    return <PermissionDeniedState requiredPermission="finance.expense_policy.manage" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Expense policy unavailable" message={state.message} />;
  }

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Expense policy</h1>
          <p style={muted}>
            {state.policy
              ? `Active policy: ${state.policy.name}`
              : 'No active policy yet — save to create one.'}
          </p>
        </div>
        <Link href="/finance/expenses" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Expenses
          </Button>
        </Link>
      </header>

      {formError ? (
        <div style={{ marginBottom: '1rem' }}>
          <ErrorState message={formError} />
        </div>
      ) : null}

      <form onSubmit={(e) => void onSave(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Policy settings</h2>
        <label style={labelStyle}>
          Name
          <input
            value={policyName}
            onChange={(e) => setPolicyName(e.target.value)}
            style={inputStyle}
          />
        </label>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Mileage rate
            <input
              value={mileageRate}
              onChange={(e) => setMileageRate(e.target.value)}
              style={inputStyle}
              inputMode="decimal"
            />
          </label>
          <label style={labelStyle}>
            Mileage unit
            <select
              value={mileageUnit}
              onChange={(e) => setMileageUnit(e.target.value)}
              style={inputStyle}
            >
              <option value="mile">mile</option>
              <option value="km">km</option>
            </select>
          </label>
        </div>
        <label style={labelStyle}>
          Per diem
          <input
            value={perDiem}
            onChange={(e) => setPerDiem(e.target.value)}
            style={inputStyle}
            inputMode="decimal"
          />
        </label>
        <label style={labelStyle}>
          Auto-approve under
          <input
            value={autoApproveUnder}
            onChange={(e) => setAutoApproveUnder(e.target.value)}
            style={inputStyle}
            inputMode="decimal"
          />
        </label>
        <label style={labelStyle}>
          Over-limit action
          <select
            value={overLimitAction}
            onChange={(e) => setOverLimitAction(e.target.value)}
            style={inputStyle}
          >
            <option value="require_approval">require_approval</option>
            <option value="reject">reject</option>
          </select>
        </label>
        {saveMsg ? <p style={muted}>{saveMsg}</p> : null}
        <Button type="submit" variant="primary" disabled={submitting}>
          {submitting ? 'Saving…' : 'Save policy'}
        </Button>
      </form>

      <form onSubmit={(e) => void onMileage(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Mileage calculator</h2>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <label style={labelStyle}>
            Distance ({mileageUnit})
            <input
              value={miles}
              onChange={(e) => setMiles(e.target.value)}
              style={inputStyle}
              inputMode="decimal"
              required
            />
          </label>
          <label style={labelStyle}>
            Currency
            <input
              value={mileageCurrency}
              onChange={(e) => setMileageCurrency(e.target.value.toUpperCase())}
              style={{ ...inputStyle, width: 88 }}
              maxLength={3}
            />
          </label>
        </div>
        <Button type="submit" variant="secondary" disabled={submitting}>
          Submit mileage expense
        </Button>
        {mileageResult ? (
          <p style={muted}>
            Created expense for{' '}
            <MoneyCell amount={mileageResult.amount_minor / 100} currency={mileageResult.currency} />{' '}
            at rate {minorToMajor(mileageResult.rate_minor)}.
          </p>
        ) : null}
      </form>

      <form onSubmit={(e) => void onCreateBatch(e)} style={formStyle}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Reimbursements</h2>
        <p style={muted}>
          Create a reimbursement batch from approved expense ids, or use{' '}
          <code>POST /api/v1/finance/reimbursements</code> directly.
        </p>
        <label style={labelStyle}>
          Expense ids (comma-separated)
          <input
            value={expenseIds}
            onChange={(e) => setExpenseIds(e.target.value)}
            style={inputStyle}
            placeholder="exp_…, exp_…"
          />
        </label>
        <Button type="submit" variant="secondary" disabled={submitting}>
          Create reimbursement batch
        </Button>
        {batchMsg ? <p style={muted}>{batchMsg}</p> : null}
      </form>
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
