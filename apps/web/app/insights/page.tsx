'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  Badge,
  Card,
  ErrorState,
  InlineAlert,
  PermissionDeniedState,
  StaleDataState,
  StatTile,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';
import {
  analyticsPageStyle,
  formatMetricValue,
  responseError,
  type MetricUnit,
} from '../../lib/analytics';

type Benchmark = {
  metric: string;
  display_name: string;
  unit: MetricUnit;
  current_value: number;
  previous_value: number;
  trend_percent?: number | null;
};

type Freshness = {
  last_event_at?: string | null;
  last_ingest_at?: string | null;
  lag_seconds: number;
  eventually_consistent: boolean;
};

export default function InsightsPage() {
  const { caps, can, loading: capsLoading } = useCapabilities();
  const canReadReports = can('analytics.report.read');
  const canReadDashboards = can('analytics.dashboard.read');
  const [benchmarks, setBenchmarks] = useState<Benchmark[]>([]);
  const [freshness, setFreshness] = useState<Freshness | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken() || !caps?.org_id || !canReadReports) return;
    setBusy(true);
    setError(null);
    const org = encodeURIComponent(caps.org_id);
    try {
      const [benchmarkResponse, freshnessResponse] = await Promise.all([
        authFetch(`/api/v1/analytics/benchmarks?org_id=${org}`),
        authFetch(`/api/v1/analytics/freshness?org_id=${org}`),
      ]);
      if (!benchmarkResponse.ok) {
        setError(await responseError(benchmarkResponse, 'Could not load analytics benchmarks.'));
        return;
      }
      if (!freshnessResponse.ok) {
        setError(await responseError(freshnessResponse, 'Could not load analytics freshness.'));
        return;
      }
      const benchmarkBody = (await benchmarkResponse.json()) as { benchmarks?: Benchmark[] };
      setBenchmarks(benchmarkBody.benchmarks ?? []);
      setFreshness((await freshnessResponse.json()) as Freshness);
    } catch {
      setError('Analytics request failed.');
    } finally {
      setBusy(false);
    }
  }, [caps?.org_id, canReadReports]);

  useEffect(() => {
    if (!capsLoading) void load();
  }, [capsLoading, load]);

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading analytics…</p>
      </main>
    );
  }
  if (!canReadReports && !canReadDashboards) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState
          title="Benchmarks & trends"
          requiredPermission="analytics.report.read or analytics.dashboard.read"
        />
      </main>
    );
  }

  const isStale = Boolean(freshness?.last_ingest_at && freshness.lag_seconds > 300);

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <header
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          gap: '1rem',
          flexWrap: 'wrap',
        }}
      >
        <div>
          <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>
            Benchmarks & trends
          </h1>
          <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
            Flagship company metrics derived from the governed event stream.
          </p>
        </div>
        <Badge tone={isStale ? 'warning' : 'info'}>
          {freshness?.last_ingest_at
            ? `Freshness: ${freshness.lag_seconds}s lag`
            : 'Awaiting first analytics event'}
        </Badge>
      </header>

      {!canReadReports ? (
        <InlineAlert tone="info" title="Benchmark access is limited">
          Dashboard access is enabled, but analytics.report.read is required to load governed
          benchmark values.
        </InlineAlert>
      ) : null}
      {error ? <ErrorState message={error} /> : null}
      {freshness?.eventually_consistent ? (
        <InlineAlert tone="info" title="Eventually consistent">
          Analytics facts are built from domain events. Recent operational changes may take a
          short time to appear.
        </InlineAlert>
      ) : null}
      {isStale && freshness?.last_ingest_at ? (
        <Card>
          <StaleDataState
            asOf={freshness.last_ingest_at}
            onRefresh={() => void load()}
            refreshing={busy}
            message="The analytics consumer is more than five minutes behind the latest event."
          />
        </Card>
      ) : null}

      <section aria-labelledby="flagship-metrics-heading">
        <h2 id="flagship-metrics-heading" style={{ fontSize: '1rem' }}>
          Flagship metrics
        </h2>
        {busy && benchmarks.length === 0 ? <p>Loading benchmarks…</p> : null}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(13rem, 1fr))',
            gap: '0.75rem',
          }}
        >
          {benchmarks.map((benchmark) => (
            <Card key={benchmark.metric}>
              <StatTile
                label={benchmark.display_name}
                value={formatMetricValue(benchmark.current_value, benchmark.unit)}
                hint="Current 7-day window"
                trend={
                  benchmark.trend_percent == null
                    ? 'No prior baseline'
                    : `${benchmark.trend_percent >= 0 ? '↑' : '↓'} ${Math.abs(
                        benchmark.trend_percent,
                      ).toFixed(1)}%`
                }
              />
            </Card>
          ))}
        </div>
      </section>
    </main>
  );
}
