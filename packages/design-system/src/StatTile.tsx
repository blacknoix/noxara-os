import type { HTMLAttributes, ReactNode } from 'react';

export type StatTileProps = HTMLAttributes<HTMLDivElement> & {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  trend?: ReactNode;
};

export function StatTile({ label, value, hint, trend, style, ...rest }: StatTileProps) {
  return (
    <div
      {...rest}
      style={{
        fontFamily: 'var(--cos-font-sans)',
        padding: 'var(--cos-space-3) 0',
        ...style,
      }}
    >
      <div style={{ fontSize: '0.75rem', fontWeight: 600, color: 'var(--cos-color-fg-muted)', letterSpacing: '0.02em' }}>
        {label}
      </div>
      <div
        style={{
          marginTop: 4,
          fontFamily: 'var(--cos-font-display)',
          fontSize: '1.75rem',
          fontWeight: 550,
          color: 'var(--cos-color-fg)',
          lineHeight: 1.15,
        }}
      >
        {value}
      </div>
      {trend || hint ? (
        <div style={{ marginTop: 6, fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)' }}>
          {trend}
          {trend && hint ? ' · ' : null}
          {hint}
        </div>
      ) : null}
    </div>
  );
}
