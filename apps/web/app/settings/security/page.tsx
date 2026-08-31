'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import { Button, EmptyState, ErrorState, Input, Table } from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Tab = 'access-review' | 'audit' | 'retention' | 'api-keys' | 'sso';

type WhoRow = {
  user_id?: string;
  email?: string;
  role_key?: string;
  permission_id?: string;
  action?: string;
  created_at?: string;
  effective_from?: string;
  effective_to?: string | null;
};

type ApiKey = {
  id: string;
  name: string;
  key_prefix: string;
  scopes: string[];
  expires_at?: string | null;
  revoked_at?: string | null;
  created_at: string;
};

type Retention = {
  default_retention_days: number;
  overrides: Record<string, unknown>;
  version: number;
};

type SsoConfig = {
  id: string;
  protocol: string;
  display_name: string;
  enabled: boolean;
};

export default function SecuritySettingsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [tab, setTab] = useState<Tab>('access-review');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [permissionId, setPermissionId] = useState('hr.payroll.read');
  const [periodStart, setPeriodStart] = useState('2026-03-01T00:00:00Z');
  const [periodEnd, setPeriodEnd] = useState('2026-03-31T23:59:59Z');
  const [couldSee, setCouldSee] = useState<WhoRow[]>([]);
  const [didSee, setDidSee] = useState<WhoRow[]>([]);
  const [exportUrl, setExportUrl] = useState<string | null>(null);

  const [verifyResult, setVerifyResult] = useState<string | null>(null);
  const [retention, setRetention] = useState<Retention | null>(null);
  const [retentionDays, setRetentionDays] = useState('2555');
  const [dryRun, setDryRun] = useState<string | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [newKeyName, setNewKeyName] = useState('');
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null);
  const [ssoConfigs, setSsoConfigs] = useState<SsoConfig[]>([]);

  const canReview = can('admin.access_review.read');
  const canManageReview = can('admin.access_review.manage');
  const canVerify = can('admin.audit.verify');
  const canRetention = can('admin.retention.manage');
  const canApiKeys = can('admin.api_key.manage');
  const canSso = can('admin.sso.manage');

  const denied = !capsLoading && !getAccessToken();

  const runAccessReview = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      const q = `permission_id=${encodeURIComponent(permissionId)}&period_start=${encodeURIComponent(periodStart)}&period_end=${encodeURIComponent(periodEnd)}`;
      const [could, did] = await Promise.all([
        authFetch(`/api/v1/governance/access-review/who-could-see?${q}`),
        authFetch(`/api/v1/governance/access-review/who-did?${q}`),
      ]);
      if (!could.ok || !did.ok) {
        setError('Access review query failed');
        return;
      }
      const couldBody = await could.json();
      const didBody = await did.json();
      setCouldSee(couldBody.items ?? []);
      setDidSee(didBody.items ?? []);

      if (canManageReview) {
        const kick = await authFetch('/api/v1/governance/access-review/runs', {
          method: 'POST',
          headers: {
            'content-type': 'application/json',
            'idempotency-key': `ui-arv-${permissionId}-${periodStart}`,
          },
          body: JSON.stringify({
            permission_id: permissionId,
            period_start: periodStart,
            period_end: periodEnd,
          }),
        });
        if (kick.ok) {
          const run = await kick.json();
          setExportUrl(
            `/api/v1/governance/access-review/runs/${run.id}/export?format=csv`,
          );
        }
      }
    } finally {
      setBusy(false);
    }
  }, [permissionId, periodStart, periodEnd, canManageReview]);

  const runVerify = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const res = await authFetch('/api/v1/governance/audit/verify', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        setError('Audit verify failed');
        return;
      }
      const body = await res.json();
      setVerifyResult(
        body.ok
          ? `OK — ${body.rows_checked} rows / ${body.partitions_checked} partitions`
          : `BREAK — ${body.first_break ?? 'unknown'}`,
      );
    } finally {
      setBusy(false);
    }
  }, []);

  const loadRetention = useCallback(async () => {
    const res = await authFetch('/api/v1/governance/retention');
    if (!res.ok) return;
    const body = await res.json();
    setRetention(body);
    setRetentionDays(String(body.default_retention_days));
  }, []);

  const saveRetention = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      setBusy(true);
      setError(null);
      try {
        const res = await authFetch('/api/v1/governance/retention', {
          method: 'PUT',
          headers: {
            'content-type': 'application/json',
            'idempotency-key': `ui-ret-${retentionDays}`,
          },
          body: JSON.stringify({
            default_retention_days: Number(retentionDays),
          }),
        });
        if (!res.ok) {
          setError('Could not update retention');
          return;
        }
        await loadRetention();
      } finally {
        setBusy(false);
      }
    },
    [retentionDays, loadRetention],
  );

  const runDryRun = useCallback(async () => {
    const res = await authFetch('/api/v1/governance/retention/dry-run', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{}',
    });
    if (!res.ok) {
      setError('Dry-run failed');
      return;
    }
    const body = await res.json();
    setDryRun(
      `Cutoff ${body.cutoff_date}; partitions ${((body.partitions as string[]) ?? []).join(', ') || '(none)'}; estimate ${body.would_affect_estimate}`,
    );
  }, []);

  const loadKeys = useCallback(async () => {
    const res = await authFetch('/api/v1/governance/api-keys');
    if (!res.ok) return;
    const body = await res.json();
    setApiKeys(body.items ?? []);
  }, []);

  const createKey = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      setBusy(true);
      setRevealedSecret(null);
      try {
        const res = await authFetch('/api/v1/governance/api-keys', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ name: newKeyName, scopes: [] }),
        });
        if (!res.ok) {
          setError('Could not create API key');
          return;
        }
        const body = await res.json();
        setRevealedSecret(body.secret);
        setNewKeyName('');
        await loadKeys();
      } finally {
        setBusy(false);
      }
    },
    [newKeyName, loadKeys],
  );

  const rotateKey = useCallback(
    async (id: string) => {
      setBusy(true);
      try {
        const res = await authFetch(`/api/v1/governance/api-keys/${id}/rotate`, {
          method: 'POST',
        });
        if (!res.ok) {
          setError('Rotate failed');
          return;
        }
        const body = await res.json();
        setRevealedSecret(body.secret);
        await loadKeys();
      } finally {
        setBusy(false);
      }
    },
    [loadKeys],
  );

  const loadSso = useCallback(async () => {
    const res = await authFetch('/api/v1/auth/sso/configs');
    if (!res.ok) {
      setSsoConfigs([]);
      return;
    }
    const body = await res.json();
    setSsoConfigs(body.items ?? []);
  }, []);

  useEffect(() => {
    if (tab === 'retention' && canRetention) void loadRetention();
    if (tab === 'api-keys' && canApiKeys) void loadKeys();
    if (tab === 'sso' && canSso) void loadSso();
  }, [tab, canRetention, canApiKeys, canSso, loadRetention, loadKeys, loadSso]);

  if (denied) {
    return <ErrorState message="Sign in to manage security settings." />;
  }

  if (
    !capsLoading &&
    !canReview &&
    !canVerify &&
    !canRetention &&
    !canApiKeys &&
    !canSso
  ) {
    return <ErrorState message="You need an admin governance permission to open this page." />;
  }

  const tabs: { id: Tab; label: string; show: boolean }[] = [
    { id: 'access-review', label: 'Access review', show: canReview },
    { id: 'audit', label: 'Audit verify', show: canVerify },
    { id: 'retention', label: 'Retention', show: canRetention },
    { id: 'api-keys', label: 'API keys', show: canApiKeys },
    { id: 'sso', label: 'SSO', show: canSso },
  ];

  return (
    <main style={styles.page}>
      <header style={styles.header}>
        <p style={styles.eyebrow}>Governance</p>
        <h1 style={styles.title}>Security</h1>
        <p style={styles.lede}>
          Access reviews, audit integrity, retention, API keys, and enterprise SSO.
        </p>
      </header>

      <nav style={styles.tabs} aria-label="Security sections">
        {tabs
          .filter((t) => t.show)
          .map((t) => (
            <button
              key={t.id}
              type="button"
              style={tab === t.id ? styles.tabActive : styles.tab}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
      </nav>

      {error ? <ErrorState message={error} /> : null}

      {tab === 'access-review' && canReview ? (
        <section style={styles.section}>
          <h2 style={styles.h2}>Who could see / who did</h2>
          <p style={styles.hint}>
            Answer payroll access questions from product data (entitlements + audited reads).
          </p>
          <div style={styles.row}>
            <Input
              label="Permission"
              value={permissionId}
              onChange={(e) => setPermissionId(e.target.value)}
            />
            <Input
              label="Period start"
              value={periodStart}
              onChange={(e) => setPeriodStart(e.target.value)}
            />
            <Input
              label="Period end"
              value={periodEnd}
              onChange={(e) => setPeriodEnd(e.target.value)}
            />
            <Button onClick={() => void runAccessReview()} disabled={busy}>
              Run review
            </Button>
          </div>
          {exportUrl ? (
            <p style={styles.hint}>
              <a href={exportUrl}>Download CSV export</a>
            </p>
          ) : null}
          <h3 style={styles.h3}>Who could see</h3>
          {couldSee.length === 0 ? (
            <EmptyState title="No entitlements in range" description="Run a review to populate." />
          ) : (
            <Table
              columns={[
                { key: 'email', header: 'Email', cell: (r: WhoRow) => r.email ?? r.user_id ?? '—' },
                { key: 'role_key', header: 'Role', cell: (r: WhoRow) => r.role_key ?? '—' },
                {
                  key: 'permission_id',
                  header: 'Permission',
                  cell: (r: WhoRow) => r.permission_id ?? permissionId,
                },
                {
                  key: 'effective_from',
                  header: 'From',
                  cell: (r: WhoRow) => r.effective_from ?? '—',
                },
              ]}
              rows={couldSee}
              getRowKey={(_, i) => String(i)}
            />
          )}
          <h3 style={styles.h3}>Who did</h3>
          {didSee.length === 0 ? (
            <EmptyState title="No sensitive reads in range" description="Audited reads appear here." />
          ) : (
            <Table
              columns={[
                { key: 'email', header: 'Email', cell: (r: WhoRow) => r.email ?? r.user_id ?? '—' },
                { key: 'action', header: 'Action', cell: (r: WhoRow) => r.action ?? '—' },
                { key: 'created_at', header: 'When', cell: (r: WhoRow) => r.created_at ?? '—' },
              ]}
              rows={didSee}
              getRowKey={(_, i) => String(i)}
            />
          )}
        </section>
      ) : null}

      {tab === 'audit' && canVerify ? (
        <section style={styles.section}>
          <h2 style={styles.h2}>Hash-chain verification</h2>
          <p style={styles.hint}>Fails closed when a partition chain is broken.</p>
          <Button onClick={() => void runVerify()} disabled={busy}>
            Verify audit log
          </Button>
          {verifyResult ? <p style={styles.result}>{verifyResult}</p> : null}
        </section>
      ) : null}

      {tab === 'retention' && canRetention ? (
        <section style={styles.section}>
          <h2 style={styles.h2}>Data retention</h2>
          <form onSubmit={saveRetention} style={styles.row}>
            <Input
              label="Default retention (days)"
              value={retentionDays}
              onChange={(e) => setRetentionDays(e.target.value)}
            />
            <Button type="submit" disabled={busy}>
              Save
            </Button>
            <Button type="button" onClick={() => void runDryRun()} disabled={busy}>
              Dry-run cutoff
            </Button>
          </form>
          {retention ? (
            <p style={styles.hint}>Version {retention.version}</p>
          ) : null}
          {dryRun ? <p style={styles.result}>{dryRun}</p> : null}
        </section>
      ) : null}

      {tab === 'api-keys' && canApiKeys ? (
        <section style={styles.section}>
          <h2 style={styles.h2}>API keys</h2>
          <form onSubmit={createKey} style={styles.row}>
            <Input
              label="Name"
              value={newKeyName}
              onChange={(e) => setNewKeyName(e.target.value)}
            />
            <Button type="submit" disabled={busy || !newKeyName.trim()}>
              Create
            </Button>
          </form>
          {revealedSecret ? (
            <p style={styles.secret}>
              Copy secret now (shown once): <code>{revealedSecret}</code>
            </p>
          ) : null}
          {apiKeys.length === 0 ? (
            <EmptyState title="No API keys" description="Create a key to call the API." />
          ) : (
            <Table
              columns={[
                { key: 'name', header: 'Name', cell: (k: ApiKey) => k.name },
                { key: 'key_prefix', header: 'Prefix', cell: (k: ApiKey) => k.key_prefix },
                {
                  key: 'status',
                  header: 'Status',
                  cell: (k: ApiKey) => (k.revoked_at ? 'revoked' : 'active'),
                },
                {
                  key: 'actions',
                  header: '',
                  cell: (k: ApiKey) =>
                    k.revoked_at ? (
                      '—'
                    ) : (
                      <Button onClick={() => void rotateKey(k.id)} disabled={busy}>
                        Rotate
                      </Button>
                    ),
                },
              ]}
              rows={apiKeys}
              getRowKey={(k) => k.id}
            />
          )}
        </section>
      ) : null}

      {tab === 'sso' && canSso ? (
        <section style={styles.section}>
          <h2 style={styles.h2}>Enterprise SSO</h2>
          <p style={styles.hint}>
            OIDC configs are gated by enterprise plan / feature flag. SCIM is Phase 4.
          </p>
          {ssoConfigs.length === 0 ? (
            <EmptyState
              title="No SSO configs"
              description="Configure an OIDC IdP when SSO is enabled for this org."
            />
          ) : (
            <Table
              columns={[
                { key: 'display_name', header: 'Name', cell: (c: SsoConfig) => c.display_name },
                { key: 'protocol', header: 'Protocol', cell: (c: SsoConfig) => c.protocol },
                {
                  key: 'enabled',
                  header: 'Enabled',
                  cell: (c: SsoConfig) => (c.enabled ? 'yes' : 'no'),
                },
              ]}
              rows={ssoConfigs}
              getRowKey={(c) => c.id}
            />
          )}
        </section>
      ) : null}
    </main>
  );
}

const styles: Record<string, CSSProperties> = {
  page: {
    maxWidth: 960,
    margin: '0 auto',
    padding: '2rem 1.25rem 4rem',
    fontFamily: '"IBM Plex Sans", "Segoe UI", sans-serif',
    background:
      'radial-gradient(1200px 500px at 10% -10%, #dce8f5 0%, transparent 55%), linear-gradient(180deg, #f7fafc 0%, #eef3f7 100%)',
    minHeight: '100%',
  },
  header: { marginBottom: '1.5rem' },
  eyebrow: {
    textTransform: 'uppercase',
    letterSpacing: '0.08em',
    fontSize: 12,
    color: '#3d5a73',
    margin: 0,
  },
  title: {
    fontFamily: '"Fraunces", "Iowan Old Style", serif',
    fontSize: '2.25rem',
    margin: '0.25rem 0',
    color: '#0f2438',
  },
  lede: { margin: 0, color: '#3d5a73', maxWidth: 520 },
  tabs: { display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: '1.25rem' },
  tab: {
    border: '1px solid #b7c7d6',
    background: 'transparent',
    padding: '0.45rem 0.85rem',
    cursor: 'pointer',
    color: '#0f2438',
  },
  tabActive: {
    border: '1px solid #0f2438',
    background: '#0f2438',
    color: '#f7fafc',
    padding: '0.45rem 0.85rem',
    cursor: 'pointer',
  },
  section: { display: 'grid', gap: '0.75rem' },
  h2: { margin: 0, fontSize: '1.25rem', color: '#0f2438' },
  h3: { margin: '1rem 0 0.35rem', fontSize: '1rem', color: '#0f2438' },
  hint: { margin: 0, color: '#3d5a73', fontSize: 14 },
  row: { display: 'flex', flexWrap: 'wrap', gap: 12, alignItems: 'end' },
  result: {
    margin: 0,
    padding: '0.75rem 1rem',
    background: 'rgba(15, 36, 56, 0.06)',
    color: '#0f2438',
  },
  secret: {
    margin: 0,
    padding: '0.75rem 1rem',
    background: 'rgba(180, 83, 9, 0.12)',
    color: '#0f2438',
    wordBreak: 'break-all',
  },
};
