import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from 'react';

type Variant = 'primary' | 'secondary' | 'ghost';

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  children: ReactNode;
};

const styles: Record<Variant, CSSProperties> = {
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
};

export function Button({ variant = 'primary', children, style, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      style={{
        fontFamily: 'var(--cos-font-sans)',
        fontWeight: 600,
        fontSize: '0.875rem',
        padding: '0.55rem 0.9rem',
        borderRadius: 'var(--cos-radius-sm)',
        cursor: rest.disabled ? 'not-allowed' : 'pointer',
        opacity: rest.disabled ? 0.55 : 1,
        ...styles[variant],
        ...style,
      }}
    >
      {children}
    </button>
  );
}
