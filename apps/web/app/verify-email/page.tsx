'use client';

import type { CSSProperties } from 'react';
import { FormEvent, Suspense, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl } from '../../lib/auth-client';

function VerifyEmailPage() {
  const params = useSearchParams();
  const router = useRouter();
  const [token, setToken] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState(false);

  useEffect(() => {
    const t = params.get('token');
    if (t) setToken(t);
  }, [params]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const res = await fetch(apiUrl('/api/v1/auth/verify-email'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'Verification failed');
      return;
    }
    setOk(true);
    window.setTimeout(() => router.push('/login'), 700);
  }

  return (
    <main style={shell}>
      <form onSubmit={onSubmit} style={form}>
        <h1 style={title}>Verify email</h1>
        <p style={muted}>Paste the token from your verification link if it did not auto-fill.</p>
        <Input value={token} onChange={(e) => setToken(e.target.value)} required />
        {error ? <ErrorState message={error} /> : null}
        {ok ? <p style={muted}>Verified — redirecting to sign in.</p> : null}
        <Button type="submit">Verify</Button>
      </form>
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
  gap: '0.75rem',
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
      <VerifyEmailPage />
    </Suspense>
  );
}
