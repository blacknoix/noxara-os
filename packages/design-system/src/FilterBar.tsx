'use client';

import type { ReactNode } from 'react';
import { Button } from './Button';
import { Input } from './Input';

export type FilterOperator = 'is' | 'is_not' | 'contains' | 'gt' | 'lt' | 'between' | 'empty';

export type FilterClause = {
  id: string;
  field: string;
  operator: FilterOperator;
  value: string | [string, string] | null;
  label?: string;
};

export type FilterBarProps = {
  filters: FilterClause[];
  onFiltersChange: (filters: FilterClause[]) => void;
  q?: string;
  onQueryChange?: (q: string) => void;
  searchPlaceholder?: string;
  onSaveView?: () => void;
  onUpdateView?: () => void;
  onClearAll?: () => void;
  /** Available fields for chip grammar display / add */
  fields?: { key: string; label: string }[];
  onAddFilter?: () => void;
  children?: ReactNode;
};

const OPERATOR_LABEL: Record<FilterOperator, string> = {
  is: 'is',
  is_not: 'is not',
  contains: 'contains',
  gt: '>',
  lt: '<',
  between: 'between',
  empty: 'is empty',
};

function formatValue(value: FilterClause['value']): string {
  if (value == null) return '';
  if (Array.isArray(value)) return `${value[0]} – ${value[1]}`;
  return String(value);
}

export function FilterBar({
  filters,
  onFiltersChange,
  q = '',
  onQueryChange,
  searchPlaceholder = 'Search…',
  onSaveView,
  onUpdateView,
  onClearAll,
  onAddFilter,
  children,
}: FilterBarProps) {
  const removeFilter = (id: string) => {
    onFiltersChange(filters.filter((f) => f.id !== id));
  };

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 'var(--cos-space-2)',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 'var(--cos-space-2)', alignItems: 'center' }}>
        {onQueryChange ? (
          <div style={{ flex: '1 1 200px', minWidth: 160, maxWidth: 320 }}>
            <Input
              aria-label="Search"
              placeholder={searchPlaceholder}
              value={q}
              onChange={(e) => onQueryChange(e.target.value)}
            />
          </div>
        ) : null}

        <div
          role="list"
          aria-label="Active filters"
          style={{ display: 'flex', flexWrap: 'wrap', gap: 'var(--cos-space-2)', flex: '1 1 auto' }}
        >
          {filters.map((f) => (
            <span
              key={f.id}
              role="listitem"
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                padding: '0.3rem 0.55rem',
                borderRadius: 'var(--cos-radius-sm)',
                background: 'var(--cos-color-bg-muted)',
                border: '1px solid var(--cos-color-border)',
                fontSize: '0.8125rem',
                color: 'var(--cos-color-fg)',
              }}
            >
              <span>
                <strong>{f.label ?? f.field}</strong> {OPERATOR_LABEL[f.operator]}
                {f.operator !== 'empty' ? ` ${formatValue(f.value)}` : ''}
              </span>
              <button
                type="button"
                aria-label={`Remove filter ${f.label ?? f.field}`}
                onClick={() => removeFilter(f.id)}
                style={{
                  all: 'unset',
                  cursor: 'pointer',
                  fontWeight: 700,
                  lineHeight: 1,
                  color: 'var(--cos-color-fg-muted)',
                }}
              >
                ×
              </button>
            </span>
          ))}
        </div>

        <div style={{ display: 'flex', gap: 'var(--cos-space-2)', flexWrap: 'wrap' }}>
          {onAddFilter ? (
            <Button variant="secondary" size="sm" onClick={onAddFilter}>
              Add filter
            </Button>
          ) : null}
          {onSaveView ? (
            <Button variant="secondary" size="sm" onClick={onSaveView}>
              Save view
            </Button>
          ) : null}
          {onUpdateView ? (
            <Button variant="ghost" size="sm" onClick={onUpdateView}>
              Update view
            </Button>
          ) : null}
          {(filters.length > 0 || q) && onClearAll ? (
            <Button variant="ghost" size="sm" onClick={onClearAll}>
              Clear all
            </Button>
          ) : null}
        </div>
      </div>
      {children}
    </div>
  );
}
