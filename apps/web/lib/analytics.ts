export type MetricUnit = 'money_minor' | 'count' | 'tokens';

export type MetricDefinition = {
  name: string;
  display_name: string;
  description: string;
  fact: string;
  measure: 'sum' | 'count' | 'avg';
  measure_field: string;
  dimensions: string[];
  unit: MetricUnit;
  required_permission: string;
  drill_route: string;
  flagship: boolean;
};

export type ReportDefinition = {
  org_id: string;
  metric: string;
  dimensions: string[];
  filters: Array<{ field: string; op: string; value: unknown }>;
  group_by: string[];
  visualization: 'table' | 'bar' | string;
};

export type Report = {
  id: string;
  org_id: string;
  name: string;
  description: string;
  definition: ReportDefinition;
  visualization: string;
  created_at: string;
  updated_at: string;
};

export type QueryRow = {
  dimensions: Record<string, string>;
  value: number;
  record_ids: string[];
  drill_links: string[];
};

export type QueryResult = {
  metric: string;
  rows: QueryRow[];
  filtered_by_permission: boolean;
  permission_denied_empty: boolean;
  dry_run: boolean;
  elapsed_ms: number;
  freshness_as_of?: string | null;
  eventually_consistent: boolean;
};

export type RunReportResponse = {
  run_id: string;
  report_id?: string | null;
  result: QueryResult;
};

export type DashboardWidget = {
  id: string;
  dashboard_id: string;
  title: string;
  metric_name: string;
  visualization: string;
  config: Record<string, unknown>;
  position: number;
  created_at: string;
};

export type Dashboard = {
  id: string;
  org_id: string;
  name: string;
  description: string;
  layout: unknown;
  widgets: DashboardWidget[];
  created_at: string;
  updated_at: string;
};

export type ForecastPoint = {
  period_index: number;
  period_label: string;
  value: number;
};

export type ForecastResponse = {
  series: string;
  metric: string;
  method: 'trailing_average' | 'linear_trend';
  inputs: {
    history_values: number[];
    history_periods: number;
    horizon_periods: number;
    method_params: Record<string, unknown>;
  };
  history: ForecastPoint[];
  forecast: ForecastPoint[];
  unit: MetricUnit;
  explainability: string;
};

export const analyticsPageStyle = {
  padding: '1.5rem',
  display: 'flex',
  flexDirection: 'column',
  gap: '1.25rem',
  maxWidth: '64rem',
} as const;

export function formatMetricValue(value: number, unit: MetricUnit): string {
  if (unit === 'money_minor') {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency: 'USD',
    }).format(value / 100);
  }
  return new Intl.NumberFormat().format(value);
}

export function dimensionLabel(dimensions: Record<string, string>): string {
  const entries = Object.entries(dimensions);
  return entries.length === 0
    ? 'Total'
    : entries.map(([key, value]) => `${key}: ${value}`).join(' · ');
}

export async function responseError(response: Response, fallback: string): Promise<string> {
  const body = (await response.json().catch(() => null)) as {
    detail?: string;
    message?: string;
  } | null;
  return body?.detail ?? body?.message ?? fallback;
}
