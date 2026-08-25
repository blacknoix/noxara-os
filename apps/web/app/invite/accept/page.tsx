'use client';

import { useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { authFetch } from '../../../lib/auth-client';

export default function AcceptInvitePage() {
  const [token, setToken] = useState('');
  const [password, setPassword] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const q = new URLSearchParams(window.location.search);
    setToken(q.get('token') ?? '');
  }, []);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const res = await authFetch('/api/v1/workspace/invitations/accept', {
        method: 'POST',
        body: JSON.stringify({
          token,
          password: password || undefined,
          display_name: displayName || undefined,
        }),
      });
      const body = await res.json();
      if (!res.ok) {
        setError(body.detail ?? 'Accept failed');
        return;
      }
      setDone(true);
    } catch {
      setError('Request failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <main style={{ maxWidth: 480, margin: '0 auto', padding: '3rem 1.5rem' }}>
      <p style={{ fontFamily: 'var(--cos-font-display)', fontSize: '1.8rem', fontWeight: 700, margin: 0 }}>
        CompanyOS
      </p>
      <h1 style={{ fontFamily: 'var(--cos-font-display)', fontWeight: 600 }}>Accept invitation</h1>
      {done ? (
        <p>
          You&apos;re in. <a href="/login">Sign in</a> to open the workspace.
        </p>
      ) : (
        <form onSubmit={(e) => void onSubmit(e)} style={{ display: 'grid', gap: '0.75rem' }}>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Invite token
  <Input value={token} onChange={(e) => setToken(e.target.value)} required />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Display name (new accounts)
  <Input
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
          />
</label>
          <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Password (new accounts)
  <Input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
</label>
          {error ? <ErrorState title="Could not accept" message={error} /> : null}
          <Button type="submit" disabled={loading}>
            {loading ? 'Accepting…' : 'Accept invite'}
          </Button>
        </form>
      )}
    </main>
  );
}
