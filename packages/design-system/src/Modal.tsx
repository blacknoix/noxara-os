'use client';

import { useEffect, useId, useRef, type ReactNode } from 'react';

export type ModalProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  /** Do not nest more than one Modal. Nesting breaks focus trapping and Escape handling. */
};

/**
 * Modal with focus trap via useEffect; Escape closes.
 * IMPORTANT: Do not nest more than one Modal at a time — nested modals are unsupported.
 */
export function Modal({ open, onClose, title, children, footer }: ModalProps) {
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    const panel = panelRef.current;
    if (!panel) return;

    const focusables = () =>
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((el) => !el.hasAttribute('disabled') && el.tabIndex !== -1);

    const first = focusables()[0];
    (first ?? panel).focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== 'Tab') return;
      const nodes = focusables();
      if (nodes.length === 0) {
        e.preventDefault();
        return;
      }
      const firstEl = nodes[0]!;
      const lastEl = nodes[nodes.length - 1]!;
      if (e.shiftKey && document.activeElement === firstEl) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && document.activeElement === lastEl) {
        e.preventDefault();
        firstEl.focus();
      }
    };

    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      previousFocus.current?.focus?.();
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      role="presentation"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 'var(--cos-z-modal)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 'var(--cos-space-4)',
      }}
    >
      <div
        aria-hidden="true"
        onClick={onClose}
        style={{
          position: 'absolute',
          inset: 0,
          background: 'var(--cos-color-overlay)',
        }}
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        style={{
          position: 'relative',
          width: '100%',
          maxWidth: 480,
          background: 'var(--cos-color-bg-elevated)',
          border: '1px solid var(--cos-color-border)',
          borderRadius: 'var(--cos-radius-md)',
          boxShadow: 'var(--cos-shadow-md)',
          fontFamily: 'var(--cos-font-sans)',
          outline: 'none',
        }}
      >
        <header
          style={{
            padding: 'var(--cos-space-4)',
            borderBottom: '1px solid var(--cos-color-border)',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          <h2
            id={titleId}
            style={{
              margin: 0,
              fontFamily: 'var(--cos-font-display)',
              fontSize: '1.2rem',
              fontWeight: 550,
            }}
          >
            {title}
          </h2>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            style={{
              all: 'unset',
              cursor: 'pointer',
              fontSize: '1.25rem',
              color: 'var(--cos-color-fg-muted)',
            }}
          >
            ×
          </button>
        </header>
        <div style={{ padding: 'var(--cos-space-4)' }}>{children}</div>
        {footer ? (
          <footer
            style={{
              padding: 'var(--cos-space-4)',
              borderTop: '1px solid var(--cos-color-border)',
              display: 'flex',
              justifyContent: 'flex-end',
              gap: 'var(--cos-space-2)',
            }}
          >
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}
