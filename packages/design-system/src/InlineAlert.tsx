import type { ReactNode } from 'react';

export type InlineAlertTone = 'info' | 'success' | 'warning' | 'danger';

export type InlineAlertProps = {
  tone?: InlineAlertTone;
  title?: string;
  children: ReactNode;
  action?: ReactNode;
};

const tones: Record<InlineAlertTone, { bg: string; fg: string; border: string }> = {
  info: {
    bg: 'var(--cos-color-info-muted)',
    fg: 'var(--cos-color-info)',
    border: 'var(--cos-color-info)',
  },
  success: {
    bg: 'var(--cos-color-success-muted)',
    fg: 'var(--cos-color-success)',
    border: 'var(--cos-color-success)',
  },
  warning: {
    bg: 'var(--cos-color-warning-muted)',
    fg: 'var(--cos-color-warning)',
    border: 'var(--cos-color-warning)',
  },
  danger: {
    bg: 'var(--cos-color-danger-muted)',
    fg: 'var(--cos-color-danger)',
    border: 'var(--cos-color-danger)',
  },
};

export function InlineAlert({ tone = 'info', title, children, action }: InlineAlertProps) {
  const t = tones[tone];
  return (
    <div
      role="status"
      style={{
        display: 'flex',
        gap: 'var(--cos-space-3)',
        alignItems: 'flex-start',
        padding: 'var(--cos-space-3)',
        borderRadius: 'var(--cos-radius-sm)',
        background: t.bg,
        borderLeft: `3px solid ${t.border}`,
        fontFamily: 'var(--cos-font-sans)',
        color: 'var(--cos-color-fg)',
      }}
    >
      <div style={{ flex: 1 }}>
        {title ? <div style={{ fontWeight: 700, color: t.fg, marginBottom: 2 }}>{title}</div> : null}
        <div style={{ fontSize: '0.875rem' }}>{children}</div>
      </div>
      {action}
    </div>
  );
}
