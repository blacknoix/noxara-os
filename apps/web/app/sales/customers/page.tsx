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
  LinkCell,
  LoadingState,
  PermissionDeniedState,
  Select,
  Table,
  type SortDir,
  type TableDensity,
  parseViewFromSearchParams,
  viewToSearchParams,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Customer = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  owner_user_id: string | null;
  created_at: string;
};

function CustomersPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // Seed once from URL on first paint
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [customers, setCustomers] = useState<Customer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [q, setQ] = useState(initialView.q ?? '');
  const [filters, setFilters] = useState<FilterClause[]>(initialView.filters ?? []);
  const [sortKey, setSortKey] = useState(initialView.sort?.key ?? 'name');
  const [sortDir, setSortDir] = useState<SortDir>(initialView.sort?.dir ?? 'asc');
  const [density, setDensity] = useState<TableDensity>(initialView.density ?? 'comfortable');
  const [hiddenColumns, setHiddenColumns] = useState<string[]>(initialView.hiddenColumns ?? []);

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
      router.replace(qs ? `/sales/customers?${qs}` : '/sales/customers', { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns],
  );

  const loadCustomers = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const params = new URLSearchParams({ limit: '200' });
    if (q.trim()) params.set('q', q.trim());
    const res = await authFetch(`/api/v1/sales/customers?${params.toString()}`);
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
      setError('Could not load customers');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setCustomers(body.items ?? []);
    setLoading(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q]);

  useEffect(() => {
    void loadCustomers();
  }, [loadCustomers]);

  useEffect(() => {
    const fromUrl = parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? ''));
    if (fromUrl.q != null) setQ(fromUrl.q);
    if (fromUrl.filters) setFilters(fromUrl.filters);
  }, [searchParams]);

  const filtered = useMemo(() => {
    const rows = [...customers];
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      const av = sortKey === 'email' ? a.email ?? '' : a.name;
      const bv = sortKey === 'email' ? b.email ?? '' : b.name;
      return av.localeCompare(bv) * dir;
    });
    return rows;
  }, [customers, sortKey, sortDir]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view customers." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('sales.customer.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="sales.customer.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={h1}>Customers</h1>
        <p style={muted}>Accounts your team is working with.</p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <FilterBar
        q={q}
        onQueryChange={(next) => {
          setQ(next);
          syncUrl({ q: next });
        }}
        searchPlaceholder="Search name or email…"
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
        <LoadingState label="Loading customers" rows={4} />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No customers match"
          description={
            customers.length === 0
              ? 'Customers appear here once created, or converted from a lead.'
              : 'Try clearing filters or search.'
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
          getRowKey={(c) => c.id}
          columns={[
            {
              key: 'name',
              header: 'Name',
              sortable: true,
              hideable: true,
              cell: (c: Customer) => <LinkCell href={`/sales/customers/${c.id}`}>{c.name}</LinkCell>,
            },
            {
              key: 'email',
              header: 'Email',
              sortable: true,
              hideable: true,
              cell: (c: Customer) => c.email ?? '—',
            },
            {
              key: 'phone',
              header: 'Phone',
              hideable: true,
              cell: (c: Customer) => c.phone ?? '—',
            },
            {
              key: 'owner',
              header: 'Owner',
              hideable: true,
              cell: (c: Customer) => c.owner_user_id ?? '—',
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function CustomersPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading customers…</p>}>
      <CustomersPageInner />
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
