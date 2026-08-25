'use client';

import type { CSSProperties } from 'react';
import { FormEvent, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, ErrorState, Input } from '@companyos/design-system';
import { apiUrl } from '../../lib/auth-client';

export default function SignupPage() {
  const router = useRouter();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [orgName, setOrgName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [loading, setLoading] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(apiUrl('/api/v1/auth/register'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email,
          password,
          display_name: displayName,
          org_name: orgName,
        }),
      });
      const body = await res.json();
      if (!res.ok) {
        setError(body.detail ?? `Signup failed (${res.status})`);
        return;
      }
      setDone(true);
      window.setTimeout(() => router.push('/verify-email'), 800);
    } catch {
      setError('Could not reach the auth service.');
    } finally {
      setLoading(false);
    }
  }

  return (
    <main style={shell}>
      <form onSubmit={onSubmit} style={form}>
        <div style={{ fontFamily: 'var(--cos-font-display)', fontSize: '1.5rem', fontWeight: 650 }}>
          CompanyOS
        </div>
        <h1 style={title}>Create your workspace</h1>
        {done ? (
          <p style={muted}>Check your email (or local mail log) to verify, then sign in.</p>
        ) : (
          <>
            <label style={label}>
              Work email
              <Input value={email} onChange={(e) => setEmail(e.target.value)} type="email" required />
            </label>
            <label style={label}>
              Display name
              <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
            </label>
            <label style={label}>
              Organization
              <Input value={orgName} onChange={(e) => setOrgName(e.target.value)} required />
            </label>
            <label style={label}>
              Password
              <Input
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                type="password"
                required
                minLength={10}
              />
            </label>
            {error ? <ErrorState message={error} /> : null}
            <Button type="submit" disabled={loading}>
              {loading ? 'Creating…' : 'Sign up'}
            </Button>
          </>
        )}
        <p style={muted}>
          <a href="/login">Already have an account?</a>
        </p>
      </form>
    </main>
  );
}

const shell: CSSProperties = {
  minHeight: '100vh',
  display: 'grid',
  placeItems: 'center',
  padding: '2rem',
  background:
    'radial-gradient(1000px 500px at 0% 0%, #d7efe8 0%, transparent 55%), var(--cos-color-bg)',
};
const form: CSSProperties = {
  width: 'min(440px, 100%)',
  display: 'grid',
  gap: '0.85rem',
  padding: '1.5rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg-elevated)',
};
const title: CSSProperties = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.75rem',
};
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' };
const label: CSSProperties = { display: 'grid', gap: '0.35rem', fontSize: '0.85rem' };
