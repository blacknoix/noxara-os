import type { InputHTMLAttributes, ReactNode } from 'react';
import { fieldStyle } from './Input';

export type DatePickerProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
};

/** Thin native date input wrapper — keeps browser picker a11y. */
export function DatePicker({ label, hint, error, id, style, ...rest }: DatePickerProps) {
  const dateId = id ?? (typeof label === 'string' ? `date-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
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
        id={dateId}
        type="date"
        aria-invalid={error ? true : undefined}
        {...rest}
        style={{
          ...fieldStyle,
          borderColor: error ? 'var(--cos-color-danger)' : 'var(--cos-color-border)',
          ...style,
        }}
      />
      {error || hint ? (
        <span style={{ fontSize: '0.75rem', color: error ? 'var(--cos-color-danger)' : 'var(--cos-color-fg-muted)' }}>
          {error ?? hint}
        </span>
      ) : null}
    </label>
  );
}
