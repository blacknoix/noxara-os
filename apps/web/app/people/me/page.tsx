'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  ErrorState,
  Input,
  LoadingState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';

type SelfProfile = {
  id: string;
  display_name: string;
  work_email: string | null;
  personal_email: string | null;
  phone: string | null;
  title: string | null;
  status: string;
  location: string | null;
  department_id: string | null;
  start_date: string | null;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'not_found' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; profile: SelfProfile };

export default function MyProfilePage() {
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [displayName, setDisplayName] = useState('');
  const [personalEmail, setPersonalEmail] = useState('');
  const [phone, setPhone] = useState('');
  const [location, setLocation] = useState('');
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch('/api/v1/people/me');
    const requestId = res.headers.get('x-request-id') ?? undefined;
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 404) {
      setState({ status: 'not_found' });
      return;
    }
    if (!res.ok) {
      let message = 'Could not load your profile.';
      try {
        const body = await res.json();
        if (typeof body.detail === 'string') message = body.detail;
      } catch {
        /* ignore */
      }
      setState({ status: 'error', message, requestId });
      return;
    }
    const profile = (await res.json()) as SelfProfile;
    setDisplayName(profile.display_name ?? '');
    setPersonalEmail(profile.personal_email ?? '');
    setPhone(profile.phone ?? '');
    setLocation(profile.location ?? '');
    setState({ status: 'ready', profile });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (state.status !== 'ready') return;
    setFormError(null);
    setSaved(false);
    if (!displayName.trim()) {
      setFormError('Display name is required.');
      return;
    }
    setBusy(true);
    try {
      const res = await authFetch('/api/v1/people/me', {
        method: 'PATCH',
        body: JSON.stringify({
          display_name: displayName.trim(),
          personal_email: personalEmail.trim() || null,
          phone: phone.trim() || null,
          location: location.trim() || null,
        }),
      });
      if (!res.ok) {
        let message = 'Could not update your profile.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setFormError(message);
        return;
      }
      const profile = (await res.json()) as SelfProfile;
      setDisplayName(profile.display_name ?? '');
      setPersonalEmail(profile.personal_email ?? '');
      setPhone(profile.phone ?? '');
      setLocation(profile.location ?? '');
      setState({ status: 'ready', profile });
      setSaved(true);
    } finally {
      setBusy(false);
    }
  }

  if (state.status === 'loading') {
    return <LoadingState label="Loading your profile" rows={4} />;
  }

  if (state.status === 'signed_out') {
    return <ErrorState title="Sign in required" message="Open /login to view your profile." />;
  }

  if (state.status === 'not_found') {
    return (
      <ErrorState
        title="No employee profile"
        message="Your account is not linked to an employee record yet."
      />
    );
  }

  if (state.status === 'error') {
    return <ErrorState message={state.message} requestId={state.requestId} />;
  }

  const { profile } = state;

  return (
    <div style={{ display: 'grid', gap: '1.25rem', maxWidth: 520 }}>
      <header>
        <p style={eyebrow}>People</p>
        <h1 style={h1}>My profile</h1>
        <p style={muted}>Update non-restricted details on your employee record.</p>
      </header>

      <dl style={dlStyle}>
        <dt style={dtStyle}>Work email</dt>
        <dd style={ddStyle}>{profile.work_email ?? '—'}</dd>
        <dt style={dtStyle}>Title</dt>
        <dd style={ddStyle}>{profile.title ?? '—'}</dd>
        <dt style={dtStyle}>Status</dt>
        <dd style={ddStyle}>{profile.status}</dd>
        <dt style={dtStyle}>Department</dt>
        <dd style={ddStyle}>{profile.department_id ?? '—'}</dd>
        <dt style={dtStyle}>Start date</dt>
        <dd style={ddStyle}>{profile.start_date ?? '—'}</dd>
      </dl>

      {formError ? <ErrorState message={formError} /> : null}
      {saved ? (
        <p style={{ margin: 0, color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' }}>
          Profile saved.
        </p>
      ) : null}

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
          label="Personal email"
          name="personal_email"
          type="email"
          value={personalEmail}
          onChange={(e) => setPersonalEmail(e.target.value)}
          autoComplete="email"
        />
        <Input
          label="Phone"
          name="phone"
          type="tel"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          autoComplete="tel"
        />
        <Input
          label="Location"
          name="location"
          value={location}
          onChange={(e) => setLocation(e.target.value)}
        />
        <div style={{ display: 'flex', gap: '0.75rem', flexWrap: 'wrap' }}>
          <Button type="submit" disabled={busy}>
            {busy ? 'Saving…' : 'Save profile'}
          </Button>
          <Link href="/people" style={{ textDecoration: 'none' }}>
            <Button type="button" variant="ghost">
              Directory
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

const dlStyle: CSSProperties = {
  margin: 0,
  display: 'grid',
  gridTemplateColumns: '140px 1fr',
  gap: '0.5rem 1rem',
};

const dtStyle: CSSProperties = {
  fontWeight: 600,
  color: 'var(--cos-color-fg-muted)',
  fontSize: '0.8125rem',
};

const ddStyle: CSSProperties = {
  margin: 0,
  color: 'var(--cos-color-fg)',
};
