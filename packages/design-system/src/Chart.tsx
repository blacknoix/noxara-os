import type { ReactNode } from 'react';

export type ChartProps = {
  title?: string;
  description?: string;
  height?: number | string;
  empty?: boolean;
  emptyMessage?: string;
  children?: ReactNode;
};

/** Empty chart slot wrapper — no chart library; pass children as the plot. */
export function Chart({
  title,
  description,
  height = 240,
  empty,
  emptyMessage = 'No chart data',
  children,
}: ChartProps) {
  return (
    <figure
      style={{
        margin: 0,
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      {title || description ? (
        <figcaption style={{ marginBottom: 'var(--cos-space-2)' }}>
          {title ? (
            <div style={{ fontWeight: 600, color: 'var(--cos-color-fg)' }}>{title}</div>
          ) : null}
          {description ? (
            <div style={{ fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)' }}>{description}</div>
          ) : null}
        </figcaption>
      ) : null}
      <div
        style={{
          height,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          border: '1px dashed var(--cos-color-border)',
          borderRadius: 'var(--cos-radius-sm)',
          color: 'var(--cos-color-fg-muted)',
          background: 'var(--cos-color-bg)',
        }}
      >
        {empty || !children ? emptyMessage : children}
      </div>
    </figure>
  );
}
