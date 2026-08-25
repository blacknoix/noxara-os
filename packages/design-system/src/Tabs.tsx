'use client';

import type { ReactNode } from 'react';

export type TabItem = {
  id: string;
  label: ReactNode;
  disabled?: boolean;
};

export type TabsProps = {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  children?: ReactNode;
};

export function Tabs({ items, value, onChange, children }: TabsProps) {
  return (
    <div style={{ fontFamily: 'var(--cos-font-sans)' }}>
      <div role="tablist" aria-orientation="horizontal" style={{ display: 'flex', gap: 0, borderBottom: '1px solid var(--cos-color-border)' }}>
        {items.map((tab) => {
          const selected = tab.id === value;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              id={`tab-${tab.id}`}
              aria-selected={selected}
              aria-controls={`panel-${tab.id}`}
              disabled={tab.disabled}
              tabIndex={selected ? 0 : -1}
              onClick={() => onChange(tab.id)}
              onKeyDown={(e) => {
                const enabled = items.filter((t) => !t.disabled);
                const idx = enabled.findIndex((t) => t.id === value);
                if (e.key === 'ArrowRight') {
                  e.preventDefault();
                  const next = enabled[(idx + 1) % enabled.length];
                  if (next) onChange(next.id);
                } else if (e.key === 'ArrowLeft') {
                  e.preventDefault();
                  const next = enabled[(idx - 1 + enabled.length) % enabled.length];
                  if (next) onChange(next.id);
                }
              }}
              style={{
                fontFamily: 'inherit',
                fontWeight: selected ? 650 : 500,
                fontSize: '0.875rem',
                padding: '0.65rem 0.9rem',
                border: 'none',
                borderBottom: selected ? '2px solid var(--cos-color-accent)' : '2px solid transparent',
                background: 'transparent',
                color: selected ? 'var(--cos-color-fg)' : 'var(--cos-color-fg-muted)',
                cursor: tab.disabled ? 'not-allowed' : 'pointer',
                opacity: tab.disabled ? 0.5 : 1,
                marginBottom: -1,
              }}
            >
              {tab.label}
            </button>
          );
        })}
      </div>
      {children ? (
        <div role="tabpanel" id={`panel-${value}`} aria-labelledby={`tab-${value}`} style={{ paddingTop: 'var(--cos-space-4)' }}>
          {children}
        </div>
      ) : null}
    </div>
  );
}
