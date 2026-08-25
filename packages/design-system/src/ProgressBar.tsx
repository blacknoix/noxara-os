import type { HTMLAttributes } from 'react';

export type ProgressBarProps = HTMLAttributes<HTMLDivElement> & {
  value: number;
  max?: number;
  label?: string;
};

export function ProgressBar({ value, max = 100, label, style, ...rest }: ProgressBarProps) {
  const pct = Math.max(0, Math.min(100, (value / max) * 100));
  return (
    <div {...rest} style={{ fontFamily: 'var(--cos-font-sans)', width: '100%', ...style }}>
      {label ? (
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            fontSize: '0.75rem',
            color: 'var(--cos-color-fg-muted)',
            marginBottom: 4,
          }}
        >
          <span>{label}</span>
          <span>{Math.round(pct)}%</span>
        </div>
      ) : null}
      <div
        role="progressbar"
        aria-valuenow={value}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-label={label}
        style={{
          height: 8,
          borderRadius: 999,
          background: 'var(--cos-color-bg-muted)',
          overflow: 'hidden',
          border: '1px solid var(--cos-color-border)',
        }}
      >
        <div
          style={{
            width: `${pct}%`,
            height: '100%',
            background: 'var(--cos-color-accent)',
            transition: `width var(--cos-duration-normal) var(--cos-ease-standard)`,
          }}
        />
      </div>
    </div>
  );
}
