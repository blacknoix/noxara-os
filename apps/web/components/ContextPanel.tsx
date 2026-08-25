'use client';

export function ContextPanel({ open }: { open: boolean }) {
  return (
    <aside
      aria-hidden={!open}
      style={{
        borderLeft: open ? '1px solid var(--cos-color-border)' : 'none',
        background: 'var(--cos-color-bg-elevated)',
        padding: open ? '1rem' : 0,
        overflow: 'hidden',
        opacity: open ? 1 : 0,
        transition: 'opacity 200ms ease',
      }}
    >
      {open ? (
        <>
          <h2
            style={{
              margin: '0 0 0.5rem',
              fontFamily: 'var(--cos-font-display)',
              fontSize: '1.05rem',
              fontWeight: 550,
            }}
          >
            Context
          </h2>
          <p style={{ margin: 0, color: 'var(--cos-color-fg-muted)', fontSize: '0.88rem', lineHeight: 1.5 }}>
            Collapsible context panel placeholder. Detail views and AI citations will land here in later
            phases.
          </p>
        </>
      ) : null}
    </aside>
  );
}
