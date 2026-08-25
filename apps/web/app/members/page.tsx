'use client';

import {
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type FormEvent,
} from 'react';
import Link from 'next/link';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  Button,
  EmptyState,
  ErrorState,
  FilterBar,
  type FilterClause,
  Input,
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

type Member = {
  membership_id: string;
  user_id: string;
  email: string;
  display_name: string;
  role: string;
  status: string;
};

const STATUS_OPTIONS = [
  { value: '', label: 'Any status' },
  { value: 'active', label: 'Active' },
  { value: 'invited', label: 'Invited' },
  { value: 'suspended', label: 'Suspended' },
];

const ROLE_OPTIONS = [
  { value: '', label: 'Any role' },
  { value: 'owner', label: 'Owner' },
  { value: 'admin', label: 'Admin' },
  { value: 'finance', label: 'Finance' },
  { value: 'sales', label: 'Sales' },
  { value: 'manager', label: 'Manager' },
  { value: 'member', label: 'Member' },
  { value: 'read_only', label: 'Read only' },
];

function statusToneSafe(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'active':
      return 'success';
    case 'invited':
      return 'info';
    case 'suspended':
      return 'danger';
    default:
      return 'neutral';
  }
}

function MembersPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { can, loading: capsLoading } = useCapabilities();

  const initialView = useMemo(
    () => parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? '')),
    // Seed once from URL on first paint
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const [members, setMembers] = useState<Member[]>([]);
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
  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState('member');
  const [busy, setBusy] = useState(false);

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
      router.replace(qs ? `/members?${qs}` : '/members', { scroll: false });
    },
    [router, q, filters, sortKey, sortDir, density, hiddenColumns],
  );

  const loadMembers = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/workspace/members');
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
      setError('Could not load members');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setMembers(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void loadMembers();
  }, [loadMembers]);

  useEffect(() => {
    const fromUrl = parseViewFromSearchParams(new URLSearchParams(searchParams?.toString() ?? ''));
    if (fromUrl.q != null) setQ(fromUrl.q);
    if (fromUrl.filters) setFilters(fromUrl.filters);
  }, [searchParams]);

  const statusFilter = filters.find((f) => f.field === 'status' && f.operator === 'is');
  const roleFilter = filters.find((f) => f.field === 'role' && f.operator === 'is');

  const filtered = useMemo(() => {
    let rows = [...members];
    const needle = q.trim().toLowerCase();
    if (needle) {
      rows = rows.filter(
        (m) =>
          m.display_name.toLowerCase().includes(needle) || m.email.toLowerCase().includes(needle),
      );
    }
    if (statusFilter?.value && typeof statusFilter.value === 'string') {
      rows = rows.filter((m) => m.status === statusFilter.value);
    }
    if (roleFilter?.value && typeof roleFilter.value === 'string') {
      rows = rows.filter((m) => m.role === roleFilter.value);
    }
    rows.sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      const av =
        sortKey === 'email'
          ? a.email
          : sortKey === 'role'
            ? a.role
            : sortKey === 'status'
              ? a.status
              : a.display_name;
      const bv =
        sortKey === 'email'
          ? b.email
          : sortKey === 'role'
            ? b.role
            : sortKey === 'status'
              ? b.status
              : b.display_name;
      return av.localeCompare(bv) * dir;
    });
    return rows;
  }, [members, q, statusFilter, roleFilter, sortKey, sortDir]);

  function setStatusFilter(value: string) {
    const rest = filters.filter((f) => f.field !== 'status');
    const next = value
      ? [...rest, { id: 'status-is', field: 'status', operator: 'is' as const, value, label: 'Status' }]
      : rest;
    setFilters(next);
    syncUrl({ filters: next });
  }

  function setRoleFilter(value: string) {
    const rest = filters.filter((f) => f.field !== 'role');
    const next = value
      ? [...rest, { id: 'role-is', field: 'role', operator: 'is' as const, value, label: 'Role' }]
      : rest;
    setFilters(next);
    syncUrl({ filters: next });
  }

  async function invite(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const res = await authFetch('/api/v1/workspace/members/invite', {
      method: 'POST',
      body: JSON.stringify({ email: inviteEmail, role: inviteRole }),
    });
    setBusy(false);
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Invite failed');
      return;
    }
    setInviteEmail('');
    await loadMembers();
  }

  async function changeRole(userId: string, role: string) {
    const res = await authFetch(`/api/v1/workspace/members/${userId}/role`, {
      method: 'PUT',
      body: JSON.stringify({ role }),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Role change failed');
      return;
    }
    await loadMembers();
  }

  async function suspend(userId: string) {
    const res = await authFetch(`/api/v1/workspace/members/${userId}/suspend`, { method: 'POST' });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Suspend failed');
      return;
    }
    await loadMembers();
  }

  async function revoke(userId: string) {
    const res = await authFetch(`/api/v1/workspace/members/${userId}/revoke`, { method: 'POST' });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Revoke failed');
      return;
    }
    await loadMembers();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to manage members." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('workspace.member.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="workspace.member.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Settings</p>
        <h1 style={h1}>Members</h1>
        <p style={muted}>
          Filter, sort, and save views of your organization roster.{' '}
          <Link href="/settings" style={{ color: 'var(--cos-color-accent)' }}>
            Organization settings
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      {can('workspace.member.invite') ? (
        <form
          onSubmit={(e) => void invite(e)}
          style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap', alignItems: 'flex-end' }}
        >
          <div style={{ minWidth: 220 }}>
            <Input
              label="Invite email"
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              type="email"
              required
            />
          </div>
          <div style={{ minWidth: 140 }}>
            <Select
              label="Role"
              value={inviteRole}
              onChange={(e) => setInviteRole(e.target.value)}
              options={ROLE_OPTIONS.filter((o) => o.value && o.value !== 'owner')}
            />
          </div>
          <Button type="submit" disabled={busy}>
            {busy ? 'Inviting…' : 'Invite'}
          </Button>
        </form>
      ) : null}

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
              label="Status is"
              value={typeof statusFilter?.value === 'string' ? statusFilter.value : ''}
              onChange={(e) => setStatusFilter(e.target.value)}
              options={STATUS_OPTIONS}
            />
          </div>
          <div style={{ minWidth: 140 }}>
            <Select
              label="Role is"
              value={typeof roleFilter?.value === 'string' ? roleFilter.value : ''}
              onChange={(e) => setRoleFilter(e.target.value)}
              options={ROLE_OPTIONS}
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
        <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading members…</p>
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No members match"
          description={
            members.length === 0
              ? 'Invite teammates to collaborate.'
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
          getRowKey={(m) => m.membership_id}
          columns={[
            {
              key: 'name',
              header: 'Name',
              sortable: true,
              hideable: true,
              cell: (m: Member) => m.display_name,
            },
            {
              key: 'email',
              header: 'Email',
              sortable: true,
              hideable: true,
              cell: (m: Member) => m.email,
            },
            {
              key: 'role',
              header: 'Role',
              sortable: true,
              hideable: true,
              cell: (m: Member) =>
                can('workspace.role.assign') ? (
                  <select
                    value={m.role}
                    onChange={(e) => void changeRole(m.user_id, e.target.value)}
                    style={select}
                    aria-label={`Role for ${m.display_name}`}
                  >
                    {['owner', 'admin', 'finance', 'sales', 'manager', 'member', 'read_only'].map(
                      (r) => (
                        <option key={r} value={r}>
                          {r}
                        </option>
                      ),
                    )}
                  </select>
                ) : (
                  m.role
                ),
            },
            {
              key: 'status',
              header: 'Status',
              sortable: true,
              hideable: true,
              cell: (m: Member) => (
                <StatusCell status={m.status} tone={statusToneSafe(m.status)} />
              ),
            },
            {
              key: 'actions',
              header: 'Actions',
              hideable: true,
              cell: (m: Member) => (
                <span style={{ display: 'flex', gap: '0.35rem', flexWrap: 'wrap' }}>
                  {can('workspace.member.suspend') && m.status === 'active' ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      onClick={() => void suspend(m.user_id)}
                    >
                      Suspend
                    </Button>
                  ) : null}
                  {can('workspace.member.revoke') ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => void revoke(m.user_id)}
                    >
                      Revoke
                    </Button>
                  ) : null}
                </span>
              ),
            },
          ]}
          rows={filtered}
        />
      )}
    </div>
  );
}

export default function MembersPage() {
  return (
    <Suspense fallback={<p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading members…</p>}>
      <MembersPageInner />
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

const select: CSSProperties = {
  padding: '0.35rem 0.5rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
