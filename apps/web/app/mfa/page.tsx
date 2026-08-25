'use client';

import type { CSSProperties } from 'react';
import { FormEvent, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl, setAccessToken } from '../../lib/auth-client';

export default function MfaPage() {
  const router = useRouter();
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [setupSecret, setSetupSecret] = useState<string | null>(null);
  const challenge =
    typeof window !== 'undefined' ? sessionStorage.getItem('cos_mfa_challenge') : null;

  async function startSetup() {
    if (!challenge) return;
    const res = await fetch(apiUrl('/api/v1/auth/mfa/setup'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ challenge_token: challenge }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'MFA setup failed');
      return;
    }
    setSetupSecret(body.secret);
  }

  async function confirmSetup() {
    if (!challenge || !code) return;
    const res = await fetch(apiUrl('/api/v1/auth/mfa/confirm'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ challenge_token: challenge, code }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'MFA confirm failed');
      return;
    }
    setError(null);
    alert(`Save recovery codes:\n${(body.recovery_codes as string[]).join('\n')}`);
  }

  async function verify(e: FormEvent) {
    e.preventDefault();
    if (!challenge) {
      setError('Missing MFA challenge — sign in again.');
      return;
    }
    const res = await fetch(apiUrl('/api/v1/auth/mfa/verify'), {
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ challenge_token: challenge, code }),
    });
    const body = await res.json();
    if (!res.ok) {
      setError(body.detail ?? 'Invalid MFA code');
      return;
    }
    setAccessToken(body.access_token);
    sessionStorage.removeItem('cos_mfa_challenge');
    router.push('/');
  }

  return (
    <main style={shell}>
      <form onSubmit={verify} style={form}>
        <h1 style={title}>Multi-factor challenge</h1>
        <p style={muted}>Owner and Admin roles require TOTP before an access token is issued.</p>
        {setupSecret ? (
          <p style={muted}>
            Secret: <code>{setupSecret}</code>
          </p>
        ) : (
          <Button type="button" variant="secondary" onClick={startSetup}>
            Enroll authenticator
          </Button>
        )}
        {setupSecret ? (
          <Button type="button" variant="secondary" onClick={confirmSetup}>
            Confirm enrollment
          </Button>
        ) : null}
        <label style={label}>
          Authenticator code
          <Input value={code} onChange={(e) => setCode(e.target.value)} required />
        </label>
        {error ? <ErrorState message={error} /> : null}
        <Button type="submit">Continue</Button>
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
const label: CSSProperties = { display: 'grid', gap: '0.35rem', fontSize: '0.85rem' };
