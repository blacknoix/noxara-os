import type { ReactNode } from 'react';

export type BreadcrumbItem = {
  id: string;
  label: ReactNode;
  href?: string;
  onClick?: () => void;
};

export type BreadcrumbProps = {
  items: BreadcrumbItem[];
};

export function Breadcrumb({ items }: BreadcrumbProps) {
  return (
    <nav aria-label="Breadcrumb" style={{ fontFamily: 'var(--cos-font-sans)', fontSize: '0.8125rem' }}>
      <ol style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
        {items.map((item, i) => {
          const last = i === items.length - 1;
          return (
            <li key={item.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              {i > 0 ? (
                <span aria-hidden="true" style={{ color: 'var(--cos-color-fg-muted)' }}>
                  /
                </span>
              ) : null}
              {last ? (
                <span aria-current="page" style={{ color: 'var(--cos-color-fg)', fontWeight: 600 }}>
                  {item.label}
                </span>
              ) : item.href ? (
                <a href={item.href} style={{ color: 'var(--cos-color-fg-muted)', textDecoration: 'none' }}>
                  {item.label}
                </a>
              ) : (
                <button
                  type="button"
                  onClick={item.onClick}
                  style={{
                    all: 'unset',
                    cursor: 'pointer',
                    color: 'var(--cos-color-fg-muted)',
                  }}
                >
                  {item.label}
                </button>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
