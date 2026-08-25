'use client';

import { useId, useState, type ReactNode } from 'react';

export type TooltipProps = {
  content: ReactNode;
  children: ReactNode;
  side?: 'top' | 'bottom';
};

export function Tooltip({ content, children, side = 'top' }: TooltipProps) {
  const id = useId();
  const [open, setOpen] = useState(false);

  return (
    <span
      style={{ position: 'relative', display: 'inline-flex' }}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      <span aria-describedby={open ? id : undefined}>{children}</span>
      {open ? (
        <span
          id={id}
          role="tooltip"
          style={{
            position: 'absolute',
            zIndex: 'var(--cos-z-dropdown)',
            left: '50%',
            transform: 'translateX(-50%)',
            bottom: side === 'top' ? 'calc(100% + 6px)' : undefined,
            top: side === 'bottom' ? 'calc(100% + 6px)' : undefined,
            background: 'var(--cos-color-fg)',
            color: 'var(--cos-color-bg)',
            fontFamily: 'var(--cos-font-sans)',
            fontSize: '0.75rem',
            padding: '0.35rem 0.55rem',
            borderRadius: 'var(--cos-radius-sm)',
            whiteSpace: 'nowrap',
            pointerEvents: 'none',
            maxWidth: 240,
          }}
        >
          {content}
        </span>
      ) : null}
    </span>
  );
}
