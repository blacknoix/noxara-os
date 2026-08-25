import type { ButtonHTMLAttributes, ReactNode } from 'react';

export type TagProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  children: ReactNode;
  onRemove?: () => void;
};

export function Tag({ children, onRemove, style, ...rest }: TagProps) {
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        fontFamily: 'var(--cos-font-sans)',
        fontSize: '0.8125rem',
        padding: '0.2rem 0.5rem',
        borderRadius: 'var(--cos-radius-sm)',
        background: 'var(--cos-color-bg-muted)',
        border: '1px solid var(--cos-color-border)',
        color: 'var(--cos-color-fg)',
        ...style,
      }}
    >
      {children}
      {onRemove ? (
        <button
          type="button"
          aria-label="Remove"
          onClick={onRemove}
          {...rest}
          style={{
            all: 'unset',
            cursor: 'pointer',
            fontWeight: 700,
            color: 'var(--cos-color-fg-muted)',
            lineHeight: 1,
          }}
        >
          ×
        </button>
      ) : null}
    </span>
  );
}
