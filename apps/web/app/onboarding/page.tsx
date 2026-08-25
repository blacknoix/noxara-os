'use client';

import { useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import { Button, EmptyState, ErrorState, Input } from '@companyos/design-system';
import { authFetch, getAccessToken, setAccessToken } from '../../lib/auth-client';
import { clearCapabilitiesCache } from '../../lib/capabilities';

export default function CreateOrgPage() {
  const [name, setName] = useState('');
  const [businessType, setBusinessType] = useState('general');
  const [currency, setCurrency] = useState('USD');
  const [timezone, setTimezone] = useState('UTC');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [authed, setAuthed] = useState(false);

  useEffect(() => {
    setAuthed(!!getAccessToken());
  }, []);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const res = await authFetch('/api/v1/workspace/organizations', {
        method: 'POST',
        body: JSON.stringify({
          name,
          business_type: businessType,
          currency,
          timezone,
        }),
      });
      const body = await res.json();
      if (!res.ok) {
        setError(body.detail ?? 'Could not create organization');
        return;
      }
      // Switch into the new org
      const sw = await authFetch('/api/v1/auth/switch-org', {
        method: 'POST',
        body: JSON.stringify({ org_id: body.org_id }),
      });
      if (sw.ok) {
        const tok = await sw.json();
        setAccessToken(tok.access_token);
        clearCapabilitiesCache();
      }
      window.location.href = '/settings';
    } catch {
      setError('Request failed');
    } finally {
      setLoading(false);
    }
  }

  if (!authed) {
    return (
      <main style={shell}>
        <ErrorState title="Sign in required" message="Create an account or sign in first." />
        <p style={{ marginTop: '1rem' }}>
          <a href="/login">Sign in</a> · <a href="/signup">Sign up</a>
        </p>
      </main>
    );
  }

  return (
    <main style={shell}>
      <p style={brand}>CompanyOS</p>
      <h1 style={title}>Create your workspace</h1>
      <p style={lede}>Name the organization you will operate from. Defaults seed during provisioning.</p>
      <form onSubmit={(e) => void onSubmit(e)} style={{ display: 'grid', gap: '0.85rem', maxWidth: 420 }}>
        <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Organization name
  <Input value={name} onChange={(e) => setName(e.target.value)} required />
</label>
        <label style={label}>
          Business type
          <select value={businessType} onChange={(e) => setBusinessType(e.target.value)} style={select}>
            <option value="general">General</option>
            <option value="agency">Agency</option>
            <option value="retail">Retail</option>
          </select>
        </label>
        <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Currency
  <Input value={currency} onChange={(e) => setCurrency(e.target.value)} />
</label>
        <label style={{ display: "grid", gap: "0.35rem", fontSize: "0.85rem", color: "var(--cos-color-fg-muted)" }}>
  Timezone
  <Input value={timezone} onChange={(e) => setTimezone(e.target.value)} />
</label>
        {error ? <ErrorState title="Could not create" message={error} /> : null}
        <Button type="submit" disabled={loading}>
          {loading ? 'Provisioning…' : 'Create organization'}
        </Button>
      </form>
      <EmptyState
        title="What happens next"
        description="System roles, settings, and seed defaults are applied via OrgProvisioning — no manual steps."
      />
    </main>
  );
}

const shell: CSSProperties = {
  maxWidth: 640,
  margin: '0 auto',
  padding: '3rem 1.5rem',
};

const brand: CSSProperties = {
  fontFamily: 'var(--cos-font-display)',
  fontSize: '2rem',
  fontWeight: 700,
  letterSpacing: '-0.03em',
  margin: '0 0 0.5rem',
};

const title: CSSProperties = {
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.6rem',
  fontWeight: 600,
  margin: '0 0 0.35rem',
};

const lede: CSSProperties = {
  color: 'var(--cos-color-fg-muted)',
  margin: '0 0 1.5rem',
};

const label: CSSProperties = {
  display: 'grid',
  gap: '0.35rem',
  fontSize: '0.9rem',
  color: 'var(--cos-color-fg-muted)',
};

const select: CSSProperties = {
  padding: '0.55rem 0.7rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
