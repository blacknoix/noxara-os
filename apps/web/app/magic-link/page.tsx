'use client';

import type { CSSProperties } from 'react';
import { FormEvent, Suspense, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl, setAccessToken } from '../../lib/auth-client';

function MagicLinkPage() {
  const params = useSearchParams();
  const router = useRouter();
  const [email, setEmail] = useState('');
  const [token, setToken] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const t = params.get('token');
    if (t) setToken(t);
  }, [params]);

  async function requestLink(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const res = await fetch(apiUrl('/api/v1/auth/magic-link'), {
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

  async function consume(e: FormEvent) {
    e.preventDefault();
    const res = await fetch(apiUrl('/api/v1/auth/magic-link/consume'), {
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    const body = await res.json();
    if (res.status === 401 && body.mfa_required) {
      sessionStorage.setItem('cos_mfa_challenge', body.challenge_token);
      router.push('/mfa');
      return;
    }
    if (!res.ok) {
      setError(body.detail ?? 'Invalid magic link');
      return;
    }
    setAccessToken(body.access_token);
    router.push('/');
  }

  return (
    <main style={shell}>
      <div style={form}>
        <h1 style={title}>Magic link</h1>
        <form onSubmit={requestLink} style={{ display: 'grid', gap: '0.75rem' }}>
          <Input
            placeholder="you@company.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            type="email"
            required
          />
          <Button type="submit">Send link</Button>
        </form>
        <form onSubmit={consume} style={{ display: 'grid', gap: '0.75rem' }}>
          <Input
            placeholder="Token from email"
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
          <Button type="submit" variant="secondary">
            Open link
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
      <MagicLinkPage />
    </Suspense>
  );
}
