'use client';

import { useEffect, useRef, type ReactNode } from 'react';

export type DrawerProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  side?: 'left' | 'right';
  footer?: ReactNode;
  width?: number | string;
};

export function Drawer({
  open,
  onClose,
  title,
  children,
  side = 'right',
  footer,
  width = 360,
}: DrawerProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    ref.current?.focus();
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
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{
          position: 'fixed',
          top: 0,
          bottom: 0,
          left: side === 'left' ? 0 : undefined,
          right: side === 'right' ? 0 : undefined,
          width,
          maxWidth: '100%',
          zIndex: 'var(--cos-z-modal)',
          background: 'var(--cos-color-bg-elevated)',
          borderLeft: side === 'right' ? '1px solid var(--cos-color-border)' : undefined,
          borderRight: side === 'left' ? '1px solid var(--cos-color-border)' : undefined,
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
          }}
        >
          <h2 style={{ margin: 0, fontFamily: 'var(--cos-font-display)', fontSize: '1.15rem', fontWeight: 550 }}>
            {title}
          </h2>
          <button
            type="button"
            aria-label="Close drawer"
            onClick={onClose}
            style={{ all: 'unset', cursor: 'pointer', fontSize: '1.25rem', color: 'var(--cos-color-fg-muted)' }}
          >
            ×
          </button>
        </header>
        <div style={{ flex: 1, overflow: 'auto', padding: 'var(--cos-space-4)' }}>{children}</div>
        {footer ? (
          <footer style={{ padding: 'var(--cos-space-4)', borderTop: '1px solid var(--cos-color-border)' }}>
            {footer}
          </footer>
        ) : null}
      </aside>
    </>
  );
}
