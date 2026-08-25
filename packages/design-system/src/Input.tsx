import type { InputHTMLAttributes } from 'react';

export type InputProps = InputHTMLAttributes<HTMLInputElement>;

export function Input(props: InputProps) {
  return (
    <input
      {...props}
      style={{
        fontFamily: 'var(--cos-font-sans)',
        fontSize: '0.9rem',
        padding: '0.55rem 0.75rem',
        borderRadius: 'var(--cos-radius-sm)',
        border: '1px solid var(--cos-color-border)',
        background: 'var(--cos-color-bg-elevated)',
        color: 'var(--cos-color-fg)',
        width: '100%',
        ...props.style,
      }}
    />
  );
}
