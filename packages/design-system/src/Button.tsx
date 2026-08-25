'use client';

import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from 'react';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
type Size = 'sm' | 'md';

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  leftIcon?: ReactNode;
  children: ReactNode;
};

const variantStyles: Record<Variant, CSSProperties> = {
  primary: {
    background: 'var(--cos-color-accent)',
    color: 'var(--cos-color-accent-fg)',
    border: '1px solid transparent',
  },
  secondary: {
    background: 'var(--cos-color-bg-elevated)',
    color: 'var(--cos-color-fg)',
    border: '1px solid var(--cos-color-border)',
  },
  ghost: {
    background: 'transparent',
    color: 'var(--cos-color-fg)',
    border: '1px solid transparent',
  },
  danger: {
    background: 'var(--cos-color-danger)',
    color: 'var(--cos-color-danger-fg)',
    border: '1px solid transparent',
  },
};

const sizeStyles: Record<Size, CSSProperties> = {
  sm: { fontSize: '0.8125rem', padding: '0.35rem 0.65rem' },
  md: { fontSize: '0.875rem', padding: '0.55rem 0.9rem' },
};

export function Button({
  variant = 'primary',
  size = 'md',
  loading = false,
  leftIcon,
  children,
  style,
  disabled,
  ...rest
}: ButtonProps) {
  const isDisabled = disabled || loading;
  return (
    <button
      type="button"
      {...rest}
      disabled={isDisabled}
      aria-busy={loading || undefined}
      style={{
        fontFamily: 'var(--cos-font-sans)',
        fontWeight: 600,
        borderRadius: 'var(--cos-radius-sm)',
        cursor: isDisabled ? 'not-allowed' : 'pointer',
        opacity: isDisabled ? 0.55 : 1,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '0.4rem',
        lineHeight: 1.2,
        transition: `opacity var(--cos-duration-fast) var(--cos-ease-standard)`,
        ...variantStyles[variant],
        ...sizeStyles[size],
        ...style,
      }}
    >
      {loading ? (
        <span aria-hidden="true" style={{ display: 'inline-block', width: '0.9em', height: '0.9em' }}>
          ◌
        </span>
      ) : leftIcon ? (
        <span aria-hidden="true" style={{ display: 'inline-flex' }}>
          {leftIcon}
        </span>
      ) : null}
      {children}
    </button>
  );
}
