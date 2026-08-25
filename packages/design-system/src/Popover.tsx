'use client';

import { useEffect, useId, useRef, useState, type ReactNode } from 'react';

export type PopoverProps = {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  label?: string;
};

export function Popover({ trigger, children, open: controlledOpen, onOpenChange, label = 'Menu' }: PopoverProps) {
  const [uncontrolled, setUncontrolled] = useState(false);
  const open = controlledOpen ?? uncontrolled;
  const setOpen = (v: boolean) => {
    onOpenChange?.(v);
    if (controlledOpen === undefined) setUncontrolled(v);
  };
  const id = useId();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={ref} style={{ position: 'relative', display: 'inline-flex' }}>
      <div
        onClick={() => setOpen(!open)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            setOpen(!open);
          }
        }}
        aria-expanded={open}
        aria-controls={id}
        aria-haspopup="dialog"
      >
        {trigger}
      </div>
      {open ? (
        <div
          id={id}
          role="dialog"
          aria-label={label}
          style={{
            position: 'absolute',
            top: 'calc(100% + 6px)',
            right: 0,
            zIndex: 'var(--cos-z-dropdown)',
            minWidth: 180,
            background: 'var(--cos-color-bg-elevated)',
            border: '1px solid var(--cos-color-border)',
            borderRadius: 'var(--cos-radius-md)',
            boxShadow: 'var(--cos-shadow-md)',
            padding: 'var(--cos-space-2)',
            fontFamily: 'var(--cos-font-sans)',
          }}
        >
          {children}
        </div>
      ) : null}
    </div>
  );
}
