'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type TaxGroup = { id: string; name: string; description?: string | null };
type TaxRate = {
  id: string;
  name: string;
  rate_bps: number;
  valid_from: string;
  valid_to: string | null;
  tax_group_id?: string | null;
};
type DunningProfile = {
  id: string;
  name: string;
  is_default: boolean;
  steps: Array<{ offset_days: number; channel: string; label: string }>;
  version: number;
};
type Entity = {
  id: string;
  name: string;
  code: string;
  currency: string;
  is_default: boolean;
};

export default function FinanceDepthSettingsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [groups, setGroups] = useState<TaxGroup[]>([]);
  const [rates, setRates] = useState<TaxRate[]>([]);
  const [profiles, setProfiles] = useState<DunningProfile[]>([]);
  const [entities, setEntities] = useState<Entity[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [entityName, setEntityName] = useState('');
  const [entityCode, setEntityCode] = useState('');
  const [profileName, setProfileName] = useState('');
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    const [g, r, d, e] = await Promise.all([
      authFetch('/api/v1/finance/tax/groups'),
      authFetch('/api/v1/finance/tax/rates'),
      authFetch('/api/v1/finance/dunning/profiles'),
      authFetch('/api/v1/finance/entities'),
    ]);
    if ([g, r, d, e].some((res) => res.status === 403)) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (![g, r, d, e].every((res) => res.ok)) {
      setError('Could not load finance depth settings');
      setLoading(false);
      return;
    }
    setGroups((await g.json()).items ?? []);
    setRates((await r.json()).items ?? []);
    setProfiles((await d.json()).items ?? []);
    setEntities((await e.json()).items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function createEntity(ev: FormEvent) {
    ev.preventDefault();
    if (!can('finance.entity.manage')) return;
    setSaving(true);
    await authFetch('/api/v1/finance/entities', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        name: entityName.trim(),
        code: entityCode.trim().toUpperCase(),
        currency: 'USD',
        is_default: entities.length === 0,
      }),
    });
    setEntityName('');
    setEntityCode('');
    setSaving(false);
    await load();
  }

  async function createDefaultDunning(ev: FormEvent) {
    ev.preventDefault();
    if (!can('finance.dunning.manage')) return;
    setSaving(true);
    await authFetch('/api/v1/finance/dunning/profiles', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        name: profileName.trim() || 'Standard dunning',
        is_default: profiles.length === 0,
        steps: [
          { offset_days: 3, channel: 'email', label: 'Reminder 1' },
          { offset_days: 7, channel: 'email', label: 'Reminder 2' },
          { offset_days: 14, channel: 'email', label: 'Final notice' },
        ],
      }),
    });
    setProfileName('');
    setSaving(false);
    await load();
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to manage finance settings." />;
  }
  if (capsLoading || loading) return <LoadingState label="Loading finance settings…" />;
  if (denied) {
    return <PermissionDeniedState requiredPermission="finance.tax.read" />;
  }
  if (error) return <ErrorState title="Settings unavailable" message={error} />;

  return (
    <div style={page}>
      <header>
        <p style={eyebrow}>Finance</p>
        <h1 style={title}>Tax, dunning & entities</h1>
        <p style={muted}>
          Versioned tax rates, configurable InvoiceDunning profiles, and multi-entity foundations.
        </p>
      </header>

      <section style={section}>
        <h2 style={sectionTitle}>Legal entities</h2>
        {can('finance.entity.manage') ? (
          <form onSubmit={createEntity} style={form}>
            <input
              value={entityName}
              onChange={(e) => setEntityName(e.target.value)}
              placeholder="Entity name"
              style={input}
            />
            <input
              value={entityCode}
              onChange={(e) => setEntityCode(e.target.value)}
              placeholder="Code"
              style={{ ...input, maxWidth: '8rem' }}
            />
            <Button type="submit" disabled={saving || !entityName.trim() || !entityCode.trim()}>
              Add entity
            </Button>
          </form>
        ) : null}
        {entities.length === 0 ? (
          <EmptyState title="No entities" description="Create the first legal entity for this org." />
        ) : (
          <Table
            getRowKey={(row: Entity) => row.id}
            columns={[
              { key: 'name', header: 'Name', cell: (row: Entity) => row.name },
              { key: 'code', header: 'Code', cell: (row: Entity) => row.code },
              { key: 'currency', header: 'Currency', cell: (row: Entity) => row.currency },
              {
                key: 'default',
                header: 'Default',
                cell: (row: Entity) => (row.is_default ? 'Yes' : '—'),
              },
            ]}
            rows={entities}
          />
        )}
      </section>

      <section style={section}>
        <h2 style={sectionTitle}>Dunning profiles</h2>
        <p style={muted}>
          Profiles drive InvoiceDunning timers. Members can view; only Finance can configure.
        </p>
        {can('finance.dunning.manage') ? (
          <form onSubmit={createDefaultDunning} style={form}>
            <input
              value={profileName}
              onChange={(e) => setProfileName(e.target.value)}
              placeholder="Profile name"
              style={input}
            />
            <Button type="submit" disabled={saving}>
              Create T+3/+7/+14 profile
            </Button>
          </form>
        ) : null}
        {profiles.length === 0 ? (
          <EmptyState title="No dunning profiles" description="Create a profile to schedule reminders." />
        ) : (
          <Table
            getRowKey={(p: DunningProfile) => p.id}
            columns={[
              { key: 'name', header: 'Name', cell: (p: DunningProfile) => p.name },
              {
                key: 'steps',
                header: 'Offsets (days)',
                cell: (p: DunningProfile) => p.steps.map((s) => s.offset_days).join(', '),
              },
              {
                key: 'default',
                header: 'Default',
                cell: (p: DunningProfile) => (p.is_default ? 'Yes' : '—'),
              },
            ]}
            rows={profiles}
          />
        )}
      </section>

      <section style={section}>
        <h2 style={sectionTitle}>Tax groups & rates</h2>
        <p style={muted}>Rates are versioned by validity window — never edited in place.</p>
        {groups.length === 0 && rates.length === 0 ? (
          <EmptyState title="No tax configuration" description="Add a tax group, then append rate versions." />
        ) : (
          <>
            <Table
              getRowKey={(g: TaxGroup) => g.id}
              columns={[
                { key: 'name', header: 'Group', cell: (g: TaxGroup) => g.name },
                { key: 'id', header: 'Id', cell: (g: TaxGroup) => g.id },
              ]}
              rows={groups}
            />
            <Table
              getRowKey={(r: TaxRate) => r.id}
              columns={[
                { key: 'name', header: 'Rate', cell: (r: TaxRate) => r.name },
                {
                  key: 'bps',
                  header: 'bps',
                  cell: (r: TaxRate) => String(r.rate_bps),
                },
                {
                  key: 'window',
                  header: 'Valid',
                  cell: (r: TaxRate) => `${r.valid_from} → ${r.valid_to ?? 'open'}`,
                },
              ]}
              rows={rates}
            />
          </>
        )}
      </section>
    </div>
  );
}

const page: CSSProperties = { padding: '1.5rem', display: 'grid', gap: '1.75rem' };
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem' };
const muted: CSSProperties = { margin: 0, color: 'var(--cos-color-fg-muted)' };
const section: CSSProperties = { display: 'grid', gap: '0.75rem' };
const sectionTitle: CSSProperties = { margin: 0, fontSize: '1.1rem' };
const form: CSSProperties = { display: 'flex', gap: '0.75rem', flexWrap: 'wrap' };
const input: CSSProperties = {
  flex: 1,
  minWidth: '10rem',
  padding: '0.5rem 0.75rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg)',
  color: 'var(--cos-color-fg)',
};
