import type { ReactNode } from 'react';
import { EmptyState } from './EmptyState';
import { ErrorState } from './ErrorState';
import { Skeleton } from './Skeleton';

export type WidgetProps = {
  title: string;
  range?: ReactNode;
  menu?: ReactNode;
  body?: ReactNode;
  footer?: ReactNode;
  loading?: boolean;
  empty?: boolean | ReactNode;
  error?: string | ReactNode;
  children?: ReactNode;
};

export function Widget({ title, range, menu, body, footer, loading, empty, error, children }: WidgetProps) {
  let content: ReactNode = body ?? children;
  if (loading) {
    content = (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: 'var(--cos-space-2) 0' }}>
        <Skeleton height={18} />
        <Skeleton height={18} width="80%" />
        <Skeleton height={18} width="60%" />
      </div>
    );
  } else if (error) {
    content =
      typeof error === 'string' ? <ErrorState message={error} title="Could not load" /> : error;
  } else if (empty) {
    content =
      empty === true ? <EmptyState title="No data" description="Nothing to show yet." /> : empty;
  }

  return (
    <section
      style={{
        fontFamily: 'var(--cos-font-sans)',
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
      }}
    >
      <header
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 'var(--cos-space-3)',
          marginBottom: 'var(--cos-space-3)',
        }}
      >
        <div>
          <h3
            style={{
              margin: 0,
              fontFamily: 'var(--cos-font-display)',
              fontSize: '1.1rem',
              fontWeight: 550,
              color: 'var(--cos-color-fg)',
            }}
          >
            {title}
          </h3>
          {range ? (
            <div style={{ marginTop: 2, fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>{range}</div>
          ) : null}
        </div>
        {menu}
      </header>
      <div style={{ flex: 1, minHeight: 0 }}>{content}</div>
      {footer ? (
        <footer
          style={{
            marginTop: 'var(--cos-space-3)',
            paddingTop: 'var(--cos-space-2)',
            borderTop: '1px solid var(--cos-color-border)',
            fontSize: '0.8125rem',
            color: 'var(--cos-color-fg-muted)',
          }}
        >
          {footer}
        </footer>
      ) : null}
    </section>
  );
}
