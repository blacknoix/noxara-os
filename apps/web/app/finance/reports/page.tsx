'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
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
  Tabs,
  Widget,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type AgeingBucket = { label: string; amount_minor: number };
type CategoryAmount = { category: string; amount_minor: number };
type CashFlowPoint = { period: string; inflow_minor: number; outflow_minor: number };

type ReportSummary = {
  as_of: string;
  currency: string;
  revenue_minor: number;
  expenses_minor: number;
  cash_minor: number;
  receivables_minor: number;
  ageing: AgeingBucket[];
  expenses_by_category: CategoryAmount[];
  cash_flow: CashFlowPoint[];
};

type TrialBalanceRow = {
  account_code: string;
  account_name: string;
  account_type: string;
  debit_minor: number;
  credit_minor: number;
};

type TrialBalanceResponse = {
  currency: string;
  period_id: string | null;
  rows: TrialBalanceRow[];
  total_debit_minor: number;
  total_credit_minor: number;
  balanced: boolean;
};

type ReportLine = {
  account_code: string;
  account_name: string;
  amount_minor: number;
};

type ProfitAndLossResponse = {
  currency: string;
  from?: string | null;
  to?: string | null;
  period_id?: string | null;
  revenue: ReportLine[];
  expenses: ReportLine[];
  revenue_total_minor: number;
  expense_total_minor: number;
  net_income_minor: number;
};

type BalanceSheetResponse = {
  currency: string;
  as_of: string;
  period_id?: string | null;
  assets: ReportLine[];
  liabilities: ReportLine[];
  equity: ReportLine[];
  assets_total_minor: number;
  liabilities_total_minor: number;
  equity_total_minor: number;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string }
  | { status: 'ready'; report: ReportSummary };

type TabId = 'summary' | 'trial-balance' | 'pnl' | 'balance-sheet';

