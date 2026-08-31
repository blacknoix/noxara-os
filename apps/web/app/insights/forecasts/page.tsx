'use client';

import { useState } from 'react';
import {
  Button,
  Card,
  EmptyState,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Select,
  Table,
} from '@companyos/design-system';
import { authFetch } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  analyticsPageStyle,
  formatMetricValue,
  responseError,
  type ForecastPoint,
  type ForecastResponse,
} from '../../../lib/analytics';

export default function ForecastsPage() {
  const { caps, can, loading: capsLoading } = useCapabilities();
  const canRun = can('analytics.report.run');
  const [series, setSeries] = useState('revenue');
  const [method, setMethod] = useState<'trailing_average' | 'linear_trend'>(
    'trailing_average',
  );
  const [historyPeriods, setHistoryPeriods] = useState(6);
  const [horizonPeriods, setHorizonPeriods] = useState(3);
  const [forecast, setForecast] = useState<ForecastResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runForecast = async () => {
    if (!caps?.org_id) return;
    setBusy(true);
    setError(null);
    const response = await authFetch('/api/v1/analytics/forecasts', {
      method: 'POST',
      body: JSON.stringify({
        org_id: caps.org_id,
        series,
        history_periods: historyPeriods,
        horizon_periods: horizonPeriods,
        method,
      }),
    });
    if (!response.ok) {
      setError(
        await responseError(
          response,
          'Could not generate forecast. The selected series may require another module permission.',
        ),
      );
    } else {
      setForecast((await response.json()) as ForecastResponse);
    }
    setBusy(false);
  };

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <p>Loading forecasts…</p>
      </main>
    );
  }
  if (!canRun) {
    return (
      <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
        <PermissionDeniedState
          title="Forecasts"
          requiredPermission="analytics.report.run"
        />
      </main>
    );
  }

  return (
    <main id="main-content" tabIndex={-1} style={analyticsPageStyle}>
      <header>
        <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>Forecasts</h1>
        <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' }}>
          Explainable projections from governed analytics series.
        </p>
      </header>

      {error ? <ErrorState message={error} /> : null}

      <Card as="section" aria-labelledby="forecast-controls-heading">
        <h2 id="forecast-controls-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
          Forecast inputs
        </h2>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(12rem, 1fr))',
            gap: '0.75rem',
            alignItems: 'end',
          }}
        >
          <Select
            label="Series"
            value={series}
            onChange={(event) => setSeries(event.target.value)}
            options={[
              { value: 'revenue', label: 'Issued revenue' },
              { value: 'cash_flow', label: 'Cash collected' },
              { value: 'pipeline', label: 'Sales pipeline' },
              { value: 'expenses', label: 'Expenses' },
              { value: 'headcount', label: 'Headcount proxy' },
            ]}
          />
          <Select
            label="Method"
            value={method}
            onChange={(event) =>
              setMethod(event.target.value as 'trailing_average' | 'linear_trend')
            }
            options={[
              { value: 'trailing_average', label: 'Trailing average' },
              { value: 'linear_trend', label: 'Linear trend' },
            ]}
          />
          <Input
            label="History periods"
            type="number"
            min={1}
            max={365}
            value={historyPeriods}
            onChange={(event) => setHistoryPeriods(Number(event.target.value))}
          />
          <Input
            label="Forecast periods"
            type="number"
            min={1}
            max={52}
            value={horizonPeriods}
            onChange={(event) => setHorizonPeriods(Number(event.target.value))}
          />
        </div>
        <Button
          onClick={() => void runForecast()}
          loading={busy}
          disabled={
            historyPeriods < 1 ||
            historyPeriods > 365 ||
            horizonPeriods < 1 ||
            horizonPeriods > 52
          }
          style={{ marginTop: '1rem' }}
        >
          Generate forecast
        </Button>
      </Card>

      {forecast ? (
        <>
          <InlineAlert tone="info" title="Forecast method">
            {forecast.explainability}
          </InlineAlert>
          <Card as="section" aria-labelledby="forecast-method-heading">
            <h2 id="forecast-method-heading" style={{ marginTop: 0, fontSize: '1rem' }}>
              Method & inputs
            </h2>
            <dl
              style={{
                display: 'grid',
                gridTemplateColumns: 'max-content 1fr',
                gap: '0.45rem 1rem',
                margin: 0,
              }}
            >
              <dt>Method</dt>
              <dd style={{ margin: 0 }}>{forecast.method.replaceAll('_', ' ')}</dd>
              <dt>Metric</dt>
              <dd style={{ margin: 0 }}>{forecast.metric}</dd>
              <dt>History values</dt>
              <dd style={{ margin: 0 }}>
                {forecast.inputs.history_values.length
                  ? forecast.inputs.history_values.join(', ')
                  : 'No history facts available'}
              </dd>
              <dt>History periods</dt>
              <dd style={{ margin: 0 }}>{forecast.inputs.history_periods}</dd>
              <dt>Horizon periods</dt>
              <dd style={{ margin: 0 }}>{forecast.inputs.horizon_periods}</dd>
              <dt>Method parameters</dt>
              <dd style={{ margin: 0 }}>
                <code>{JSON.stringify(forecast.inputs.method_params)}</code>
              </dd>
            </dl>
          </Card>
        </>
      ) : null}

      <section aria-labelledby="forecast-results-heading">
        <h2 id="forecast-results-heading" style={{ fontSize: '1rem' }}>
          Forecast
        </h2>
        <Table<ForecastPoint>
          getRowKey={(point) => `${point.period_index}-${point.period_label}`}
          rows={forecast?.forecast ?? []}
          columns={[
            { key: 'period', header: 'Period', cell: (point) => point.period_label },
            {
              key: 'index',
              header: 'Step',
              cell: (point) => point.period_index,
            },
            {
              key: 'value',
              header: 'Forecast value',
              align: 'right',
              cell: (point) =>
                formatMetricValue(point.value, forecast?.unit ?? 'count'),
            },
          ]}
          empty={
            <EmptyState
              title={busy ? 'Generating forecast…' : 'No forecast yet'}
              description="Choose a series and method, then generate an explainable forecast."
            />
          }
        />
      </section>
    </main>
  );
}
