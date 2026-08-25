export type ErrorStateProps = {
  title?: string;
  message: string;
};

export function ErrorState({ title = 'Something went wrong', message }: ErrorStateProps) {
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
    </div>
  );
}
