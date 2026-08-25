import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';

export type BadgeTone = 'neutral' | 'success' | 'warning' | 'danger' | 'info' | 'accent';

export type BadgeProps = HTMLAttributes<HTMLSpanElement> & {
  tone?: BadgeTone;
  children: ReactNode;
};

const toneStyles: Record<BadgeTone, CSSProperties> = {
  neutral: { background: 'var(--cos-color-bg-muted)', color: 'var(--cos-color-fg)' },
  success: { background: 'var(--cos-color-success-muted)', color: 'var(--cos-color-success)' },
  warning: { background: 'var(--cos-color-warning-muted)', color: 'var(--cos-color-warning)' },
  danger: { background: 'var(--cos-color-danger-muted)', color: 'var(--cos-color-danger)' },
  info: { background: 'var(--cos-color-info-muted)', color: 'var(--cos-color-info)' },
  accent: { background: 'var(--cos-color-accent-muted)', color: 'var(--cos-color-accent)' },
};

export function Badge({ tone = 'neutral', children, style, ...rest }: BadgeProps) {
  return (
    <span
      {...rest}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        fontFamily: 'var(--cos-font-sans)',
        fontSize: '0.75rem',
        fontWeight: 600,
        padding: '0.15rem 0.45rem',
        borderRadius: 'var(--cos-radius-sm)',
        lineHeight: 1.3,
        ...toneStyles[tone],
        ...style,
      }}
    >
      {children}
    </span>
  );
}
