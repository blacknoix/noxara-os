'use client';

import type { CSSProperties } from 'react';
import { FormEvent, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl, setAccessToken } from '../../lib/auth-client';

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(apiUrl('/api/v1/auth/login'), {
        method: 'POST',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
      });
      const body = await res.json();
      if (res.status === 401 && body.mfa_required) {
        sessionStorage.setItem('cos_mfa_challenge', body.challenge_token);
        router.push('/mfa');
        return;
      }
      if (!res.ok) {
        setError(body.detail ?? body.message ?? `Login failed (${res.status})`);
        return;
      }
      setAccessToken(body.access_token);
      router.push('/');
    } catch {
      setError('Could not reach the auth service.');
    } finally {
      setLoading(false);
    }
  }

  return (
    <main style={shellStyle}>
      <form onSubmit={onSubmit} style={formStyle}>
        <Brand />
        <h1 style={titleStyle}>Sign in</h1>
        <p style={muted}>Use your CompanyOS email and password.</p>
        <label style={label}>
          Email
          <Input value={email} onChange={(e) => setEmail(e.target.value)} type="email" required />
        </label>
        <label style={label}>
          Password
          <Input
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            type="password"
            required
          />
        </label>
        {error ? <ErrorState message={error} /> : null}
        <Button type="submit" disabled={loading}>
          {loading ? 'Signing in…' : 'Sign in'}
        </Button>
        <p style={muted}>
          <a href="/signup">Create an account</a>
          {' · '}
          <a href="/magic-link">Magic link</a>
          {' · '}
          <a href="/reset-password">Reset password</a>
        </p>
      </form>
    </main>
  );
}

function Brand() {
  return (
    <div style={{ fontFamily: 'var(--cos-font-display)', fontSize: '1.5rem', fontWeight: 650 }}>
      CompanyOS
    </div>
  );
}

const shellStyle: CSSProperties = {
  minHeight: '100vh',
  display: 'grid',
  placeItems: 'center',
  padding: '2rem',
  background:
    'radial-gradient(1200px 600px at 10% -10%, #d7efe8 0%, transparent 55%), radial-gradient(900px 500px at 100% 0%, #f0e2c8 0%, transparent 50%), var(--cos-color-bg)',
};

const formStyle: CSSProperties = {
  width: 'min(420px, 100%)',
  display: 'grid',
  gap: '0.85rem',
  padding: '1.5rem',
  background: 'color-mix(in srgb, var(--cos-color-bg-elevated) 92%, transparent)',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.75rem',
};

const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' };
const label: CSSProperties = { display: 'grid', gap: '0.35rem', fontSize: '0.85rem' };
