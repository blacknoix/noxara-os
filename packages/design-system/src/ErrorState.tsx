export type ErrorStateProps = {
  title?: string;
  message: string;
  requestId?: string;
};

export function ErrorState({ title = 'Something went wrong', message, requestId }: ErrorStateProps) {
  return (
    <div
      role="alert"
      style={{
        padding: 'var(--cos-space-8) var(--cos-space-4)',
        textAlign: 'center',
        fontFamily: 'var(--cos-font-sans)',
        color: 'var(--cos-color-danger)',
      }}
    >
      <h2
        style={{
          margin: 0,
          fontFamily: 'var(--cos-font-display)',
          fontSize: '1.35rem',
          fontWeight: 550,
        }}
      >
        {title}
      </h2>
      <p style={{ margin: '0.5rem auto 0', maxWidth: 480, lineHeight: 1.5, color: 'var(--cos-color-fg-muted)' }}>
        {message}
      </p>
      {requestId ? (
        <p
          style={{
            margin: '0.75rem auto 0',
            maxWidth: 480,
            fontSize: '0.75rem',
            color: 'var(--cos-color-fg-muted)',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          Request ID: <code style={{ fontFamily: 'ui-monospace, monospace' }}>{requestId}</code>
        </p>
      ) : null}
    </div>
  );
}
