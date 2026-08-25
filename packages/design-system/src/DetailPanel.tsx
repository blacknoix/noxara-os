'use client';

import { useEffect, useRef, type ReactNode } from 'react';

export type DetailPanelProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number | string;
};

/** Slide-over detail panel from the right (context panel). */
export function DetailPanel({
  open,
  onClose,
  title,
  children,
  footer,
  width = 'var(--cos-context-panel-width)',
}: DetailPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    panelRef.current?.focus();
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <>
      <div
        aria-hidden="true"
        onClick={onClose}
        style={{
          position: 'fixed',
          inset: 0,
          background: 'var(--cos-color-overlay)',
          zIndex: 'var(--cos-z-overlay)',
        }}
      />
      <aside
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{
          position: 'fixed',
          top: 0,
          right: 0,
          bottom: 0,
          width,
          maxWidth: '100%',
          zIndex: 'var(--cos-z-modal)',
          background: 'var(--cos-color-bg-elevated)',
          borderLeft: '1px solid var(--cos-color-border)',
          boxShadow: 'var(--cos-shadow-md)',
          display: 'flex',
          flexDirection: 'column',
          fontFamily: 'var(--cos-font-sans)',
          outline: 'none',
        }}
      >
        <header
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: 'var(--cos-space-4)',
            borderBottom: '1px solid var(--cos-color-border)',
            minHeight: 'var(--cos-topbar-height)',
          }}
        >
          <h2
            style={{
              margin: 0,
              fontFamily: 'var(--cos-font-display)',
              fontSize: '1.15rem',
              fontWeight: 550,
            }}
          >
            {title}
          </h2>
          <button
            type="button"
            aria-label="Close panel"
            onClick={onClose}
            style={{
              all: 'unset',
              cursor: 'pointer',
              fontSize: '1.25rem',
              lineHeight: 1,
              color: 'var(--cos-color-fg-muted)',
            }}
          >
            ×
          </button>
        </header>
        <div style={{ flex: 1, overflow: 'auto', padding: 'var(--cos-space-4)' }}>{children}</div>
        {footer ? (
          <footer
            style={{
              padding: 'var(--cos-space-4)',
              borderTop: '1px solid var(--cos-color-border)',
            }}
          >
            {footer}
          </footer>
        ) : null}
      </aside>
    </>
  );
}
