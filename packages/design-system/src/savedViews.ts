import type { FilterClause } from './FilterBar';
import type { SortDir, TableDensity } from './Table';

export type SavedViewState = {
  filters?: FilterClause[];
  sort?: { key: string; dir: SortDir };
  columns?: string[];
  hiddenColumns?: string[];
  density?: TableDensity;
  q?: string;
};

const VIEW_KEY = 'view';
const Q_KEY = 'q';
const F_KEY = 'f';

function safeParseJson<T>(raw: string | null): T | null {
  if (!raw) return null;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

/** Parse saved view state from URL search params (`view`, `q`, `f`). */
export function parseViewFromSearchParams(params: URLSearchParams): SavedViewState {
  const viewBlob = safeParseJson<SavedViewState>(params.get(VIEW_KEY));
  const filters = safeParseJson<FilterClause[]>(params.get(F_KEY)) ?? viewBlob?.filters;
  const q = params.get(Q_KEY) ?? viewBlob?.q ?? undefined;

  return {
    filters: filters ?? undefined,
    sort: viewBlob?.sort,
    columns: viewBlob?.columns,
    hiddenColumns: viewBlob?.hiddenColumns,
    density: viewBlob?.density,
    q: q || undefined,
  };
}

/** Serialize view state into URLSearchParams under `view`, `q`, `f`. */
export function viewToSearchParams(
  state: SavedViewState,
  base?: URLSearchParams,
): URLSearchParams {
  const params = new URLSearchParams(base?.toString());

  if (state.q) {
    params.set(Q_KEY, state.q);
  } else {
    params.delete(Q_KEY);
  }

  if (state.filters && state.filters.length > 0) {
    params.set(F_KEY, JSON.stringify(state.filters));
  } else {
    params.delete(F_KEY);
  }

  const viewPayload: SavedViewState = {
    sort: state.sort,
    columns: state.columns,
    hiddenColumns: state.hiddenColumns,
    density: state.density,
  };
  const hasViewMeta =
    Boolean(viewPayload.sort) ||
    Boolean(viewPayload.columns?.length) ||
    Boolean(viewPayload.hiddenColumns?.length) ||
    Boolean(viewPayload.density);

  if (hasViewMeta) {
    params.set(VIEW_KEY, JSON.stringify(viewPayload));
  } else {
    params.delete(VIEW_KEY);
  }

  return params;
}
