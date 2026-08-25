import type { InputHTMLAttributes, ReactNode } from 'react';

export type InputProps = InputHTMLAttributes<HTMLInputElement> & {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
};

const fieldStyle = {
  fontFamily: 'var(--cos-font-sans)',
  fontSize: '0.9rem',
  padding: '0.55rem 0.75rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
  width: '100%',
} as const;

export function Input({ label, hint, error, id, style, ...rest }: InputProps) {
  const inputId = id ?? (typeof label === 'string' ? `input-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
  return (
    <label
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 'var(--cos-space-1)',
        fontFamily: 'var(--cos-font-sans)',
        width: '100%',
      }}
    >
      {label ? (
        <span style={{ fontSize: '0.8125rem', fontWeight: 600, color: 'var(--cos-color-fg)' }}>{label}</span>
      ) : null}
      <input
        id={inputId}
        aria-invalid={error ? true : undefined}
        aria-describedby={error || hint ? `${inputId}-desc` : undefined}
        {...rest}
        style={{
          ...fieldStyle,
          borderColor: error ? 'var(--cos-color-danger)' : 'var(--cos-color-border)',
          ...style,
        }}
      />
      {error || hint ? (
        <span
          id={inputId ? `${inputId}-desc` : undefined}
          style={{
            fontSize: '0.75rem',
            color: error ? 'var(--cos-color-danger)' : 'var(--cos-color-fg-muted)',
          }}
        >
          {error ?? hint}
        </span>
      ) : null}
    </label>
  );
}

export { fieldStyle };
