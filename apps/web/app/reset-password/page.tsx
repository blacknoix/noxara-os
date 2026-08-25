'use client';

import type { CSSProperties } from 'react';
import { FormEvent, Suspense, useEffect, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl } from '../../lib/auth-client';

function ResetPasswordPage() {
  const params = useSearchParams();
  const [email, setEmail] = useState('');
  const [token, setToken] = useState('');
  const [password, setPassword] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const t = params.get('token');
    if (t) setToken(t);
  }, [params]);

  async function requestReset(e: FormEvent) {
    e.preventDefault();
    const res = await fetch(apiUrl('/api/v1/auth/password-reset/request'), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Idempotency-Key': crypto.randomUUID(),
      },
      body: JSON.stringify({ email }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'Request failed');
      return;
    }
    setMessage(body.message);
  }

  async function confirm(e: FormEvent) {
    e.preventDefault();
    const res = await fetch(apiUrl('/api/v1/auth/password-reset/confirm'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token, new_password: password }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'Reset failed');
      return;
    }
    setMessage(body.message);
  }

  return (
    <main style={shell}>
      <div style={form}>
        <h1 style={title}>Reset password</h1>
        <form onSubmit={requestReset} style={{ display: 'grid', gap: '0.75rem' }}>
          <Input
            type="email"
            placeholder="you@company.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <Button type="submit">Send reset email</Button>
        </form>
        <form onSubmit={confirm} style={{ display: 'grid', gap: '0.75rem' }}>
          <Input placeholder="Reset token" value={token} onChange={(e) => setToken(e.target.value)} />
          <Input
            type="password"
            placeholder="New password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            minLength={10}
          />
          <Button type="submit" variant="secondary">
            Set new password
          </Button>
        </form>
        {message ? <p style={muted}>{message}</p> : null}
        {error ? <ErrorState message={error} /> : null}
      </div>
    </main>
  );
}

const shell: CSSProperties = {
  minHeight: '100vh',
  display: 'grid',
  placeItems: 'center',
  background: 'var(--cos-color-bg)',
};
const form: CSSProperties = {
  width: 'min(420px, 100%)',
  display: 'grid',
  gap: '1rem',
  padding: '1.5rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg-elevated)',
};
const title: CSSProperties = { margin: 0, fontFamily: 'var(--cos-font-display)' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };


export default function Page() {
  return (
    <Suspense fallback={<main style={{ minHeight: '100vh', display: 'grid', placeItems: 'center' }}>Loading…</main>}>
      <ResetPasswordPage />
    </Suspense>
  );
}
