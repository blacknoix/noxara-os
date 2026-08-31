'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Badge,
  Button,
  Card,
  EmptyState,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Select,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';
import {
  analyticsPageStyle,
  responseError,
  type Dashboard,
  type MetricDefinition,
} from '../../../../lib/analytics';

export default function DashboardDesignerPage() {
  const params = useParams();
  const dashboardId = String(params?.id ?? '');
  const { can, loading: capsLoading } = useCapabilities();
  const canRead = can('analytics.dashboard.read');
  const canWrite = can('analytics.dashboard.write');
  const canReadMetrics = can('analytics.report.read');
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [metrics, setMetrics] = useState<MetricDefinition[]>([]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [widgetTitle, setWidgetTitle] = useState('');
  const [metricName, setMetricName] = useState('');
  const [visualization, setVisualization] = useState('stat');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const selectedMetric = useMemo(
    () => metrics.find((metric) => metric.name === metricName),
    [metricName, metrics],
  );

  const load = useCallback(async () => {
    if (!getAccessToken() || !canRead || !dashboardId) return;
    setBusy(true);
    setError(null);
    try {
      const dashboardResponse = await authFetch(
        `/api/v1/analytics/dashboards/${encodeURIComponent(dashboardId)}`,
      );
      if (!dashboardResponse.ok) {
        setError(await responseError(dashboardResponse, 'Could not load dashboard.'));
        return;
      }
      const nextDashboard = (await dashboardResponse.json()) as Dashboard;
      setDashboard(nextDashboard);
      setName(nextDashboard.name);
      setDescription(nextDashboard.description);

      if (canReadMetrics) {
        const metricsResponse = await authFetch('/api/v1/analytics/metrics');
        if (!metricsResponse.ok) {
          setError(await responseError(metricsResponse, 'Could not load governed metrics.'));
          return;
        }
        const metricsBody = (await metricsResponse.json()) as { metrics?: MetricDefinition[] };
        const nextMetrics = metricsBody.metrics ?? [];
        setMetrics(nextMetrics);
        setMetricName((current) => current || nextMetrics[0]?.name || '');
      }
    } catch {
      setError('Analytics request failed.');
    } finally {
      setBusy(false);
    }
  }, [canRead, canReadMetrics, dashboardId]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  const saveDashboard = async () => {
    if (!dashboard) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    const response = await authFetch(
      `/api/v1/analytics/dashboards/${encodeURIComponent(dashboard.id)}`,
      {
        method: 'PATCH',
        body: JSON.stringify({ name: name.trim(), description, layout: dashboard.layout }),
      },
    );
    if (!response.ok) {
      setError(await responseError(response, 'Could not save dashboard.'));
    } else {
      setDashboard((await response.json()) as Dashboard);
      setMessage('Dashboard saved.');
    }
    setBusy(false);
  };

  const addWidget = async () => {
    if (!dashboard || !selectedMetric) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    const response = await authFetch(
      `/api/v1/analytics/dashboards/${encodeURIComponent(dashboard.id)}/widgets`,
      {
        method: 'POST',
        body: JSON.stringify({
          title: widgetTitle.trim() || selectedMetric.display_name,
          metric_name: selectedMetric.name,
          visualization,
          config: {},
          position: dashboard.widgets.length,
        }),
      },
    );
    if (!response.ok) {
      setError(await responseError(response, 'Could not add widget.'));
    } else {
      setWidgetTitle('');
      setMessage('Metric widget added.');
      await load();
    }
    setBusy(false);
  };

  const deleteWidget = async (widgetId: string) => {
    if (!dashboard) return;
    setBusy(true);
    setError(null);
    const response = await authFetch(
      `/api/v1/analytics/dashboards/${encodeURIComponent(
        dashboard.id,
      )}/widgets/${encodeURIComponent(widgetId)}`,
      { method: 'DELETE' },
    );
    if (!response.ok) {
      setError(await responseError(response, 'Could not remove widget.'));
    } else {
      setDashboard({
        ...dashboard,
        widgets: dashboard.widgets.filter((widget) => widget.id !== widgetId),
      });
    }
    setBusy(false);
  };

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading dashboard…</p>
      </main>
    );
  }
  if (!canRead) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState
          title="Dashboard"
          requiredPermission="analytics.dashboard.read"
        />
      </main>
    );
  }

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <p style={{ margin: 0 }}>
        <Link href="/insights/dashboards">← Dashboards</Link>
      </p>
      <header>
        <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>
          {dashboard?.name || 'Dashboard designer'}
        </h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
          Every widget is bound to a governed metric name.
        </p>
      </header>
      {error ? <ErrorState message={error} /> : null}
      {message ? <InlineAlert tone="success">{message}</InlineAlert> : null}

      {dashboard && canWrite ? (
        <Card as="section" aria-labelledby="dashboard-settings-heading">
          <h2 id="dashboard-settings-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
            Dashboard settings
          </h2>
          <div style={{ display: 'grid', gap: '0.75rem' }}>
            <Input
              label="Name"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            <Input
              label="Description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
            <div>
              <Button
                onClick={() => void saveDashboard()}
                loading={busy}
                disabled={!name.trim()}
              >
                Save dashboard
              </Button>
            </div>
          </div>
        </Card>
      ) : null}

      {canWrite && !canReadMetrics ? (
        <InlineAlert tone="warning" title="Metric catalogue unavailable">
          analytics.report.read is required to choose governed metrics for new widgets.
        </InlineAlert>
      ) : null}

      {dashboard && canWrite && canReadMetrics ? (
        <Card as="section" aria-labelledby="add-widget-heading">
          <h2 id="add-widget-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
            Add metric widget
          </h2>
          <div style={{ display: 'grid', gap: '0.75rem' }}>
            <Input
              label="Widget title"
              value={widgetTitle}
              onChange={(event) => setWidgetTitle(event.target.value)}
              placeholder={selectedMetric?.display_name ?? 'Metric title'}
            />
            <Select
              label="Governed metric"
              value={metricName}
              onChange={(event) => setMetricName(event.target.value)}
              options={metrics.map((metric) => ({
                value: metric.name,
                label: `${metric.display_name} (${metric.name})`,
              }))}
            />
            <Select
              label="Visualization"
              value={visualization}
              onChange={(event) => setVisualization(event.target.value)}
              options={[
                { value: 'stat', label: 'Statistic' },
                { value: 'bar', label: 'Bar chart' },
                { value: 'table', label: 'Table' },
              ]}
            />
            <div>
              <Button
                onClick={() => void addWidget()}
                loading={busy}
                disabled={!metricName}
              >
                Add widget
              </Button>
            </div>
          </div>
        </Card>
      ) : null}

      <section aria-labelledby="dashboard-widgets-heading">
        <h2 id="dashboard-widgets-heading" style={{ fontSize: '1rem' }}>
          Widgets
        </h2>
        {!dashboard || dashboard.widgets.length === 0 ? (
          <EmptyState
            title={busy ? 'Loading widgets…' : 'No widgets yet'}
            description="Add a widget using a metric from the governed catalogue."
          />
        ) : (
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(15rem, 1fr))',
              gap: '0.75rem',
            }}
          >
            {dashboard.widgets.map((widget) => (
              <Card key={widget.id} as="article">
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'flex-start',
                    gap: '0.75rem',
                  }}
                >
                  <div>
                    <h3 style={{ margin: 0, fontSize: '1rem' }}>{widget.title}</h3>
                    <code style={{ fontSize: '0.8rem' }}>{widget.metric_name}</code>
                  </div>
                  <Badge tone="accent">{widget.visualization}</Badge>
                </div>
                {canWrite ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void deleteWidget(widget.id)}
                    loading={busy}
                    style={{ marginTop: '1rem' }}
                  >
                    Remove
                  </Button>
                ) : null}
              </Card>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}