export default function FinanceReportsPage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [tab, setTab] = useState<TabId>('summary');
  const [tb, setTb] = useState<TrialBalanceResponse | null>(null);
  const [pnl, setPnl] = useState<ProfitAndLossResponse | null>(null);
  const [bs, setBs] = useState<BalanceSheetResponse | null>(null);
  const [ledgerError, setLedgerError] = useState<string | null>(null);
  const [ledgerLoading, setLedgerLoading] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/finance/reports/summary');
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      setState({ status: 'error', message: 'Could not load report.' });
      return;
    }
    setState({ status: 'ready', report: (await res.json()) as ReportSummary });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const loadLedgerReport = useCallback(async (id: TabId) => {
    if (id === 'summary') return;
    setLedgerLoading(true);
    setLedgerError(null);
    try {
      const path =
        id === 'trial-balance'
          ? '/api/v1/finance/reports/trial-balance'
          : id === 'pnl'
            ? '/api/v1/finance/reports/profit-and-loss'
            : '/api/v1/finance/reports/balance-sheet';
      const res = await authFetch(path);
      if (!res.ok) {
        setLedgerError('Could not load ledger report.');
        return;
      }
      const body = await res.json();
      if (id === 'trial-balance') setTb(body as TrialBalanceResponse);
      if (id === 'pnl') setPnl(body as ProfitAndLossResponse);
      if (id === 'balance-sheet') setBs(body as BalanceSheetResponse);
    } finally {
      setLedgerLoading(false);
    }
  }, []);

  useEffect(() => {
    if (state.status !== 'ready') return;
    void loadLedgerReport(tab);
  }, [tab, state.status, loadLedgerReport]);

  if (state.status === 'loading') return <LoadingState label="Loading reports…" />;
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
    return <PermissionDeniedState requiredPermission="finance.report.read" />;
  }
  if (state.status === 'error') {
    return <ErrorState title="Reports unavailable" message={state.message} />;
  }

  const r = state.report;
  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance</p>
          <h1 style={title}>Reports</h1>
          <p style={subtitle}>As of {r.as_of}</p>
        </div>
        <Link href="/finance" style={{ textDecoration: 'none' }}>
          <Button type="button" variant="ghost">
            Back
          </Button>
        </Link>
      </header>

      <Tabs
        items={[
          { id: 'summary', label: 'Summary' },
          { id: 'trial-balance', label: 'Trial balance' },
          { id: 'pnl', label: 'P&L' },
          { id: 'balance-sheet', label: 'Balance sheet' },
        ]}
        value={tab}
        onChange={(id) => setTab(id as TabId)}
      >
        {tab === 'summary' ? (
          <>
            <div style={statGrid}>
              <Widget title="Revenue">
                <MoneyCell amount={r.revenue_minor / 100} currency={r.currency} />
              </Widget>
              <Widget title="Expenses">
                <MoneyCell amount={r.expenses_minor / 100} currency={r.currency} />
              </Widget>
              <Widget title="Cash">
                <MoneyCell amount={r.cash_minor / 100} currency={r.currency} />
              </Widget>
              <Widget title="Receivables">
                <MoneyCell amount={r.receivables_minor / 100} currency={r.currency} />
              </Widget>
            </div>

            <h2 style={h2}>Receivables ageing</h2>
            {(r.ageing ?? []).length === 0 ? (
              <EmptyState title="No open receivables" />
            ) : (
              <Table
                getRowKey={(b: AgeingBucket) => b.label}
                columns={[
                  { key: 'label', header: 'Bucket', cell: (b: AgeingBucket) => b.label },
                  {
                    key: 'amount',
                    header: 'Amount',
                    align: 'right',
                    cell: (b: AgeingBucket) => (
                      <MoneyCell amount={b.amount_minor / 100} currency={r.currency} />
                    ),
                  },
                ]}
                rows={r.ageing}
              />
            )}

            <h2 style={h2}>Expenses by category</h2>
            {(r.expenses_by_category ?? []).length === 0 ? (
              <EmptyState title="No posted expenses" />
            ) : (
              <Table
                getRowKey={(c: CategoryAmount) => c.category}
                columns={[
                  { key: 'cat', header: 'Category', cell: (c: CategoryAmount) => c.category },
                  {
                    key: 'amount',
                    header: 'Amount',
                    align: 'right',
                    cell: (c: CategoryAmount) => (
                      <MoneyCell amount={c.amount_minor / 100} currency={r.currency} />
                    ),
                  },
                ]}
                rows={r.expenses_by_category}
              />
            )}

            <h2 style={h2}>Cash flow (simple)</h2>
            {(r.cash_flow ?? []).length === 0 ? (
              <EmptyState title="No cash movements yet" />
            ) : (
              <Table
                getRowKey={(p: CashFlowPoint) => p.period}
                columns={[
                  { key: 'period', header: 'Period', cell: (p: CashFlowPoint) => p.period },
                  {
                    key: 'in',
                    header: 'Inflow',
                    align: 'right',
                    cell: (p: CashFlowPoint) => (
                      <MoneyCell amount={p.inflow_minor / 100} currency={r.currency} />
                    ),
                  },
                  {
                    key: 'out',
                    header: 'Outflow',
                    align: 'right',
                    cell: (p: CashFlowPoint) => (
                      <MoneyCell amount={p.outflow_minor / 100} currency={r.currency} />
                    ),
                  },
                ]}
                rows={r.cash_flow}
              />
            )}
          </>
        ) : null}

        {tab === 'trial-balance' ? (
          ledgerLoading ? (
            <LoadingState label="Loading trial balance…" />
          ) : ledgerError ? (
            <ErrorState message={ledgerError} />
          ) : !tb ? (
            <EmptyState title="No trial balance data" />
          ) : (
            <>
              <div style={{ display: 'flex', gap: 12, alignItems: 'center', marginBottom: 12 }}>
                <Badge tone={tb.balanced ? 'success' : 'danger'}>
                  {tb.balanced ? 'Balanced' : 'Out of balance'}
                </Badge>
                <span style={subtitle}>
                  Debits{' '}
                  <MoneyCell amount={tb.total_debit_minor / 100} currency={tb.currency} /> · Credits{' '}
                  <MoneyCell amount={tb.total_credit_minor / 100} currency={tb.currency} />
                </span>
              </div>
              {(tb.rows ?? []).length === 0 ? (
                <EmptyState title="No ledger activity" />
              ) : (
                <Table
                  getRowKey={(row: TrialBalanceRow) => row.account_code}
                  columns={[
                    {
                      key: 'code',
                      header: 'Code',
                      cell: (row: TrialBalanceRow) => row.account_code,
                    },
                    {
                      key: 'name',
                      header: 'Account',
                      cell: (row: TrialBalanceRow) => row.account_name,
                    },
                    {
                      key: 'type',
                      header: 'Type',
                      cell: (row: TrialBalanceRow) => row.account_type,
                    },
                    {
                      key: 'debit',
                      header: 'Debit',
                      align: 'right',
                      cell: (row: TrialBalanceRow) => (
                        <MoneyCell amount={row.debit_minor / 100} currency={tb.currency} />
                      ),
                    },
                    {
                      key: 'credit',
                      header: 'Credit',
                      align: 'right',
                      cell: (row: TrialBalanceRow) => (
                        <MoneyCell amount={row.credit_minor / 100} currency={tb.currency} />
                      ),
                    },
                  ]}
                  rows={tb.rows}
                />
              )}
            </>
          )
        ) : null}

        {tab === 'pnl' ? (
          ledgerLoading ? (
            <LoadingState label="Loading P&L…" />
          ) : ledgerError ? (
            <ErrorState message={ledgerError} />
          ) : !pnl ? (
            <EmptyState title="No P&L data" />
          ) : (
            <>
              <div style={statGrid}>
                <Widget title="Revenue">
                  <MoneyCell amount={pnl.revenue_total_minor / 100} currency={pnl.currency} />
                </Widget>
                <Widget title="Expenses">
                  <MoneyCell amount={pnl.expense_total_minor / 100} currency={pnl.currency} />
                </Widget>
                <Widget title="Net income">
                  <MoneyCell amount={pnl.net_income_minor / 100} currency={pnl.currency} />
                </Widget>
              </div>
              <h2 style={h2}>Revenue</h2>
              {(pnl.revenue ?? []).length === 0 ? (
                <EmptyState title="No revenue lines" />
              ) : (
                <Table
                  getRowKey={(line: ReportLine) => `rev-${line.account_code}`}
                  columns={[
                    { key: 'code', header: 'Code', cell: (line: ReportLine) => line.account_code },
                    { key: 'name', header: 'Account', cell: (line: ReportLine) => line.account_name },
                    {
                      key: 'amount',
                      header: 'Amount',
                      align: 'right',
                      cell: (line: ReportLine) => (
                        <MoneyCell amount={line.amount_minor / 100} currency={pnl.currency} />
                      ),
                    },
                  ]}
                  rows={pnl.revenue}
                />
              )}
              <h2 style={h2}>Expenses</h2>
              {(pnl.expenses ?? []).length === 0 ? (
                <EmptyState title="No expense lines" />
              ) : (
                <Table
                  getRowKey={(line: ReportLine) => `exp-${line.account_code}`}
                  columns={[
                    { key: 'code', header: 'Code', cell: (line: ReportLine) => line.account_code },
                    { key: 'name', header: 'Account', cell: (line: ReportLine) => line.account_name },
                    {
                      key: 'amount',
                      header: 'Amount',
                      align: 'right',
                      cell: (line: ReportLine) => (
                        <MoneyCell amount={line.amount_minor / 100} currency={pnl.currency} />
                      ),
                    },
                  ]}
                  rows={pnl.expenses}
                />
              )}
            </>
          )
        ) : null}

        {tab === 'balance-sheet' ? (
          ledgerLoading ? (
            <LoadingState label="Loading balance sheet…" />
          ) : ledgerError ? (
            <ErrorState message={ledgerError} />
          ) : !bs ? (
            <EmptyState title="No balance sheet data" />
          ) : (
            <>
              <p style={subtitle}>As of {bs.as_of}</p>
              <div style={statGrid}>
                <Widget title="Assets">
                  <MoneyCell amount={bs.assets_total_minor / 100} currency={bs.currency} />
                </Widget>
                <Widget title="Liabilities">
                  <MoneyCell amount={bs.liabilities_total_minor / 100} currency={bs.currency} />
                </Widget>
                <Widget title="Equity">
                  <MoneyCell amount={bs.equity_total_minor / 100} currency={bs.currency} />
                </Widget>
              </div>
              {(
                [
                  ['Assets', bs.assets],
                  ['Liabilities', bs.liabilities],
                  ['Equity', bs.equity],
                ] as const
              ).map(([label, lines]) => (
                <div key={label}>
                  <h2 style={h2}>{label}</h2>
                  {lines.length === 0 ? (
                    <EmptyState title={`No ${label.toLowerCase()} lines`} />
                  ) : (
                    <Table
                      getRowKey={(line: ReportLine) => `${label}-${line.account_code}`}
                      columns={[
                        {
                          key: 'code',
                          header: 'Code',
                          cell: (line: ReportLine) => line.account_code,
                        },
                        {
                          key: 'name',
                          header: 'Account',
                          cell: (line: ReportLine) => line.account_name,
                        },
                        {
                          key: 'amount',
                          header: 'Amount',
                          align: 'right',
                          cell: (line: ReportLine) => (
                            <MoneyCell amount={line.amount_minor / 100} currency={bs.currency} />
                          ),
                        },
                      ]}
                      rows={lines}
                    />
                  )}
                </div>
              ))}
            </>
          )
        ) : null}
      </Tabs>
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
const subtitle: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const h2: CSSProperties = { fontSize: '1.05rem', margin: '1.5rem 0 0.75rem' };
const statGrid: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(11rem, 1fr))',
  gap: '0.75rem',
};
