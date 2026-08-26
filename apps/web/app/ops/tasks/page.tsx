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
  KanbanBoard,
  type KanbanColumn,
  LoadingState,
  PermissionDeniedState,
  Select,
  StatusCell,
  Table,
  Tabs,
  type SortDir,
  type TableDensity,
  parseViewFromSearchParams,
  viewToSearchParams,
  useToast,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import { applyOptimisticMove, shouldRollback } from '../../../lib/boardOptimistic';

type Task = {
  id: string;
  project_id: string;
  title: string;
  status: string;
  priority: string;
  assignee_id: string | null;
  due_at: string | null;
  version: number;
};

type BoardColumn = { status: string; tasks: Task[] };
type BoardResponse = { project_id?: string | null; columns: BoardColumn[] };

const BOARD_STATUSES = ['backlog', 'todo', 'in_progress', 'in_review', 'done'] as const;

const STATUS_LABELS: Record<string, string> = {
  backlog: 'Backlog',
  todo: 'To do',
  in_progress: 'In progress',
  in_review: 'In review',
  done: 'Done',
};

const STATUS_OPTIONS = [
  { value: '', label: 'Any status' },
  ...BOARD_STATUSES.map((s) => ({ value: s, label: STATUS_LABELS[s] })),
];

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'done':
      return 'success';
    case 'in_progress':
      return 'info';
    case 'in_review':
      return 'warning';
    case 'backlog':
      return 'neutral';
    default:
      return 'neutral';
  }
}

function TasksPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();
  const { toast } = useToast();

  const tabParam = searchParams?.get('tab') ?? 'board';
  const projectId = searchParams?.get('project_id') ?? '';
  const activeTab = ['board', 'list', 'calendar'].includes(tabParam) ? tabParam : 'board';

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [board, setBoard] = useState<BoardResponse | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [q, setQ] = useState(initialView.q ?? searchParams?.get('q') ?? '');
  const [filters, setFilters] = useState<FilterClause[]>(initialView.filters ?? []);
  const [sortKey, setSortKey] = useState(initialView.sort?.key ?? 'title');
  const [sortDir, setSortDir] = useState<SortDir>(initialView.sort?.dir ?? 'asc');
  const [density, setDensity] = useState<TableDensity>(initialView.density ?? 'comfortable');
  const [hiddenColumns, setHiddenColumns] = useState<string[]>(initialView.hiddenColumns ?? []);
  const [calendarMonth, setCalendarMonth] = useState(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });

  const setTab = useCallback(
    (tab: string) => {
      const params = new URLSearchParams(searchParams?.toString() ?? '');
      params.set('tab', tab);
      if (projectId) params.set('project_id', projectId);
      router.replace(`/ops/tasks?${params.toString()}`, { scroll: false });
    },
    [router, searchParams, projectId],
  );

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
      params.set('tab', 'list');
      if (projectId) params.set('project_id', projectId);
      router.replace(`/ops/tasks?${params.toString()}`, { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns, projectId],
  );

  const loadBoard = useCallback(async () => {
    const qs = projectId ? `?project_id=${encodeURIComponent(projectId)}` : '';
    const res = await authFetch(`/api/v1/operations/board${qs}`);
    setRequestId(res.headers.get('x-request-id') ?? undefined);
    if (res.status === 401) {
      setError('Sign in required');
      return false;
    }
    if (res.status === 403) {
      setDenied(true);
      return false;
    }
    if (!res.ok) {
      setError('Could not load board');
      return false;
    }
    setBoard((await res.json()) as BoardResponse);
    return true;
  }, [projectId]);

  const loadList = useCallback(async () => {
    const params = new URLSearchParams({ limit: '200' });
    if (projectId) params.set('project_id', projectId);
    const res = await authFetch(`/api/v1/operations/tasks?${params}`);
    setRequestId(res.headers.get('x-request-id') ?? undefined);
    if (res.status === 401) {
      setError('Sign in required');
      return false;
    }
    if (res.status === 403) {
      setDenied(true);
      return false;
    }
    if (!res.ok) {
      setError('Could not load tasks');
      return false;
    }
    const body = await res.json();
    setTasks(body.items ?? []);
    return true;
  }, [projectId]);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    if (activeTab === 'board') {
      await loadBoard();
    } else {
      await loadList();
    }
    setLoading(false);
  }, [activeTab, loadBoard, loadList]);

  useEffect(() => {
    void load();
  }, [load]);

  const statusFilter = filters.find((f) => f.field === 'status' && f.operator === 'is');

  const filtered = useMemo(() => {
    let rows = [...tasks];
    const needle = q.trim().toLowerCase();
    if (needle) rows = rows.filter((t) => t.title.toLowerCase().includes(needle));
    if (statusFilter?.value && typeof statusFilter.value === 'string') {
      rows = rows.filter((t) => t.status === statusFilter.value);
    }
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      if (sortKey === 'status') return a.status.localeCompare(b.status) * dir;
      if (sortKey === 'priority') return a.priority.localeCompare(b.priority) * dir;
      if (sortKey === 'due_at') return (a.due_at ?? '').localeCompare(b.due_at ?? '') * dir;
      return a.title.localeCompare(b.title) * dir;
    });
    return rows;
  }, [tasks, q, statusFilter, sortKey, sortDir]);

  const calendarDays = useMemo(() => {
    const year = calendarMonth.getFullYear();
    const month = calendarMonth.getMonth();
    const firstDow = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const cells: Array<{ day: number | null; dateKey: string | null }> = [];
    for (let i = 0; i < firstDow; i++) cells.push({ day: null, dateKey: null });
    for (let d = 1; d <= daysInMonth; d++) {
      const key = `${year}-${String(month + 1).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
      cells.push({ day: d, dateKey: key });
    }
    return cells;
  }, [calendarMonth]);

  const tasksByDueDate = useMemo(() => {
    const map = new Map<string, Task[]>();
    for (const t of activeTab === 'calendar' ? tasks : filtered) {
      if (!t.due_at) continue;
      const key = t.due_at.slice(0, 10);
      const list = map.get(key) ?? [];
      list.push(t);
      map.set(key, list);
    }
    return map;
  }, [tasks, filtered, activeTab]);

  const moveTask = useCallback(
    async (cardId: string, fromColumnId: string, toColumnId: string) => {
      if (!board || fromColumnId === toColumnId) return;
      const columns = board.columns.map((c) => ({
        id: c.status,
        cards: c.tasks.map((t) => ({ ...t })),
      }));
      const task = board.columns.flatMap((c) => c.tasks).find((t) => t.id === cardId);
      if (!task) return;

      const { previous, next } = applyOptimisticMove(columns, cardId, fromColumnId, toColumnId);
      setBoard({
        ...board,
        columns: next.map((col) => ({
          status: col.id,
          tasks: col.cards as Task[],
        })),
      });

      const res = await authFetch(`/api/v1/operations/tasks/${cardId}/move`, {
        method: 'POST',
        headers: { 'If-Match': String(task.version) },
        body: JSON.stringify({ status: toColumnId }),
      });

      if (!res.ok) {
        setBoard({
          ...board,
          columns: previous.map((col) => ({
            status: col.id,
            tasks: col.cards as Task[],
          })),
        });
        const body = await res.json().catch(() => ({}));
        toast({
          title: shouldRollback(res.status) ? 'Task was updated elsewhere' : 'Could not move task',
          description: typeof body.detail === 'string' ? body.detail : 'Changes were rolled back.',
        });
        return;
      }

      const updated = (await res.json()) as Task;
      setBoard((current) => {
        if (!current) return current;
        return {
          ...current,
          columns: current.columns.map((col) => ({
            ...col,
            tasks: col.tasks.map((t) => (t.id === updated.id ? updated : t)),
          })),
        };
      });
    },
    [board, toast],
  );

  function setStatusFilter(value: string) {
    const rest = filters.filter((f) => f.field !== 'status');
    const next = value
      ? [...rest, { id: 'status-is', field: 'status', operator: 'is' as const, value, label: 'Status' }]
      : rest;
    setFilters(next);
    syncUrl({ filters: next });
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view tasks." />;
  }
  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }
  if (denied || !can('operations.task.read')) {
    return <PermissionDeniedState requiredPermission="operations.task.read" />;
  }

  const kanbanColumns: KanbanColumn[] = (board?.columns ?? BOARD_STATUSES.map((s) => ({ status: s, tasks: [] }))).map(
    (c) => ({
      id: c.status,
      title: STATUS_LABELS[c.status] ?? c.status,
      cards: c.tasks.map((t) => ({
        id: t.id,
        title: t.title,
        meta: t.due_at ? `Due ${t.due_at.slice(0, 10)}` : t.priority,
      })),
    }),
  );

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Ops</p>
        <h1 style={h1}>Tasks</h1>
        <p style={muted}>
          Board, list, and calendar views.{' '}
          <Link href="/ops/projects" style={{ color: 'var(--cos-color-accent)' }}>
            Projects
          </Link>
          {' · '}
          <Link href="/my-work" style={{ color: 'var(--cos-color-accent)' }}>
            My work
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <Tabs
        items={[
          { id: 'board', label: 'Board' },
          { id: 'list', label: 'List' },
          { id: 'calendar', label: 'Calendar' },
        ]}
        value={activeTab}
        onChange={setTab}
      >
        {activeTab === 'board' ? (
          loading ? (
            <LoadingState label="Loading board" rows={4} />
          ) : kanbanColumns.every((c) => c.cards.length === 0) ? (
            <EmptyState title="No tasks on the board" description="Create a task to populate the columns." />
          ) : (
            <KanbanBoard
              columns={kanbanColumns}
              onCardSelect={(cardId) => {
                router.push(`/ops/tasks?tab=list&q=${encodeURIComponent(cardId)}`);
              }}
              onCardMove={(cardId, from, to) => {
                void moveTask(cardId, from, to);
              }}
            />
          )
        ) : null}

        {activeTab === 'list' ? (
          <div style={{ display: 'grid', gap: '1rem' }}>
            <FilterBar
              q={q}
              onQueryChange={(next) => {
                setQ(next);
                syncUrl({ q: next });
              }}
              searchPlaceholder="Search task title…"
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
              onSaveView={() => syncUrl({})}
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
              <LoadingState label="Loading tasks" rows={4} />
            ) : filtered.length === 0 ? (
              <EmptyState title="No tasks match" description="Try clearing filters or search." />
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
                getRowKey={(t) => t.id}
                columns={[
                  { key: 'title', header: 'Title', sortable: true, hideable: true, cell: (t: Task) => t.title },
                  {
                    key: 'status',
                    header: 'Status',
                    sortable: true,
                    hideable: true,
                    cell: (t: Task) => <StatusCell status={t.status} tone={statusTone(t.status)} />,
                  },
                  {
                    key: 'priority',
                    header: 'Priority',
                    sortable: true,
                    hideable: true,
                    cell: (t: Task) => t.priority,
                  },
                  {
                    key: 'due_at',
                    header: 'Due',
                    sortable: true,
                    hideable: true,
                    cell: (t: Task) => (t.due_at ? t.due_at.slice(0, 10) : '—'),
                  },
                  {
                    key: 'project',
                    header: 'Project',
                    hideable: true,
                    cell: (t: Task) => (
                      <Link href={`/ops/projects/${t.project_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                        {t.project_id}
                      </Link>
                    ),
                  },
                ]}
                rows={filtered}
              />
            )}
          </div>
        ) : null}

        {activeTab === 'calendar' ? (
          loading ? (
            <LoadingState label="Loading calendar" rows={3} />
          ) : (
            <div style={{ display: 'grid', gap: '0.75rem' }}>
              <div style={{ display: 'flex', gap: '0.75rem', alignItems: 'center' }}>
                <button
                  type="button"
                  style={monthBtn}
                  onClick={() =>
                    setCalendarMonth(new Date(calendarMonth.getFullYear(), calendarMonth.getMonth() - 1, 1))
                  }
                >
                  Previous
                </button>
                <strong style={{ fontFamily: 'var(--cos-font-display)' }}>
                  {calendarMonth.toLocaleString(undefined, { month: 'long', year: 'numeric' })}
                </strong>
                <button
                  type="button"
                  style={monthBtn}
                  onClick={() =>
                    setCalendarMonth(new Date(calendarMonth.getFullYear(), calendarMonth.getMonth() + 1, 1))
                  }
                >
                  Next
                </button>
              </div>
              <div
                role="grid"
                aria-label="Task due dates"
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(7, minmax(0, 1fr))',
                  gap: '0.35rem',
                }}
              >
                {['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'].map((d) => (
                  <div key={d} style={{ fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)', padding: '0.25rem' }}>
                    {d}
                  </div>
                ))}
                {calendarDays.map((cell, idx) => {
                  const dayTasks = cell.dateKey ? tasksByDueDate.get(cell.dateKey) ?? [] : [];
                  return (
                    <div
                      key={idx}
                      role="gridcell"
                      style={{
                        minHeight: 72,
                        border: '1px solid var(--cos-color-border)',
                        borderRadius: 'var(--cos-radius-sm)',
                        padding: '0.35rem',
                        background: cell.day ? 'var(--cos-color-bg)' : 'transparent',
                      }}
                    >
                      {cell.day != null ? (
                        <>
                          <div style={{ fontSize: '0.8rem', fontWeight: 600 }}>{cell.day}</div>
                          {dayTasks.slice(0, 3).map((t) => (
                            <div
                              key={t.id}
                              style={{
                                fontSize: '0.7rem',
                                color: 'var(--cos-color-fg-muted)',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                              }}
                              title={t.title}
                            >
                              {t.title}
                            </div>
                          ))}
                          {dayTasks.length > 3 ? (
                            <div style={{ fontSize: '0.68rem', color: 'var(--cos-color-fg-muted)' }}>
                              +{dayTasks.length - 3} more
                            </div>
                          ) : null}
                        </>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            </div>
          )
        ) : null}
      </Tabs>
    </div>
  );
}

export default function TasksPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading tasks…</p>}>
      <TasksPageInner />
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

const monthBtn: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.35rem 0.65rem',
  fontSize: '0.85rem',
  cursor: 'pointer',
};
