export type PermissionDeniedStateProps = {
  title?: string;
  message?: string;
  requiredPermission: string;
};

export function PermissionDeniedState({
  title = 'Permission denied',
  message = 'You do not have access to this resource.',
  requiredPermission,
}: PermissionDeniedStateProps) {
  return (
    <div
      role="alert"
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
      <p
        style={{
          margin: '0.75rem auto 0',
          maxWidth: 480,
          fontSize: '0.8125rem',
          color: 'var(--cos-color-fg)',
        }}
      >
        Required permission:{' '}
        <code
          style={{
            fontFamily: 'ui-monospace, monospace',
            background: 'var(--cos-color-bg-muted)',
            padding: '0.15rem 0.4rem',
            borderRadius: 'var(--cos-radius-sm)',
            border: '1px solid var(--cos-color-border)',
          }}
        >
          {requiredPermission}
        </code>
      </p>
    </div>
  );
}
