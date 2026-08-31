'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  Button,
  Card,
  Checkbox,
  EmptyState,
  ErrorState,
  Input,
  PermissionDeniedState,
  Select,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  analyticsPageStyle,
  responseError,
  type MetricDefinition,
  type Report,
} from '../../../lib/analytics';

export default function ReportsPage() {
  const router = useRouter();
  const { caps, can, loading: capsLoading } = useCapabilities();
  const canRead = can('analytics.report.read');
  const canWrite = can('analytics.report.write');
  const [reports, setReports] = useState<Report[]>([]);
  const [metrics, setMetrics] = useState<MetricDefinition[]>([]);
  const [name, setName] = useState('');
  const [metricName, setMetricName] = useState('');
  const [dimensions, setDimensions] = useState<string[]>([]);
  const [visualization, setVisualization] = useState<'table' | 'bar'>('table');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedMetric = useMemo(
    () => metrics.find((metric) => metric.name === metricName),
    [metricName, metrics],
  );

  const load = useCallback(async () => {
    if (!getAccessToken() || !canRead) return;
    setBusy(true);
    setError(null);
    try {
      const [reportsResponse, metricsResponse] = await Promise.all([
        authFetch('/api/v1/analytics/reports'),
        authFetch('/api/v1/analytics/metrics'),
      ]);
      if (!reportsResponse.ok) {
        setError(await responseError(reportsResponse, 'Could not load reports.'));
        return;
      }
      if (!metricsResponse.ok) {
        setError(await responseError(metricsResponse, 'Could not load governed metrics.'));
        return;
      }
      const reportsBody = (await reportsResponse.json()) as { reports?: Report[] };
      const metricsBody = (await metricsResponse.json()) as { metrics?: MetricDefinition[] };
      const nextMetrics = metricsBody.metrics ?? [];
      setReports(reportsBody.reports ?? []);
      setMetrics(nextMetrics);
      setMetricName((current) => current || nextMetrics[0]?.name || '');
    } catch {
      setError('Analytics request failed.');
    } finally {
      setBusy(false);
    }
  }, [canRead]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  useEffect(() => {
    setDimensions((current) =>
      current.filter((dimension) => selectedMetric?.dimensions.includes(dimension)),
    );
  }, [selectedMetric]);

  const toggleDimension = (dimension: string) => {
    setDimensions((current) =>
      current.includes(dimension)
        ? current.filter((item) => item !== dimension)
        : [...current, dimension],
    );
  };

  const createReport = async () => {
    if (!caps?.org_id || !metricName || !name.trim()) return;
    setBusy(true);
    setError(null);
    const response = await authFetch('/api/v1/analytics/reports', {
      method: 'POST',
      body: JSON.stringify({
        name: name.trim(),
        description: '',
        definition: {
          org_id: caps.org_id,
          metric: metricName,
          dimensions,
          filters: [],
          group_by: dimensions,
          visualization,
        },
      }),
    });
    if (!response.ok) {
      setError(await responseError(response, 'Could not create report.'));
      setBusy(false);
      return;
    }
    const report = (await response.json()) as Report;
    router.push(`/insights/reports/${encodeURIComponent(report.id)}`);
  };

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading reports…</p>
      </main>
    );
  }
  if (!canRead) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState title="Reports" requiredPermission="analytics.report.read" />
      </main>
    );
  }

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <header>
        <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>Reports</h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
          Build reusable reports from governed metrics.
        </p>
      </header>

      {error ? <ErrorState message={error} /> : null}

      {canWrite ? (
        <Card as="section" aria-labelledby="create-report-heading">
          <h2 id="create-report-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
            Create report
          </h2>
          <div style={{ display: 'grid', gap: '0.75rem' }}>
            <Input
              label="Report name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Monthly issued revenue"
            />
            <Select
              label="Metric"
              value={metricName}
              onChange={(event) => setMetricName(event.target.value)}
              options={metrics.map((metric) => ({
                value: metric.name,
                label: `${metric.display_name} — ${metric.description}`,
              }))}
            />
            <fieldset
              style={{
                border: '1px solid var(--cos-color-border)',
                borderRadius: 'var(--cos-radius-sm)',
                display: 'flex',
                gap: '1rem',
                flexWrap: 'wrap',
                padding: '0.75rem',
              }}
            >
              <legend style={{ fontWeight: 600, fontSize: '0.8125rem' }}>Dimensions</legend>
              {selectedMetric?.dimensions.map((dimension) => (
                <Checkbox
                  key={dimension}
                  label={dimension.replaceAll('_', ' ')}
                  checked={dimensions.includes(dimension)}
                  onChange={() => toggleDimension(dimension)}
                />
              ))}
              {selectedMetric?.dimensions.length === 0 ? <span>Metric total only</span> : null}
            </fieldset>
            <Select
              label="Visualization"
              value={visualization}
              onChange={(event) => setVisualization(event.target.value as 'table' | 'bar')}
              options={[
                { value: 'table', label: 'Table' },
                { value: 'bar', label: 'Bar chart' },
              ]}
            />
            <div>
              <Button
                onClick={() => void createReport()}
                loading={busy}
                disabled={!name.trim() || !metricName}
              >
                Create report
              </Button>
            </div>
          </div>
        </Card>
      ) : null}

      <section aria-labelledby="saved-reports-heading">
        <h2 id="saved-reports-heading" style={{ fontSize: '1rem' }}>
          Saved reports
        </h2>
        <Table
          getRowKey={(report) => report.id}
          rows={reports}
          columns={[
            {
              key: 'name',
              header: 'Name',
              cell: (report) => (
                <Link href={`/insights/reports/${encodeURIComponent(report.id)}`}>
                  {report.name}
                </Link>
              ),
            },
            {
              key: 'metric',
              header: 'Metric',
              cell: (report) => report.definition.metric,
            },
            {
              key: 'visualization',
              header: 'View',
              cell: (report) => report.visualization,
            },
            {
              key: 'updated',
              header: 'Updated',
              cell: (report) => new Date(report.updated_at).toLocaleString(),
            },
          ]}
          empty={
            <EmptyState
              title={busy ? 'Loading reports…' : 'No reports yet'}
              description="Create a report from a governed metric to get started."
            />
          }
        />
      </section>
    </main>
  );
}
