'use client';

import { Button } from './Button';

export type StaleDataStateProps = {
  title?: string;
  message?: string;
  asOf: string | Date;
  onRefresh: () => void;
  refreshing?: boolean;
};

export function StaleDataState({
  title = 'Data may be out of date',
  message = 'This view was last refreshed earlier. Refresh to load the latest data.',
  asOf,
  onRefresh,
  refreshing,
}: StaleDataStateProps) {
  const asOfLabel =
    asOf instanceof Date
      ? asOf.toLocaleString()
      : (() => {
          const d = new Date(asOf);
          return Number.isNaN(d.getTime()) ? String(asOf) : d.toLocaleString();
        })();

  return (
    <div
      role="status"
      style={{
        padding: 'var(--cos-space-8) var(--cos-space-4)',
        textAlign: 'center',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      <h2
        style={{
          margin: 0,
          fontFamily: 'var(--cos-font-display)',
          fontSize: '1.35rem',
          fontWeight: 550,
          color: 'var(--cos-color-fg)',
        }}
      >
        {title}
      </h2>
      <p style={{ margin: '0.5rem auto 0', maxWidth: 480, lineHeight: 1.5, color: 'var(--cos-color-fg-muted)' }}>
        {message}
      </p>
      <p style={{ margin: '0.5rem auto 0', fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)' }}>
        As of <time dateTime={asOf instanceof Date ? asOf.toISOString() : String(asOf)}>{asOfLabel}</time>
      </p>
      <div style={{ marginTop: 'var(--cos-space-4)' }}>
        <Button onClick={onRefresh} loading={refreshing}>
          Refresh
        </Button>
      </div>
    </div>
  );
}
