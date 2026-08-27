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
  Button,
  EmptyState,
  ErrorState,
  FilterBar,
  type FilterClause,
  LinkCell,
  LoadingState,
  PermissionDeniedState,
  Select,
  StatusCell,
  Table,
  type SortDir,
  type TableDensity,
  parseViewFromSearchParams,
  viewToSearchParams,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';

type Employee = {
  id: string;
  display_name: string;
  title: string | null;
  status: string;
  department_id: string | null;
  start_date: string | null;
};

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'active':
      return 'success';
    case 'onboarding':
      return 'info';
    case 'on_leave':
      return 'warning';
    case 'offboarding':
    case 'terminated':
      return 'danger';
    default:
      return 'neutral';
  }
}

function PeopleDirectoryInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // Seed once from URL on first paint
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [employees, setEmployees] = useState<Employee[]>([]);
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
      router.replace(qs ? `/people?${qs}` : '/people', { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns],
  );

  const loadEmployees = useCallback(async () => {
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
    const res = await authFetch(`/api/v1/people/employees?${params.toString()}`);
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
      setError('Could not load employees');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setEmployees(body.items ?? []);
    setLoading(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q]);

  useEffect(() => {
    void loadEmployees();
  }, [loadEmployees]);

  useEffect(() => {
    const fromUrl = parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? ''));
    if (fromUrl.q != null) setQ(fromUrl.q);
    if (fromUrl.filters) setFilters(fromUrl.filters);
  }, [searchParams]);

  const filtered = useMemo(() => {
    const rows = [...employees];
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      const av =
        sortKey === 'title'
          ? a.title ?? ''
          : sortKey === 'status'
            ? a.status
            : sortKey === 'department_id'
              ? a.department_id ?? ''
              : sortKey === 'start_date'
                ? a.start_date ?? ''
                : a.display_name;
      const bv =
        sortKey === 'title'
          ? b.title ?? ''
          : sortKey === 'status'
            ? b.status
            : sortKey === 'department_id'
              ? b.department_id ?? ''
              : sortKey === 'start_date'
                ? b.start_date ?? ''
                : b.display_name;
      return av.localeCompare(bv) * dir;
    });
    return rows;
  }, [employees, sortKey, sortDir]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view the people directory." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('hr.employee.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="hr.employee.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '1rem',
          alignItems: 'flex-end',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <p style={eyebrow}>People</p>
          <h1 style={h1}>People</h1>
          <p style={muted}>Employee directory for your organization.</p>
        </div>
        {can('hr.employee.onboard') ? (
          <Link href="/people/onboard" style={{ textDecoration: 'none' }}>
            <Button type="button" variant="primary">
              Onboard employee
            </Button>
          </Link>
        ) : null}
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <FilterBar
        q={q}
        onQueryChange={(next) => {
          setQ(next);
          syncUrl({ q: next });
        }}
        searchPlaceholder="Search name or title…"
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
        <LoadingState label="Loading employees" rows={4} />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No employees match"
          description={
            employees.length === 0
              ? 'Employees appear here once onboarded.'
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
          getRowKey={(e) => e.id}
          columns={[
            {
              key: 'name',
              header: 'Name',
              sortable: true,
              hideable: true,
              cell: (e: Employee) => (
                <LinkCell href={`/people/${e.id}`}>{e.display_name}</LinkCell>
              ),
            },
            {
              key: 'title',
              header: 'Title',
              sortable: true,
              hideable: true,
              cell: (e: Employee) => e.title ?? '—',
            },
            {
              key: 'status',
              header: 'Status',
              sortable: true,
              hideable: true,
              cell: (e: Employee) => <StatusCell status={e.status} tone={statusTone(e.status)} />,
            },
            {
              key: 'department_id',
              header: 'Department',
              sortable: true,
              hideable: true,
              cell: (e: Employee) => e.department_id ?? '—',
            },
            {
              key: 'start_date',
              header: 'Start date',
              sortable: true,
              hideable: true,
              cell: (e: Employee) => e.start_date ?? '—',
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function PeopleDirectoryPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading people…</p>}>
      <PeopleDirectoryInner />
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
