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
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Project = {
  id: string;
  name: string;
  description: string | null;
  status: string;
  owner_user_id: string;
  due_at: string | null;
  created_at: string;
};

const STATUS_OPTIONS = [
  { value: '', label: 'Any status' },
  { value: 'active', label: 'Active' },
  { value: 'on_hold', label: 'On hold' },
  { value: 'completed', label: 'Completed' },
  { value: 'cancelled', label: 'Cancelled' },
];

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'active':
      return 'info';
    case 'completed':
      return 'success';
    case 'on_hold':
      return 'warning';
    case 'cancelled':
      return 'danger';
    default:
      return 'neutral';
  }
}

function ProjectsPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [creating, setCreating] = useState(false);
  const [q, setQ] = useState(initialView.q ?? searchParams?.get('q') ?? '');
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
      router.replace(qs ? `/ops/projects?${qs}` : '/ops/projects', { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns],
  );

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/operations/projects?limit=200');
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
      setError('Could not load projects');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setProjects(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const fromUrl = parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? ''));
    if (fromUrl.q != null) setQ(fromUrl.q);
    if (fromUrl.filters) setFilters(fromUrl.filters);
  }, [searchParams]);

  const statusFilter = filters.find((f) => f.field === 'status' && f.operator === 'is');

  const filtered = useMemo(() => {
    let rows = [...projects];
    const needle = q.trim().toLowerCase();
    if (needle) {
      rows = rows.filter((p) => p.name.toLowerCase().includes(needle));
    }
    if (statusFilter?.value && typeof statusFilter.value === 'string') {
      rows = rows.filter((p) => p.status === statusFilter.value);
    }
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      if (sortKey === 'status') return a.status.localeCompare(b.status) * dir;
      if (sortKey === 'due_at') return (a.due_at ?? '').localeCompare(b.due_at ?? '') * dir;
      return a.name.localeCompare(b.name) * dir;
    });
    return rows;
  }, [projects, q, statusFilter, sortKey, sortDir]);

  function setStatusFilter(value: string) {
    const rest = filters.filter((f) => f.field !== 'status');
    const next = value
      ? [...rest, { id: 'status-is', field: 'status', operator: 'is' as const, value, label: 'Status' }]
      : rest;
    setFilters(next);
    syncUrl({ filters: next });
  }

  async function createProject() {
    if (!can('operations.project.create') || creating) return;
    const name = window.prompt('Project name');
    if (!name?.trim()) return;
    setCreating(true);
    const res = await authFetch('/api/v1/operations/projects', {
      method: 'POST',
      body: JSON.stringify({ name: name.trim() }),
    });
    setCreating(false);
    if (!res.ok) {
      setError('Could not create project');
      return;
    }
    const created = (await res.json()) as Project;
    router.push(`/ops/projects/${created.id}`);
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view projects." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('operations.project.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="operations.project.read" />;
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
          <p style={eyebrow}>Ops</p>
          <h1 style={h1}>Projects</h1>
          <p style={muted}>
            Delivery workstreams linked to customers and deals.{' '}
            <Link href="/ops/tasks" style={{ color: 'var(--cos-color-accent)' }}>
              View tasks
            </Link>
          </p>
        </div>
        {can('operations.project.create') ? (
          <Button type="button" variant="primary" onClick={() => void createProject()} disabled={creating}>
            {creating ? 'Creating…' : 'New project'}
          </Button>
        ) : null}
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <FilterBar
        q={q}
        onQueryChange={(next) => {
          setQ(next);
          syncUrl({ q: next });
        }}
        searchPlaceholder="Search project name…"
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
        <LoadingState label="Loading projects" rows={4} />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No projects match"
          description={
            projects.length === 0
              ? 'Create a project to start tracking delivery work.'
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
          getRowKey={(p) => p.id}
          columns={[
            {
              key: 'name',
              header: 'Name',
              sortable: true,
              hideable: true,
              cell: (p: Project) => (
                <Link href={`/ops/projects/${p.id}`} style={{ color: 'var(--cos-color-accent)' }}>
                  {p.name}
                </Link>
              ),
            },
            {
              key: 'status',
              header: 'Status',
              sortable: true,
              hideable: true,
              cell: (p: Project) => <StatusCell status={p.status} tone={statusTone(p.status)} />,
            },
            {
              key: 'due_at',
              header: 'Due',
              sortable: true,
              hideable: true,
              cell: (p: Project) => p.due_at ?? '—',
            },
            {
              key: 'owner',
              header: 'Owner',
              hideable: true,
              cell: (p: Project) => p.owner_user_id,
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function ProjectsPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading projects…</p>}>
      <ProjectsPageInner />
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
