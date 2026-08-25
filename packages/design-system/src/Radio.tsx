'use client';

import type { InputHTMLAttributes, ReactNode } from 'react';

export type RadioProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & {
  label: ReactNode;
  description?: ReactNode;
};

export function Radio({ label, description, id, style, ...rest }: RadioProps) {
  const radioId = id ?? (typeof label === 'string' ? `radio-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
  return (
    <label
      htmlFor={radioId}
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 'var(--cos-space-2)',
        fontFamily: 'var(--cos-font-sans)',
        cursor: rest.disabled ? 'not-allowed' : 'pointer',
        opacity: rest.disabled ? 0.55 : 1,
        ...style,
      }}
    >
      <input
        id={radioId}
        type="radio"
        {...rest}
        style={{
          width: '1.05rem',
          height: '1.05rem',
          marginTop: '0.15rem',
          accentColor: 'var(--cos-color-accent)',
          flexShrink: 0,
        }}
      />
      <span>
        <span style={{ display: 'block', fontSize: '0.9rem', color: 'var(--cos-color-fg)', fontWeight: 500 }}>
          {label}
        </span>
        {description ? (
          <span style={{ display: 'block', fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)', marginTop: 2 }}>
            {description}
          </span>
        ) : null}
      </span>
    </label>
  );
}

export type RadioGroupProps = {
  name: string;
  label?: ReactNode;
  value?: string;
  onChange?: (value: string) => void;
  options: { value: string; label: ReactNode; description?: ReactNode; disabled?: boolean }[];
  disabled?: boolean;
};

export function RadioGroup({ name, label, value, onChange, options, disabled }: RadioGroupProps) {
  return (
    <fieldset
      disabled={disabled}
      style={{
        border: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 'var(--cos-space-2)',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      {label ? (
        <legend style={{ fontSize: '0.8125rem', fontWeight: 600, color: 'var(--cos-color-fg)', marginBottom: 4 }}>
          {label}
        </legend>
      ) : null}
      {options.map((o) => (
        <Radio
          key={o.value}
          name={name}
          value={o.value}
          label={o.label}
          description={o.description}
          disabled={o.disabled}
          checked={value === o.value}
          onChange={() => onChange?.(o.value)}
        />
      ))}
    </fieldset>
  );
}
