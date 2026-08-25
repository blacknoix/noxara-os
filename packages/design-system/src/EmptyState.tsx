import type { ReactNode } from 'react';

export type EmptyStateProps = {
  title: string;
  description?: string;
  action?: ReactNode;
};

export function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div
      style={{
        padding: 'var(--cos-space-8) var(--cos-space-4)',
        textAlign: 'center',
        color: 'var(--cos-color-fg-muted)',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      <h2
        style={{
          margin: 0,
          fontFamily: 'var(--cos-font-display)',
          fontSize: '1.35rem',
          color: 'var(--cos-color-fg)',
          fontWeight: 550,
        }}
      >
        {title}
      </h2>
      {description ? (
        <p style={{ margin: '0.5rem auto 0', maxWidth: 420, lineHeight: 1.5 }}>{description}</p>
      ) : null}
      {action ? <div style={{ marginTop: 'var(--cos-space-4)' }}>{action}</div> : null}
    </div>
  );
}
