'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Button,
  Card,
  Chart,
  Checkbox,
  EmptyState,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Select,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';
import {
  analyticsPageStyle,
  dimensionLabel,
  formatMetricValue,
  responseError,
  type MetricDefinition,
  type QueryRow,
  type Report,
  type RunReportResponse,
} from '../../../../lib/analytics';

export default function ReportBuilderPage() {
  const params = useParams();
  const reportId = String(params?.id ?? '');
  const { caps, can, loading: capsLoading } = useCapabilities();
  const canRead = can('analytics.report.read');
  const canWrite = can('analytics.report.write');
  const canRun = can('analytics.report.run');
  const canExport = can('analytics.report.export');
  const [report, setReport] = useState<Report | null>(null);
  const [metrics, setMetrics] = useState<MetricDefinition[]>([]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [metricName, setMetricName] = useState('');
  const [dimensions, setDimensions] = useState<string[]>([]);
  const [visualization, setVisualization] = useState<'table' | 'bar'>('table');
  const [result, setResult] = useState<RunReportResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const metric = useMemo(
    () => metrics.find((item) => item.name === metricName),
    [metricName, metrics],
  );

  const load = useCallback(async () => {
    if (!getAccessToken() || !canRead || !reportId) return;
    setBusy(true);
    setError(null);
    try {
      const [reportResponse, metricsResponse] = await Promise.all([
        authFetch(`/api/v1/analytics/reports/${encodeURIComponent(reportId)}`),
        authFetch('/api/v1/analytics/metrics'),
      ]);
      if (!reportResponse.ok) {
        setError(await responseError(reportResponse, 'Could not load report.'));
        return;
      }
      if (!metricsResponse.ok) {
        setError(await responseError(metricsResponse, 'Could not load governed metrics.'));
        return;
      }
      const nextReport = (await reportResponse.json()) as Report;
      const metricsBody = (await metricsResponse.json()) as { metrics?: MetricDefinition[] };
      setReport(nextReport);
      setName(nextReport.name);
      setDescription(nextReport.description);
      setMetricName(nextReport.definition.metric);
      setDimensions(nextReport.definition.dimensions);
      setVisualization(nextReport.definition.visualization === 'bar' ? 'bar' : 'table');
      setMetrics(metricsBody.metrics ?? []);
    } catch {
      setError('Analytics request failed.');
    } finally {
      setBusy(false);
    }
  }, [canRead, reportId]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  useEffect(() => {
    setDimensions((current) =>
      current.filter((dimension) => metric?.dimensions.includes(dimension)),
    );
  }, [metric]);

  const toggleDimension = (dimension: string) => {
    setDimensions((current) =>
      current.includes(dimension)
        ? current.filter((item) => item !== dimension)
        : [...current, dimension],
    );
  };

  const save = async () => {
    if (!caps?.org_id || !report) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    const response = await authFetch(`/api/v1/analytics/reports/${encodeURIComponent(report.id)}`, {
      method: 'PATCH',
      body: JSON.stringify({
        name: name.trim(),
        description,
        definition: {
          org_id: caps.org_id,
          metric: metricName,
          dimensions,
          filters: report.definition.filters,
          group_by: dimensions,
          visualization,
        },
      }),
    });
    if (!response.ok) {
      setError(await responseError(response, 'Could not save report.'));
    } else {
      setReport((await response.json()) as Report);
      setMessage('Report saved.');
    }
    setBusy(false);
  };

  const run = async () => {
    if (!report) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    const response = await authFetch(
      `/api/v1/analytics/reports/${encodeURIComponent(report.id)}/run`,
      {
        method: 'POST',
        body: JSON.stringify({ dry_run: false }),
      },
    );
    if (!response.ok) {
      setError(await responseError(response, 'Could not run report.'));
    } else {
      const runResult = (await response.json()) as RunReportResponse;
      setResult(runResult);
      setMessage(`Report completed in ${runResult.result.elapsed_ms}ms.`);
    }
    setBusy(false);
  };

  const exportCsv = async () => {
    if (!report) return;
    setBusy(true);
    setError(null);
    const response = await authFetch(
      `/api/v1/analytics/reports/${encodeURIComponent(report.id)}/export`,
      {
        method: 'POST',
        body: JSON.stringify({ format: 'csv' }),
      },
    );
    if (!response.ok) {
      setError(await responseError(response, 'Could not export report.'));
      setBusy(false);
      return;
    }
    const exported = (await response.json()) as { content: string };
    const url = URL.createObjectURL(new Blob([exported.content], { type: 'text/csv' }));
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${report.name.replaceAll(/[^a-z0-9]+/gi, '-').toLowerCase() || 'report'}.csv`;
    anchor.click();
    URL.revokeObjectURL(url);
    setMessage('CSV export downloaded.');
    setBusy(false);
  };

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading report…</p>
      </main>
    );
  }
  if (!canRead) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState title="Report" requiredPermission="analytics.report.read" />
      </main>
    );
  }

  const rows = result?.result.rows ?? [];
  const maxValue = Math.max(1, ...rows.map((row) => Math.abs(row.value)));

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <p style={{ margin: 0 }}>
        <Link href="/insights/reports">← Reports</Link>
      </p>
      <header>
        <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>
          {report?.name || 'Report builder'}
        </h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
          Configure governed dimensions, run the report, and drill into source records.
        </p>
      </header>

      {error ? <ErrorState message={error} /> : null}
      {message ? <InlineAlert tone="success">{message}</InlineAlert> : null}

      {report ? (
        <Card as="section" aria-labelledby="report-definition-heading">
          <h2 id="report-definition-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
            Definition
          </h2>
          <div style={{ display: 'grid', gap: '0.75rem' }}>
            <Input
              label="Report name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              disabled={!canWrite}
            />
            <Input
              label="Description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              disabled={!canWrite}
            />
            <Select
              label="Metric"
              value={metricName}
              onChange={(event) => setMetricName(event.target.value)}
              disabled={!canWrite}
              options={metrics.map((item) => ({
                value: item.name,
                label: `${item.display_name} — ${item.description}`,
              }))}
            />
            <fieldset
              disabled={!canWrite}
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
              {metric?.dimensions.map((dimension) => (
                <Checkbox
                  key={dimension}
                  label={dimension.replaceAll('_', ' ')}
                  checked={dimensions.includes(dimension)}
                  onChange={() => toggleDimension(dimension)}
                />
              ))}
            </fieldset>
            <Select
              label="Visualization"
              value={visualization}
              onChange={(event) => setVisualization(event.target.value as 'table' | 'bar')}
              disabled={!canWrite}
              options={[
                { value: 'table', label: 'Table' },
                { value: 'bar', label: 'Bar chart' },
              ]}
            />
            <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
              {canWrite ? (
                <Button
                  onClick={() => void save()}
                  loading={busy}
                  disabled={!name.trim() || !metricName}
                >
                  Save report
                </Button>
              ) : null}
              {canRun ? (
                <Button variant="secondary" onClick={() => void run()} loading={busy}>
                  Run report
                </Button>
              ) : null}
              {canExport ? (
                <Button variant="secondary" onClick={() => void exportCsv()} loading={busy}>
                  Export CSV
                </Button>
              ) : null}
            </div>
          </div>
        </Card>
      ) : null}

      {result?.result.permission_denied_empty ? (
        <InlineAlert tone="warning" title="Rows filtered by permission">
          You can run this report, but its source facts require {metric?.required_permission}.
        </InlineAlert>
      ) : null}

      <section aria-labelledby="report-results-heading">
        <h2 id="report-results-heading" style={{ fontSize: '1rem' }}>
          Results
        </h2>
        {visualization === 'bar' ? (
          <Chart
            title={metric?.display_name ?? metricName}
            description="Bars use the returned grouped values."
            height={Math.max(180, rows.length * 44)}
            empty={rows.length === 0}
            emptyMessage={
              result ? 'No rows are visible for this report.' : 'Run the report to chart results.'
            }
          >
            <div
              role="img"
              aria-label={`${metric?.display_name ?? metricName} bar chart`}
              style={{ width: '100%', padding: '1rem', display: 'grid', gap: '0.65rem' }}
            >
              {rows.map((row, index) => (
                <div key={`${dimensionLabel(row.dimensions)}-${index}`}>
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: '1rem',
                      fontSize: '0.8rem',
                    }}
                  >
                    <span>{dimensionLabel(row.dimensions)}</span>
                    <strong>{formatMetricValue(row.value, metric?.unit ?? 'count')}</strong>
                  </div>
                  <div
                    aria-hidden
                    style={{
                      height: '0.65rem',
                      marginTop: '0.2rem',
                      background: 'var(--cos-color-bg-muted)',
                      borderRadius: 'var(--cos-radius-sm)',
                    }}
                  >
                    <div
                      style={{
                        width: `${(Math.abs(row.value) / maxValue) * 100}%`,
                        minWidth: row.value === 0 ? 0 : '0.25rem',
                        height: '100%',
                        background: 'var(--cos-color-accent)',
                        borderRadius: 'var(--cos-radius-sm)',
                      }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </Chart>
        ) : null}
        <Table<QueryRow>
          getRowKey={(row, index) => `${dimensionLabel(row.dimensions)}-${index}`}
          rows={rows}
          columns={[
            {
              key: 'dimensions',
              header: 'Dimensions',
              cell: (row) => dimensionLabel(row.dimensions),
            },
            {
              key: 'value',
              header: 'Value',
              align: 'right',
              cell: (row) => formatMetricValue(row.value, metric?.unit ?? 'count'),
            },
            {
              key: 'drill',
              header: 'Drill through',
              cell: (row) =>
                row.drill_links.length === 0 ? (
                  '—'
                ) : (
                  <span style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
                    {row.drill_links.slice(0, 3).map((href, index) => (
                      <Link key={href} href={href}>
                        {row.record_ids[index] ?? `Record ${index + 1}`}
                      </Link>
                    ))}
                  </span>
                ),
            },
          ]}
          empty={
            <EmptyState
              title={busy ? 'Running report…' : 'No report results'}
              description={
                result
                  ? 'No rows are visible for this metric and permission set.'
                  : 'Run the report to load event-derived facts.'
              }
            />
          }
        />
      </section>
    </main>
  );
}
