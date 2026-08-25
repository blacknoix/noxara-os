'use client';

import type { CSSProperties } from 'react';
import { useEffect, useState } from 'react';
import { Button, EmptyState, ErrorState, Input } from '@companyos/design-system';
import { authFetch, getAccessToken, setAccessToken } from '../lib/auth-client';

type Membership = { org_id: string; org_name: string; role: string };
type Session = {
  id: string;
  org_id: string;
  device_label?: string | null;
  current: boolean;
  last_seen_at: string;
};

export function TopBar({ onTogglePanel }: { onTogglePanel: () => void }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [orgOpen, setOrgOpen] = useState(false);
  const [memberships, setMemberships] = useState<Membership[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
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
      const me = await meRes.json();
      const mem = await memRes.json();
      setCurrentOrg(me.org_id);
      setMemberships(mem.items ?? []);
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
    setCurrentOrg(orgId);
    setOrgOpen(false);
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
    window.location.href = '/login';
  }

  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '0.75rem',
        padding: '0 1rem',
        borderBottom: '1px solid var(--cos-color-border)',
        background: 'color-mix(in srgb, var(--cos-color-topbar) 92%, transparent)',
        backdropFilter: 'blur(8px)',
        position: 'relative',
      }}
    >
      <div
        style={{
          fontFamily: 'var(--cos-font-display)',
          fontWeight: 650,
          fontSize: '1.15rem',
          letterSpacing: '-0.02em',
          minWidth: 140,
        }}
      >
        CompanyOS
      </div>
      <div style={{ position: 'relative' }}>
        <button
          type="button"
          aria-label="Organization switcher"
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
          </div>
        ) : null}
      </div>
      <div style={{ flex: 1, maxWidth: 420 }}>
        <Input placeholder="Command bar placeholder…" aria-label="Command bar placeholder" readOnly />
      </div>
      <Button variant="secondary">Create</Button>
      <Button variant="ghost" aria-label="Notifications">
        Alerts
      </Button>
      <Button variant="ghost" onClick={onTogglePanel} aria-label="Toggle context panel">
        Panel
      </Button>
      <div style={{ position: 'relative' }}>
        <button
          type="button"
          aria-label="User menu"
          onClick={() => void loadSessions()}
          style={{
            width: 32,
            height: 32,
            borderRadius: '50%',
            border: 'none',
            background: 'var(--cos-color-accent)',
            color: 'var(--cos-color-accent-fg)',
            display: 'grid',
            placeItems: 'center',
            fontSize: '0.75rem',
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          YO
        </button>
        {menuOpen ? (
          <div style={{ ...popover, right: 0, left: 'auto', width: 280 }}>
            <strong style={{ fontSize: '0.85rem' }}>Sessions</strong>
            {sessions.length === 0 ? (
              <p style={{ color: 'var(--cos-color-fg-muted)', fontSize: '0.85rem' }}>
                No active sessions (sign in first).
              </p>
            ) : (
              sessions.map((s) => (
                <div key={s.id} style={{ display: 'grid', gap: 4, marginTop: 8 }}>
                  <span style={{ fontSize: '0.8rem' }}>
                    {s.device_label || 'Device'} {s.current ? '(this)' : ''}
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => void revokeSession(s.id)}
                  >
                    Revoke
                  </Button>
                </div>
              ))
            )}
            <Button type="button" variant="secondary" onClick={() => void logout()}>
              Sign out
            </Button>
            <a href="/login" style={{ fontSize: '0.85rem' }}>
              Sign in
            </a>
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
};
