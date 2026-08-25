import type { SelectHTMLAttributes, ReactNode } from 'react';
import { fieldStyle } from './Input';

export type SelectOption = { value: string; label: string; disabled?: boolean };

export type SelectProps = Omit<SelectHTMLAttributes<HTMLSelectElement>, 'children'> & {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  options: SelectOption[];
  placeholder?: string;
};

export function Select({ label, hint, error, options, placeholder, id, style, ...rest }: SelectProps) {
  const selectId = id ?? (typeof label === 'string' ? `select-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
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
      <select
        id={selectId}
        aria-invalid={error ? true : undefined}
        {...rest}
        style={{
          ...fieldStyle,
          borderColor: error ? 'var(--cos-color-danger)' : 'var(--cos-color-border)',
          ...style,
        }}
      >
        {placeholder ? (
          <option value="" disabled>
            {placeholder}
          </option>
        ) : null}
        {options.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label}
          </option>
        ))}
      </select>
      {error || hint ? (
        <span style={{ fontSize: '0.75rem', color: error ? 'var(--cos-color-danger)' : 'var(--cos-color-fg-muted)' }}>
          {error ?? hint}
        </span>
      ) : null}
    </label>
  );
}
