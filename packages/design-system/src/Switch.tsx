'use client';

import type { ButtonHTMLAttributes, ReactNode } from 'react';

export type SwitchProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onChange'> & {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
};

export function Switch({ checked, onCheckedChange, label, description, disabled, id, style, ...rest }: SwitchProps) {
  const switchId = id ?? (typeof label === 'string' ? `switch-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 'var(--cos-space-3)',
        fontFamily: 'var(--cos-font-sans)',
        opacity: disabled ? 0.55 : 1,
        ...style,
      }}
    >
      <button
        {...rest}
        id={switchId}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onCheckedChange?.(!checked)}
        style={{
          width: 44,
          height: 26,
          borderRadius: 999,
          border: '1px solid var(--cos-color-border)',
          background: checked ? 'var(--cos-color-accent)' : 'var(--cos-color-bg-muted)',
          padding: 2,
          cursor: disabled ? 'not-allowed' : 'pointer',
          flexShrink: 0,
          position: 'relative',
          transition: `background var(--cos-duration-fast) var(--cos-ease-standard)`,
        }}
      >
        <span
          aria-hidden="true"
          style={{
            display: 'block',
            width: 20,
            height: 20,
            borderRadius: '50%',
            background: checked ? 'var(--cos-color-accent-fg)' : 'var(--cos-color-bg-elevated)',
            transform: checked ? 'translateX(18px)' : 'translateX(0)',
            transition: `transform var(--cos-duration-fast) var(--cos-ease-standard)`,
            boxShadow: 'var(--cos-shadow-soft)',
          }}
        />
      </button>
      <label htmlFor={switchId} style={{ cursor: disabled ? 'not-allowed' : 'pointer' }}>
        <span style={{ display: 'block', fontSize: '0.9rem', color: 'var(--cos-color-fg)', fontWeight: 500 }}>
          {label}
        </span>
        {description ? (
          <span style={{ display: 'block', fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)', marginTop: 2 }}>
            {description}
          </span>
        ) : null}
      </label>
    </div>
  );
}
