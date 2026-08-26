'use client';

import {
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
} from 'react';
import Link from 'next/link';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  EmptyState,
  ErrorState,
  FilterBar,
  type FilterClause,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  Select,
  StatusCell,
  Table,
  type SortDir,
  type TableDensity,
  parseViewFromSearchParams,
  viewToSearchParams,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Deal = {
  id: string;
  stage_id: string;
  customer_id: string | null;
  name: string;
  amount_minor: number;
  currency: string;
  expected_close_date: string | null;
  owner_user_id: string | null;
  status: string;
};

const STATUS_OPTIONS = [
  { value: '', label: 'Any status' },
  { value: 'open', label: 'Open' },
  { value: 'won', label: 'Won' },
  { value: 'lost', label: 'Lost' },
];

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'won':
      return 'success';
    case 'lost':
      return 'danger';
    case 'open':
      return 'info';
    default:
      return 'neutral';
  }
}

function DealsPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // Seed once from URL on first paint
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [deals, setDeals] = useState<Deal[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [q, setQ] = useState(initialView.q ?? searchParams?.get('q') ?? '');
  const [filters, setFilters] = useState<FilterClause[]>(initialView.filters ?? []);
  const [sortKey, setSortKey] = useState(initialView.sort?.key ?? 'name');
  const [sortDir, setSortDir] = useState<SortDir>(initialView.sort?.dir ?? 'asc');
  const [density, setDensity] = useState<TableDensity>(initialView.density ?? 'comfortable');
  const [hiddenColumns, setHiddenColumns] = useState<string[]>(initialView.hiddenColumns ?? []);
  const [stageNames, setStageNames] = useState<Record<string, string>>({});

  const syncUrl = useCallback(
    (next: {
      q?: string;
      filters?: FilterClause[];
      sortKey?: string;
      sortDir?: SortDir;
      density?: TableDensity;
      hiddenColumns?: string[];
    }) => {
      const params = viewToSearchParams({
        q: next.q ?? q,
        filters: next.filters ?? filters,
        sort: { key: next.sortKey ?? sortKey, dir: next.sortDir ?? sortDir },
        density: next.density ?? density,
        hiddenColumns: next.hiddenColumns ?? hiddenColumns,
      });
      const qs = params.toString();
      router.replace(qs ? `/sales/deals?${qs}` : '/sales/deals', { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns],
  );

  const loadDeals = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/sales/deals?limit=200');
    const rid = res.headers.get('x-request-id') ?? undefined;
    setRequestId(rid);
    if (res.status === 401) {
      setError('Sign in required');
      setLoading(false);
      return;
    }
    if (res.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!res.ok) {
      setError('Could not load deals');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setDeals(body.items ?? []);
    setLoading(false);
  }, []);

  const loadStageNames = useCallback(async () => {
    if (!getAccessToken()) return;
    const res = await authFetch('/api/v1/sales/pipelines/default/board');
    if (!res.ok) return;
    const body = await res.json();
    const map: Record<string, string> = {};
    for (const s of body.stages ?? []) {
      map[s.stage.id] = s.stage.name;
    }
    setStageNames(map);
  }, []);

  useEffect(() => {
    void loadDeals();
    void loadStageNames();
  }, [loadDeals, loadStageNames]);

  useEffect(() => {
    const fromUrl = parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? ''));
    if (fromUrl.q != null) setQ(fromUrl.q);
    if (fromUrl.filters) setFilters(fromUrl.filters);
  }, [searchParams]);

  const statusFilter = filters.find((f) => f.field === 'status' && f.operator === 'is');

  const filtered = useMemo(() => {
    let rows = [...deals];
    const needle = q.trim().toLowerCase();
    if (needle) {
      rows = rows.filter((d) => d.name.toLowerCase().includes(needle));
    }
    if (statusFilter?.value && typeof statusFilter.value === 'string') {
      rows = rows.filter((d) => d.status === statusFilter.value);
    }
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      if (sortKey === 'amount') return (a.amount_minor - b.amount_minor) * dir;
      if (sortKey === 'status') return a.status.localeCompare(b.status) * dir;
      if (sortKey === 'expected_close_date') {
        return (a.expected_close_date ?? '').localeCompare(b.expected_close_date ?? '') * dir;
      }
      return a.name.localeCompare(b.name) * dir;
    });
    return rows;
  }, [deals, q, statusFilter, sortKey, sortDir]);

  function setStatusFilter(value: string) {
    const rest = filters.filter((f) => f.field !== 'status');
    const next = value
      ? [...rest, { id: 'status-is', field: 'status', operator: 'is' as const, value, label: 'Status' }]
      : rest;
    setFilters(next);
    syncUrl({ filters: next });
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view deals." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('sales.deal.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="sales.deal.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={h1}>Deals</h1>
        <p style={muted}>
          All deals visible to you.{' '}
          <Link href="/sales" style={{ color: 'var(--cos-color-accent)' }}>
            View pipeline board
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <FilterBar
        q={q}
        onQueryChange={(next) => {
          setQ(next);
          syncUrl({ q: next });
        }}
        searchPlaceholder="Search deal name…"
        filters={filters}
        onFiltersChange={(next) => {
          setFilters(next);
          syncUrl({ filters: next });
        }}
        onClearAll={() => {
          setQ('');
          setFilters([]);
          syncUrl({ q: '', filters: [] });
        }}
        onSaveView={() => {
          syncUrl({});
        }}
      >
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.75rem', alignItems: 'flex-end' }}>
          <div style={{ minWidth: 140 }}>
            <Select
              label="Status is"
              value={typeof statusFilter?.value === 'string' ? statusFilter.value : ''}
              onChange={(e) => setStatusFilter(e.target.value)}
              options={STATUS_OPTIONS}
            />
          </div>
          <div style={{ minWidth: 140 }}>
            <Select
              label="Density"
              value={density}
              onChange={(e) => {
                const next = e.target.value as TableDensity;
                setDensity(next);
                syncUrl({ density: next });
              }}
              options={[
                { value: 'compact', label: 'Compact' },
                { value: 'comfortable', label: 'Comfortable' },
                { value: 'spacious', label: 'Spacious' },
              ]}
            />
          </div>
        </div>
      </FilterBar>

      {loading ? (
        <LoadingState label="Loading deals" rows={4} />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No deals match"
          description={
            deals.length === 0 ? 'Deals appear here once created.' : 'Try clearing filters or search.'
          }
        />
      ) : (
        <Table
          density={density}
          sortKey={sortKey}
          sortDir={sortDir}
          onSortChange={(key, dir) => {
            setSortKey(key);
            setSortDir(dir);
            syncUrl({ sortKey: key, sortDir: dir });
          }}
          hiddenColumns={hiddenColumns}
          onHiddenColumnsChange={(keys) => {
            setHiddenColumns(keys);
            syncUrl({ hiddenColumns: keys });
          }}
          getRowKey={(d) => d.id}
          columns={[
            {
              key: 'name',
              header: 'Name',
              sortable: true,
              hideable: true,
              cell: (d: Deal) =>
                d.customer_id && can('sales.customer.read') ? (
                  <Link href={`/sales/customers/${d.customer_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                    {d.name}
                  </Link>
                ) : (
                  d.name
                ),
            },
            {
              key: 'amount',
              header: 'Amount',
              sortable: true,
              hideable: true,
              align: 'right',
              cell: (d: Deal) => <MoneyCell amount={d.amount_minor / 100} currency={d.currency} />,
            },
            {
              key: 'stage',
              header: 'Stage',
              hideable: true,
              cell: (d: Deal) => stageNames[d.stage_id] ?? d.stage_id,
            },
            {
              key: 'status',
              header: 'Status',
              sortable: true,
              hideable: true,
              cell: (d: Deal) => <StatusCell status={d.status} tone={statusTone(d.status)} />,
            },
            {
              key: 'expected_close_date',
              header: 'Expected close',
              sortable: true,
              hideable: true,
              cell: (d: Deal) => d.expected_close_date ?? '—',
            },
            {
              key: 'owner',
              header: 'Owner',
              hideable: true,
              cell: (d: Deal) => d.owner_user_id ?? '—',
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function DealsPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading deals…</p>}>
      <DealsPageInner />
    </Suspense>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const h1: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: 'clamp(1.75rem, 2.5vw, 2.25rem)',
  fontWeight: 650,
  letterSpacing: '-0.02em',
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 560,
};
