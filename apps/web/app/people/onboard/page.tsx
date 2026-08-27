'use client';

import { useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  Button,
  ErrorState,
  Input,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

export default function OnboardEmployeePage() {
  const router = useRouter();
  const { can, loading: capsLoading } = useCapabilities();

  const [displayName, setDisplayName] = useState('');
  const [workEmail, setWorkEmail] = useState('');
  const [title, setTitle] = useState('');
  const [startDate, setStartDate] = useState('');
  const [userId, setUserId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to onboard an employee." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (!can('hr.employee.onboard')) {
    return <PermissionDeniedState requiredPermission="hr.employee.onboard" />;
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setRequestId(undefined);
    if (!displayName.trim()) {
      setError('Display name is required.');
      return;
    }
    setBusy(true);
    try {
      const body: Record<string, string> = {
        display_name: displayName.trim(),
      };
      if (workEmail.trim()) body.work_email = workEmail.trim();
      if (title.trim()) body.title = title.trim();
      if (startDate.trim()) body.start_date = startDate.trim();
      if (userId.trim()) body.user_id = userId.trim();

      const res = await authFetch('/api/v1/people/employees/onboard', {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify(body),
      });
      const rid = res.headers.get('x-request-id') ?? undefined;
      setRequestId(rid);
      if (!res.ok) {
        let message = 'Could not start onboarding.';
        try {
          const payload = await res.json();
          if (typeof payload.detail === 'string') message = payload.detail;
        } catch {
          /* ignore */
        }
        setError(message);
        return;
      }
      const payload = (await res.json()) as { employee?: { id?: string } };
      const id = payload.employee?.id;
      if (id) {
        router.push(`/people/${id}`);
      } else {
        router.push('/people');
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem', maxWidth: 520 }}>
      <header>
        <p style={eyebrow}>
          <Link href="/people" style={{ color: 'inherit', textDecoration: 'none' }}>
            People / Directory
          </Link>
        </p>
        <h1 style={h1}>Onboard employee</h1>
        <p style={muted}>Create an employee record and kick off the onboarding workflow.</p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      <form onSubmit={(e) => void onSubmit(e)} style={{ display: 'grid', gap: '1rem' }}>
        <Input
          label="Display name"
          name="display_name"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          required
          autoComplete="name"
        />
        <Input
          label="Work email"
          name="work_email"
          type="email"
          value={workEmail}
          onChange={(e) => setWorkEmail(e.target.value)}
          autoComplete="email"
        />
        <Input
          label="Title"
          name="title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <Input
          label="Start date"
          name="start_date"
          type="date"
          value={startDate}
          onChange={(e) => setStartDate(e.target.value)}
        />
        <Input
          label="User ID (optional)"
          name="user_id"
          value={userId}
          onChange={(e) => setUserId(e.target.value)}
          placeholder="usr_…"
        />
        <div style={{ display: 'flex', gap: '0.75rem', flexWrap: 'wrap' }}>
          <Button type="submit" disabled={busy}>
            {busy ? 'Starting…' : 'Start onboarding'}
          </Button>
          <Link href="/people" style={{ textDecoration: 'none' }}>
            <Button type="button" variant="ghost">
              Cancel
            </Button>
          </Link>
        </div>
      </form>
    </div>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const h1: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: 'clamp(1.75rem, 2.5vw, 2.25rem)',
  fontWeight: 650,
  letterSpacing: '-0.02em',
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 560,
};
