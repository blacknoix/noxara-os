import type { TextareaHTMLAttributes, ReactNode } from 'react';
import { fieldStyle } from './Input';

export type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement> & {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
};

export function Textarea({ label, hint, error, id, style, rows = 4, ...rest }: TextareaProps) {
  const areaId = id ?? (typeof label === 'string' ? `textarea-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
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
      <textarea
        id={areaId}
        rows={rows}
        aria-invalid={error ? true : undefined}
        {...rest}
        style={{
          ...fieldStyle,
          borderColor: error ? 'var(--cos-color-danger)' : 'var(--cos-color-border)',
          resize: 'vertical',
          minHeight: '5rem',
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
