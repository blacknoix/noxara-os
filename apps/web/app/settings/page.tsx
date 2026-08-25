'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties, type FormEvent } from 'react';
import { Button, EmptyState, ErrorState, Input, Table } from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';

type Tab = 'profile' | 'members' | 'teams' | 'roles';

type Org = {
  org_id: string;
  name: string;
  currency: string;
  timezone: string;
  fiscal_year_start_month: number;
  business_type: string;
  plan: string;
  branding: Record<string, unknown>;
};

type Member = {
  membership_id: string;
  user_id: string;
  email: string;
  display_name: string;
  role: string;
  status: string;
};

type Role = {
  role_id: string;
  name: string;
  description: string;
  system_key?: string | null;
  is_system: boolean;
  approval_limit_amount_minor?: number | null;
  approval_limit_currency?: string | null;
  permissions: { permission_id: string; effect: string; scope: string }[];
};

type Perm = { id: string; description: string; sensitive: boolean };

export default function SettingsPage() {
  const { can, loading: capsLoading, error: capsError, refresh: refreshCaps } = useCapabilities();
  const [tab, setTab] = useState<Tab>('profile');
  const [error, setError] = useState<string | null>(null);
  const [org, setOrg] = useState<Org | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [teams, setTeams] = useState<{ team_id: string; name: string }[]>([]);
  const [departments, setDepartments] = useState<{ department_id: string; name: string }[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [catalogue, setCatalogue] = useState<Perm[]>([]);
  const [selectedRole, setSelectedRole] = useState<Role | null>(null);
  const [preview, setPreview] = useState<string[]>([]);
  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState('member');
  const [teamName, setTeamName] = useState('');
  const [deptName, setDeptName] = useState('');
  const [busy, setBusy] = useState(false);

  const denied = capsError === 'Permission denied' || (!capsLoading && !getAccessToken());

  const loadOrg = useCallback(async () => {
    const res = await authFetch('/api/v1/workspace/organizations');
    if (!res.ok) {
      setError(res.status === 403 ? 'Permission denied' : 'Could not load organization');
      return;
    }
    setOrg(await res.json());
  }, []);

  const loadMembers = useCallback(async () => {
    const res = await authFetch('/api/v1/workspace/members');
    if (!res.ok) return;
    const body = await res.json();
    setMembers(body.items ?? []);
  }, []);

  const loadTeams = useCallback(async () => {
    const [t, d] = await Promise.all([
      authFetch('/api/v1/workspace/teams'),
      authFetch('/api/v1/workspace/departments'),
    ]);
    if (t.ok) setTeams((await t.json()).items ?? []);
    if (d.ok) setDepartments((await d.json()).items ?? []);
  }, []);

  const loadRoles = useCallback(async () => {
    const [r, p] = await Promise.all([
      authFetch('/api/v1/workspace/roles'),
      authFetch('/api/v1/workspace/permissions'),
    ]);
    if (r.ok) {
      const body = await r.json();
      setRoles(body.items ?? []);
    }
    if (p.ok) {
      const body = await p.json();
      setCatalogue(body.items ?? []);
    }
  }, []);

  useEffect(() => {
    if (!getAccessToken()) return;
    void loadOrg();
  }, [loadOrg]);

  useEffect(() => {
    if (tab === 'members') void loadMembers();
    if (tab === 'teams') void loadTeams();
    if (tab === 'roles') void loadRoles();
  }, [tab, loadMembers, loadTeams, loadRoles]);

  async function saveProfile(e: FormEvent) {
    e.preventDefault();
    if (!org) return;
    setBusy(true);
    setError(null);
    const res = await authFetch('/api/v1/workspace/organizations/settings', {
      method: 'PUT',
      body: JSON.stringify({
        name: org.name,
        currency: org.currency,
        timezone: org.timezone,
        fiscal_year_start_month: org.fiscal_year_start_month,
        branding: org.branding,
      }),
    });
    setBusy(false);
    if (!res.ok) {
      const body = await res.json();
      setError(body.detail ?? 'Update failed');
      return;
    }
    setOrg(await res.json());
    await refreshCaps();
  }

  async function invite(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    const res = await authFetch('/api/v1/workspace/members/invite', {
      method: 'POST',
      body: JSON.stringify({ email: inviteEmail, role: inviteRole }),
    });
    setBusy(false);
    if (!res.ok) {
      const body = await res.json();
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
      const body = await res.json();
      setError(body.detail ?? 'Role change failed');
      return;
    }
    await loadMembers();
  }

  async function suspend(userId: string) {
    const res = await authFetch(`/api/v1/workspace/members/${userId}/suspend`, { method: 'POST' });
    if (!res.ok) {
      const body = await res.json();
      setError(body.detail ?? 'Suspend failed');
      return;
    }
    await loadMembers();
  }

  async function revoke(userId: string) {
    const res = await authFetch(`/api/v1/workspace/members/${userId}/revoke`, { method: 'POST' });
    if (!res.ok) {
      const body = await res.json();
      setError(body.detail ?? 'Revoke failed');
      return;
    }
    await loadMembers();
  }

  async function createTeam(e: FormEvent) {
    e.preventDefault();
    const res = await authFetch('/api/v1/workspace/teams', {
      method: 'POST',
      body: JSON.stringify({ name: teamName }),
    });
    if (!res.ok) {
      setError('Could not create team');
      return;
    }
    setTeamName('');
    await loadTeams();
  }

  async function createDept(e: FormEvent) {
    e.preventDefault();
    const res = await authFetch('/api/v1/workspace/departments', {
      method: 'POST',
      body: JSON.stringify({ name: deptName }),
    });
    if (!res.ok) {
      setError('Could not create department');
      return;
    }
    setDeptName('');
    await loadTeams();
  }

  async function openRole(role: Role) {
    setSelectedRole(role);
    const res = await authFetch(`/api/v1/workspace/roles/${role.role_id}/preview`);
    if (res.ok) {
      const body = await res.json();
      setPreview(body.allowed ?? []);
    }
  }

  function togglePerm(permissionId: string, effect: 'allow' | 'deny') {
    if (!selectedRole) return;
    const existing = selectedRole.permissions.filter((p) => p.permission_id !== permissionId);
    const next = {
      ...selectedRole,
      permissions: [...existing, { permission_id: permissionId, effect, scope: 'organization' }],
    };
    setSelectedRole(next);
  }

  async function saveRole() {
    if (!selectedRole) return;
    setBusy(true);
    const res = await authFetch(`/api/v1/workspace/roles/${selectedRole.role_id}`, {
      method: 'PUT',
      body: JSON.stringify({
        name: selectedRole.name,
        description: selectedRole.description,
        approval_limit_amount_minor: selectedRole.approval_limit_amount_minor,
        approval_limit_currency: selectedRole.approval_limit_currency,
        permissions: selectedRole.permissions,
      }),
    });
    setBusy(false);
    if (!res.ok) {
      const body = await res.json();
      setError(body.detail ?? 'Role save failed');
      return;
    }
    const updated = await res.json();
    setSelectedRole(updated);
    await loadRoles();
    await openRole(updated);
  }

  const tabs = useMemo(
    () =>
      (
        [
          { id: 'profile' as const, label: 'Organization', show: can('workspace.org.read') },
          { id: 'members' as const, label: 'Members', show: can('workspace.member.read') },
          { id: 'teams' as const, label: 'Teams', show: can('workspace.team.read') },
          { id: 'roles' as const, label: 'Roles', show: can('workspace.role.read') },
        ] as const
      ).filter((t) => t.show),
    [can],
  );

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to continue." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied && !can('workspace.org.read')) {
    return (
      <ErrorState
        title="Permission denied"
        message="Your role cannot access settings. Ask an Owner or Admin."
      />
    );
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <h1 style={h1}>Settings</h1>
        <p style={muted}>Organization profile, members, teams, and the role permission matrix.</p>
      </header>

      <nav style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            style={{
              ...tabBtn,
              background: tab === t.id ? 'var(--cos-color-bg-elevated)' : 'transparent',
              fontWeight: tab === t.id ? 600 : 500,
            }}
          >
            {t.label}
          </button>
        ))}
        <a href="/onboarding" style={{ ...tabBtn, marginLeft: 'auto' }}>
          New org
        </a>
      </nav>

      {error ? <ErrorState title="Something went wrong" message={error} /> : null}

      {tab === 'profile' && org ? (
        <form onSubmit={(e) => void saveProfile(e)} style={formGrid}>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Name
  <Input
            value={org.name}
            onChange={(e) => setOrg({ ...org, name: e.target.value })}
            disabled={!can('workspace.org.update_settings')}
          />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Currency
  <Input
            value={org.currency}
            onChange={(e) => setOrg({ ...org, currency: e.target.value })}
            disabled={!can('workspace.org.update_settings')}
          />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Timezone
  <Input
            value={org.timezone}
            onChange={(e) => setOrg({ ...org, timezone: e.target.value })}
            disabled={!can('workspace.org.update_settings')}
          />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Fiscal year start month
  <Input
            type="number"
            value={String(org.fiscal_year_start_month)}
            onChange={(e) =>
              setOrg({ ...org, fiscal_year_start_month: Number(e.target.value) || 1 })
            }
            disabled={!can('workspace.org.update_settings')}
          />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Plan
  <Input value={org.plan} disabled />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Brand accent (placeholder)
  <Input
            value={String(org.branding?.accent ?? '')}
            onChange={(e) =>
              setOrg({ ...org, branding: { ...org.branding, accent: e.target.value } })
            }
            disabled={!can('workspace.org.update_settings')}
          />
</label>
          {can('workspace.org.update_settings') ? (
            <Button type="submit" disabled={busy}>
              {busy ? 'Saving…' : 'Save settings'}
            </Button>
          ) : null}
        </form>
      ) : null}

      {tab === 'members' ? (
        <section style={{ display: 'grid', gap: '1rem' }}>
          <p style={muted}>
            Prefer the full members experience?{' '}
            <a href="/members" style={{ color: 'var(--cos-color-accent)' }}>
              Open members with filters
            </a>
          </p>
          {can('workspace.member.invite') ? (
            <form onSubmit={(e) => void invite(e)} style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
              <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Invite email
  <Input
                value={inviteEmail}
                onChange={(e) => setInviteEmail(e.target.value)}
              />
</label>
              <label style={label}>
                Role
                <select value={inviteRole} onChange={(e) => setInviteRole(e.target.value)} style={select}>
                  {['member', 'manager', 'finance', 'sales', 'admin', 'read_only'].map((r) => (
                    <option key={r} value={r}>
                      {r}
                    </option>
                  ))}
                </select>
              </label>
              <Button type="submit" disabled={busy}>
                Invite
              </Button>
            </form>
          ) : null}
          {members.length === 0 ? (
            <EmptyState title="No members" description="Invite teammates to collaborate." />
          ) : (
            <Table
              columns={[
                { key: 'name', header: 'Name', cell: (m: Member) => m.display_name },
                { key: 'email', header: 'Email', cell: (m: Member) => m.email },
                {
                  key: 'role',
                  header: 'Role',
                  cell: (m: Member) =>
                    can('workspace.role.assign') ? (
                      <select
                        value={m.role}
                        onChange={(e) => void changeRole(m.user_id, e.target.value)}
                        style={select}
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
                { key: 'status', header: 'Status', cell: (m: Member) => m.status },
                {
                  key: 'actions',
                  header: '',
                  cell: (m: Member) => (
                    <span style={{ display: 'flex', gap: '0.35rem' }}>
                      {can('workspace.member.suspend') && m.status === 'active' ? (
                        <Button type="button" onClick={() => void suspend(m.user_id)}>
                          Suspend
                        </Button>
                      ) : null}
                      {can('workspace.member.revoke') ? (
                        <Button type="button" onClick={() => void revoke(m.user_id)}>
                          Revoke
                        </Button>
                      ) : null}
                    </span>
                  ),
                },
              ]}
              rows={members}
            />
          )}
        </section>
      ) : null}

      {tab === 'teams' ? (
        <section style={{ display: 'grid', gap: '1.25rem', gridTemplateColumns: '1fr 1fr' }}>
          <div>
            <h2 style={h2}>Teams</h2>
            {can('workspace.team.manage') ? (
              <form onSubmit={(e) => void createTeam(e)} style={{ display: 'flex', gap: '0.5rem' }}>
                <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Team name
  <Input value={teamName} onChange={(e) => setTeamName(e.target.value)} />
</label>
                <Button type="submit">Add</Button>
              </form>
            ) : null}
            {teams.length === 0 ? (
              <EmptyState title="No teams" description="Create a team to group people." />
            ) : (
              <ul>
                {teams.map((t) => (
                  <li key={t.team_id}>{t.name}</li>
                ))}
              </ul>
            )}
          </div>
          <div>
            <h2 style={h2}>Departments</h2>
            {can('workspace.department.manage') ? (
              <form onSubmit={(e) => void createDept(e)} style={{ display: 'flex', gap: '0.5rem' }}>
                <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Department name
  <Input
                  value={deptName}
                  onChange={(e) => setDeptName(e.target.value)}
                />
</label>
                <Button type="submit">Add</Button>
              </form>
            ) : null}
            {departments.length === 0 ? (
              <EmptyState title="No departments" description="Optional reporting-line structure." />
            ) : (
              <ul>
                {departments.map((d) => (
                  <li key={d.department_id}>{d.name}</li>
                ))}
              </ul>
            )}
          </div>
        </section>
      ) : null}

      {tab === 'roles' ? (
        <section style={{ display: 'grid', gap: '1rem', gridTemplateColumns: '220px 1fr' }}>
          <aside style={{ display: 'grid', gap: '0.35rem', alignContent: 'start' }}>
            {roles.map((r) => (
              <button key={r.role_id} type="button" style={tabBtn} onClick={() => void openRole(r)}>
                {r.name}
              </button>
            ))}
          </aside>
          <div>
            {!selectedRole ? (
              <EmptyState title="Select a role" description="Edit the permission matrix and preview capabilities." />
            ) : (
              <div style={{ display: 'grid', gap: '1rem' }}>
                <h2 style={h2}>{selectedRole.name}</h2>
                <p style={muted}>{selectedRole.description}</p>
                <div style={{ display: 'flex', gap: '0.75rem' }}>
                  <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Approval limit (minor units)
  <Input
                    type="number"
                    value={String(selectedRole.approval_limit_amount_minor ?? '')}
                    onChange={(e) =>
                      setSelectedRole({
                        ...selectedRole,
                        approval_limit_amount_minor: e.target.value
                          ? Number(e.target.value)
                          : null,
                      })
                    }
                    disabled={!can('workspace.role.manage')}
                  />
</label>
                  <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Limit currency
  <Input
                    value={selectedRole.approval_limit_currency ?? ''}
                    onChange={(e) =>
                      setSelectedRole({
                        ...selectedRole,
                        approval_limit_currency: e.target.value || null,
                      })
                    }
                    disabled={!can('workspace.role.manage')}
                  />
</label>
                </div>
                <div style={{ overflow: 'auto', maxHeight: 360 }}>
                  <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.9rem' }}>
                    <thead>
                      <tr>
                        <th align="left">Permission</th>
                        <th>Allow</th>
                        <th>Deny</th>
                      </tr>
                    </thead>
                    <tbody>
                      {catalogue.map((p) => {
                        const row = selectedRole.permissions.find((x) => x.permission_id === p.id);
                        return (
                          <tr key={p.id}>
                            <td>
                              <code>{p.id}</code>
                              {p.sensitive ? ' · sensitive' : ''}
                            </td>
                            <td align="center">
                              <input
                                type="radio"
                                name={`p-${p.id}`}
                                checked={row?.effect === 'allow'}
                                disabled={!can('workspace.role.manage') || selectedRole.system_key === 'owner'}
                                onChange={() => togglePerm(p.id, 'allow')}
                              />
                            </td>
                            <td align="center">
                              <input
                                type="radio"
                                name={`p-${p.id}`}
                                checked={row?.effect === 'deny'}
                                disabled={!can('workspace.role.manage') || selectedRole.system_key === 'owner'}
                                onChange={() => togglePerm(p.id, 'deny')}
                              />
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
                <div>
                  <h3 style={h2}>Live capability preview</h3>
                  <p style={muted}>{preview.length} permissions allowed</p>
                  <ul style={{ columns: 2, fontSize: '0.85rem' }}>
                    {preview.map((p) => (
                      <li key={p}>
                        <code>{p}</code>
                      </li>
                    ))}
                  </ul>
                </div>
                {can('workspace.role.manage') ? (
                  <Button type="button" onClick={() => void saveRole()} disabled={busy}>
                    {busy ? 'Saving…' : 'Save role'}
                  </Button>
                ) : null}
              </div>
            )}
          </div>
        </section>
      ) : null}
    </div>
  );
}

const h1: CSSProperties = {
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.75rem',
  fontWeight: 650,
  margin: 0,
};

const h2: CSSProperties = {
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.15rem',
  fontWeight: 600,
  margin: '0 0 0.5rem',
};

const muted: CSSProperties = { color: 'var(--cos-color-fg-muted)', margin: '0.25rem 0 0' };

const tabBtn: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.4rem 0.75rem',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
  cursor: 'pointer',
  textAlign: 'left',
};

const formGrid: CSSProperties = {
  display: 'grid',
  gap: '0.75rem',
  maxWidth: 480,
};

const label: CSSProperties = {
  display: 'grid',
  gap: '0.35rem',
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};

const select: CSSProperties = {
  padding: '0.45rem 0.6rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
