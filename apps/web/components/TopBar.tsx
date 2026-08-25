'use client';

import type { CSSProperties } from 'react';
import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  Avatar,
  Button,
  EmptyState,
  ErrorState,
  Popover,
} from '@companyos/design-system';
import { authFetch, getAccessToken, setAccessToken } from '../lib/auth-client';
import { clearCapabilitiesCache, useCapabilities } from '../lib/capabilities';
import { THEME_OPTIONS, type CosTheme } from '../lib/theme';
import { useTheme } from './ThemeProvider';

type Membership = { org_id: string; org_name: string; role: string };
type Session = {
  id: string;
  org_id: string;
  device_label?: string | null;
  current: boolean;
  last_seen_at: string;
};
type Me = { email?: string; display_name?: string; org_id?: string };

export function TopBar({
  onTogglePanel,
  onToggleSidebar,
  onOpenCommand,
  sidebarCollapsed,
  panelOpen,
}: {
  onTogglePanel: () => void;
  onToggleSidebar: () => void;
  onOpenCommand: () => void;
  sidebarCollapsed: boolean;
  panelOpen: boolean;
}) {
  const router = useRouter();
  const { theme, setTheme } = useTheme();
  const { can } = useCapabilities();
  const [orgOpen, setOrgOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [notifyOpen, setNotifyOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [memberships, setMemberships] = useState<Membership[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [me, setMe] = useState<Me | null>(null);
  const [currentOrg, setCurrentOrg] = useState('Select org');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!getAccessToken()) return;
    void refreshIdentity();
  }, []);

  async function refreshIdentity() {
    setLoading(true);
    setError(null);
    try {
      const [meRes, memRes] = await Promise.all([
        authFetch('/api/v1/auth/me'),
        authFetch('/api/v1/auth/memberships'),
      ]);
      if (meRes.status === 401 || memRes.status === 401) {
        setError('Sign in required');
        return;
      }
      if (!meRes.ok || !memRes.ok) {
        setError('Could not load identity');
        return;
      }
      const meBody = (await meRes.json()) as Me;
      const mem = await memRes.json();
      const items: Membership[] = mem.items ?? [];
      setMe(meBody);
      setMemberships(items);
      const match = items.find((m) => m.org_id === meBody.org_id);
      setCurrentOrg(match?.org_name ?? meBody.org_id ?? 'Organization');
    } catch {
      setError('Identity request failed');
    } finally {
      setLoading(false);
    }
  }

  async function switchOrg(orgId: string) {
    const res = await authFetch('/api/v1/auth/switch-org', {
      method: 'POST',
      body: JSON.stringify({ org_id: orgId }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'Switch failed');
      return;
    }
    setAccessToken(body.access_token);
    clearCapabilitiesCache();
    const match = memberships.find((m) => m.org_id === orgId);
    setCurrentOrg(match?.org_name ?? orgId);
    setOrgOpen(false);
    window.dispatchEvent(new Event('cos:org-switched'));
    router.refresh();
    await refreshIdentity();
  }

  async function loadSessions() {
    setMenuOpen(true);
    const res = await authFetch('/api/v1/auth/sessions');
    if (!res.ok) {
      setSessions([]);
      return;
    }
    const body = await res.json();
    setSessions(body.items ?? []);
  }

  async function revokeSession(id: string) {
    await authFetch(`/api/v1/auth/sessions/${id}`, { method: 'DELETE' });
    await loadSessions();
  }

  async function logout() {
    await authFetch('/api/v1/auth/logout', { method: 'POST' });
    setAccessToken(null);
    clearCapabilitiesCache();
    window.location.href = '/login';
  }

  const displayName = me?.display_name || me?.email || 'You';

  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '0.65rem',
        padding: '0 1rem',
        height: 56,
        borderBottom: '1px solid var(--cos-color-border)',
        background: 'color-mix(in srgb, var(--cos-color-topbar) 92%, transparent)',
        backdropFilter: 'blur(8px)',
        position: 'relative',
        zIndex: 30,
      }}
    >
      <Button
        type="button"
        variant="ghost"
        size="sm"
        aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        onClick={onToggleSidebar}
      >
        Menu
      </Button>

      <div
        style={{
          fontFamily: 'var(--cos-font-display)',
          fontWeight: 650,
          fontSize: '1.15rem',
          letterSpacing: '-0.02em',
          minWidth: 110,
        }}
      >
        CompanyOS
      </div>

      <div style={{ position: 'relative' }}>
        <button
          type="button"
          aria-label="Organization switcher"
          aria-expanded={orgOpen}
          onClick={() => {
            setOrgOpen((v) => !v);
            void refreshIdentity();
          }}
          style={{
            border: '1px solid var(--cos-color-border)',
            background: 'var(--cos-color-bg-elevated)',
            borderRadius: 'var(--cos-radius-sm)',
            padding: '0.35rem 0.65rem',
            color: 'var(--cos-color-fg-muted)',
            cursor: 'pointer',
            maxWidth: 200,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {loading ? 'Loading…' : `${currentOrg} ▾`}
        </button>
        {orgOpen ? (
          <div style={popover}>
            {memberships.length === 0 ? (
              <EmptyState title="No orgs" description="Sign in to load memberships." />
            ) : (
              memberships.map((m) => (
                <button
                  key={m.org_id}
                  type="button"
                  onClick={() => void switchOrg(m.org_id)}
                  style={menuItem}
                >
                  {m.org_name} ({m.role})
                </button>
              ))
            )}
            <Link href="/onboarding" style={{ ...menuItem, display: 'block' }} onClick={() => setOrgOpen(false)}>
              Create organization…
            </Link>
          </div>
        ) : null}
      </div>

      <div style={{ flex: 1, maxWidth: 440 }}>
        <button
          type="button"
          onClick={onOpenCommand}
          aria-label="Open command bar"
          style={{
            width: '100%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 8,
            border: '1px solid var(--cos-color-border)',
            background: 'var(--cos-color-bg-elevated)',
            borderRadius: 'var(--cos-radius-sm)',
            padding: '0.4rem 0.75rem',
            color: 'var(--cos-color-fg-muted)',
            cursor: 'pointer',
            fontSize: '0.875rem',
          }}
        >
          <span>Search or run a command…</span>
          <kbd
            style={{
              fontSize: '0.7rem',
              border: '1px solid var(--cos-color-border)',
              borderRadius: 4,
              padding: '0.1rem 0.35rem',
            }}
          >
            ⌘K
          </kbd>
        </button>
      </div>

      <Popover
        open={createOpen}
        onOpenChange={setCreateOpen}
        label="Create"
        trigger={
          <Button type="button" variant="secondary" size="sm">
            Create
          </Button>
        }
      >
        <div style={{ display: 'grid', gap: 4, minWidth: 200 }}>
          <Link href="/onboarding" style={menuItem} onClick={() => setCreateOpen(false)}>
            Create organization
          </Link>
          {can('workspace.member.invite') ? (
            <Link href="/members" style={menuItem} onClick={() => setCreateOpen(false)}>
              Invite member
            </Link>
          ) : null}
          {can('workspace.org.read') ? (
            <Link href="/settings" style={menuItem} onClick={() => setCreateOpen(false)}>
              Open settings
            </Link>
          ) : null}
        </div>
      </Popover>

      <Popover
        open={notifyOpen}
        onOpenChange={setNotifyOpen}
        label="Notifications"
        trigger={
          <Button type="button" variant="ghost" size="sm" aria-label="Notifications">
            Alerts
          </Button>
        }
      >
        <div style={{ minWidth: 240, padding: '0.35rem' }}>
          <EmptyState title="No notifications yet" description="Alerts will appear here when modules ship." />
        </div>
      </Popover>

      <Popover
        open={helpOpen}
        onOpenChange={setHelpOpen}
        label="Help"
        trigger={
          <Button type="button" variant="ghost" size="sm" aria-label="Help">
            Help
          </Button>
        }
      >
        <div style={{ display: 'grid', gap: 6, minWidth: 220, padding: '0.25rem' }}>
          <a
            href="https://github.com"
            target="_blank"
            rel="noopener noreferrer"
            style={menuItem}
          >
            Documentation
          </a>
          <button
            type="button"
            style={menuItem}
            onClick={() => {
              setHelpOpen(false);
              onOpenCommand();
            }}
          >
            Keyboard shortcuts (⌘K)
          </button>
        </div>
      </Popover>

      <Button
        type="button"
        variant="ghost"
        size="sm"
        aria-label={panelOpen ? 'Hide context panel' : 'Show context panel'}
        aria-pressed={panelOpen}
        onClick={onTogglePanel}
      >
        Panel
      </Button>

      <div style={{ position: 'relative' }}>
        <button
          type="button"
          aria-label="User menu"
          aria-expanded={menuOpen}
          onClick={() => void loadSessions()}
          style={{
            border: 'none',
            background: 'transparent',
            padding: 0,
            cursor: 'pointer',
            display: 'grid',
            placeItems: 'center',
          }}
        >
          <Avatar name={displayName} size="sm" />
        </button>
        {menuOpen ? (
          <div style={{ ...popover, right: 0, left: 'auto', width: 280 }}>
            <strong style={{ fontSize: '0.85rem' }}>Theme</strong>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
              {THEME_OPTIONS.map((opt) => (
                <Button
                  key={opt.value}
                  type="button"
                  size="sm"
                  variant={theme === opt.value ? 'primary' : 'secondary'}
                  onClick={() => setTheme(opt.value as CosTheme)}
                >
                  {opt.label}
                </Button>
              ))}
            </div>
            <strong style={{ fontSize: '0.85rem', marginTop: 8 }}>Sessions</strong>
            {sessions.length === 0 ? (
              <p style={{ color: 'var(--cos-color-fg-muted)', fontSize: '0.85rem', margin: 0 }}>
                No active sessions (sign in first).
              </p>
            ) : (
              sessions.map((s) => (
                <div key={s.id} style={{ display: 'grid', gap: 4, marginTop: 8 }}>
                  <span style={{ fontSize: '0.8rem' }}>
                    {s.device_label || 'Device'} {s.current ? '(this)' : ''}
                  </span>
                  <Button type="button" variant="ghost" size="sm" onClick={() => void revokeSession(s.id)}>
                    Revoke
                  </Button>
                </div>
              ))
            )}
            <Button type="button" variant="secondary" onClick={() => void logout()}>
              Sign out
            </Button>
            <Link href="/login" style={{ fontSize: '0.85rem' }}>
              Sign in
            </Link>
          </div>
        ) : null}
      </div>

      {error ? (
        <div style={{ position: 'absolute', right: 12, top: '100%', zIndex: 20 }}>
          <ErrorState message={error} />
        </div>
      ) : null}
    </header>
  );
}

const popover: CSSProperties = {
  position: 'absolute',
  top: 'calc(100% + 6px)',
  left: 0,
  zIndex: 30,
  minWidth: 220,
  padding: '0.75rem',
  background: 'var(--cos-color-bg-elevated)',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  display: 'grid',
  gap: '0.35rem',
  boxShadow: 'var(--cos-shadow-soft)',
};

const menuItem: CSSProperties = {
  textAlign: 'left',
  border: 'none',
  background: 'transparent',
  padding: '0.4rem 0.2rem',
  cursor: 'pointer',
  color: 'var(--cos-color-fg)',
  fontSize: '0.9rem',
};
