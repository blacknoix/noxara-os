'use client';

import type { InputHTMLAttributes, ReactNode, CSSProperties } from 'react';

export type CheckboxProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & {
  label?: ReactNode;
  description?: ReactNode;
};

const visuallyHidden: CSSProperties = {
  position: 'absolute',
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: 'hidden',
  clip: 'rect(0, 0, 0, 0)',
  whiteSpace: 'nowrap',
  border: 0,
};

export function Checkbox({ label, description, id, style, ...rest }: CheckboxProps) {
  const checkId =
    id ?? (typeof label === 'string' ? `check-${label.replace(/\s+/g, '-').toLowerCase()}` : undefined);
  const showLabel = label != null;
  return (
    <label
      htmlFor={checkId}
      style={{
        display: 'inline-flex',
        alignItems: 'flex-start',
        gap: showLabel ? 'var(--cos-space-2)' : 0,
        fontFamily: 'var(--cos-font-sans)',
        cursor: rest.disabled ? 'not-allowed' : 'pointer',
        opacity: rest.disabled ? 0.55 : 1,
        position: 'relative',
        ...style,
      }}
    >
      <input
        id={checkId}
        type="checkbox"
        {...rest}
        style={{
          width: '1.05rem',
          height: '1.05rem',
          marginTop: showLabel ? '0.15rem' : 0,
          accentColor: 'var(--cos-color-accent)',
          flexShrink: 0,
        }}
      />
      {showLabel ? (
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
      ) : rest['aria-label'] ? (
        <span style={visuallyHidden}>{rest['aria-label']}</span>
      ) : null}
    </label>
  );
}
