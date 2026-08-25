import type { ReactNode } from 'react';

export type BannerProps = {
  children: ReactNode;
  tone?: 'info' | 'warning' | 'danger' | 'accent';
  action?: ReactNode;
  onDismiss?: () => void;
};

const bg: Record<NonNullable<BannerProps['tone']>, string> = {
  info: 'var(--cos-color-info-muted)',
  warning: 'var(--cos-color-warning-muted)',
  danger: 'var(--cos-color-danger-muted)',
  accent: 'var(--cos-color-accent-muted)',
};

export function Banner({ children, tone = 'accent', action, onDismiss }: BannerProps) {
  return (
    <div
      role="region"
      aria-label="Announcement"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 'var(--cos-space-3)',
        padding: 'var(--cos-space-2) var(--cos-space-4)',
        background: bg[tone],
        borderBottom: '1px solid var(--cos-color-border)',
        fontFamily: 'var(--cos-font-sans)',
        fontSize: '0.875rem',
        color: 'var(--cos-color-fg)',
      }}
    >
      <div style={{ flex: 1 }}>{children}</div>
      {action}
      {onDismiss ? (
        <button
          type="button"
          aria-label="Dismiss banner"
          onClick={onDismiss}
          style={{ all: 'unset', cursor: 'pointer', fontWeight: 700, color: 'var(--cos-color-fg-muted)' }}
        >
          ×
        </button>
      ) : null}
    </div>
  );
}
