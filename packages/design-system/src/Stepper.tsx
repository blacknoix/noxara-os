import type { ReactNode } from 'react';

export type StepperStep = {
  id: string;
  label: ReactNode;
  description?: ReactNode;
};

export type StepperProps = {
  steps: StepperStep[];
  current: number;
};

export function Stepper({ steps, current }: StepperProps) {
  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexWrap: 'wrap',
        gap: 'var(--cos-space-4)',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      {steps.map((step, i) => {
        const done = i < current;
        const active = i === current;
        return (
          <li
            key={step.id}
            aria-current={active ? 'step' : undefined}
            style={{ display: 'flex', alignItems: 'flex-start', gap: 'var(--cos-space-2)', minWidth: 120 }}
          >
            <span
              aria-hidden="true"
              style={{
                width: 24,
                height: 24,
                borderRadius: '50%',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: '0.75rem',
                fontWeight: 700,
                background: done || active ? 'var(--cos-color-accent)' : 'var(--cos-color-bg-muted)',
                color: done || active ? 'var(--cos-color-accent-fg)' : 'var(--cos-color-fg-muted)',
                border: '1px solid var(--cos-color-border)',
                flexShrink: 0,
              }}
            >
              {done ? '✓' : i + 1}
            </span>
            <span>
              <span
                style={{
                  display: 'block',
                  fontWeight: active ? 700 : 500,
                  color: active ? 'var(--cos-color-fg)' : 'var(--cos-color-fg-muted)',
                  fontSize: '0.875rem',
                }}
              >
                {step.label}
              </span>
              {step.description ? (
                <span style={{ display: 'block', fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>
                  {step.description}
                </span>
              ) : null}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
