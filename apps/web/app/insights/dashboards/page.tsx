'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  Badge,
  Button,
  Card,
  EmptyState,
  ErrorState,
  Input,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  analyticsPageStyle,
  responseError,
  type Dashboard,
} from '../../../lib/analytics';

export default function DashboardsPage() {
  const router = useRouter();
  const { can, loading: capsLoading } = useCapabilities();
  const canRead = can('analytics.dashboard.read');
  const canWrite = can('analytics.dashboard.write');
  const [dashboards, setDashboards] = useState<Dashboard[]>([]);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken() || !canRead) return;
    setBusy(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/analytics/dashboards');
      if (!response.ok) {
        setError(await responseError(response, 'Could not load dashboards.'));
        return;
      }
      const body = (await response.json()) as { dashboards?: Dashboard[] };
      setDashboards(body.dashboards ?? []);
    } catch {
      setError('Analytics request failed.');
    } finally {
      setBusy(false);
    }
  }, [canRead]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  const createDashboard = async () => {
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    const response = await authFetch('/api/v1/analytics/dashboards', {
      method: 'POST',
      body: JSON.stringify({ name: name.trim(), description: '', layout: [] }),
    });
    if (!response.ok) {
      setError(await responseError(response, 'Could not create dashboard.'));
      setBusy(false);
      return;
    }
    const dashboard = (await response.json()) as Dashboard;
    router.push(`/insights/dashboards/${encodeURIComponent(dashboard.id)}`);
  };

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading dashboards…</p>
      </main>
    );
  }
  if (!canRead) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState
          title="Dashboards"
          requiredPermission="analytics.dashboard.read"
        />
      </main>
    );
  }

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <header>
        <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>Dashboards</h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
          Arrange governed metric widgets into reusable views.
        </p>
      </header>
      {error ? <ErrorState message={error} /> : null}

      {canWrite ? (
        <Card as="section" aria-labelledby="new-dashboard-heading">
          <h2 id="new-dashboard-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
            New dashboard
          </h2>
          <div style={{ display: 'flex', alignItems: 'flex-end', gap: '0.75rem' }}>
            <Input
              label="Dashboard name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Executive overview"
            />
            <Button
              onClick={() => void createDashboard()}
              loading={busy}
              disabled={!name.trim()}
              style={{ flexShrink: 0 }}
            >
              Create
            </Button>
          </div>
        </Card>
      ) : null}

      <section aria-labelledby="dashboard-list-heading">
        <h2 id="dashboard-list-heading" style={{ fontSize: '1rem' }}>
          Saved dashboards
        </h2>
        {dashboards.length === 0 ? (
          <EmptyState
            title={busy ? 'Loading dashboards…' : 'No dashboards yet'}
            description="Create a dashboard, then bind widgets to governed metric names."
          />
        ) : (
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(15rem, 1fr))',
              gap: '0.75rem',
            }}
          >
            {dashboards.map((dashboard) => (
              <Card key={dashboard.id} as="article">
                <Link
                  href={`/insights/dashboards/${encodeURIComponent(dashboard.id)}`}
                  style={{ fontWeight: 650, fontSize: '1rem' }}
                >
                  {dashboard.name}
                </Link>
                <p style={{ color: 'var(--cos-color-fg-muted)', minHeight: '2.5rem' }}>
                  {dashboard.description || 'No description'}
                </p>
                <Badge>{dashboard.widgets.length} widgets</Badge>
              </Card>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}
