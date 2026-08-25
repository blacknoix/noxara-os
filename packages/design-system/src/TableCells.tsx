'use client';

import type { CSSProperties, ReactNode } from 'react';
import { Avatar } from './Avatar';
import { Badge } from './Badge';

export type MoneyCellProps = {
  amount: number;
  currency?: string;
  locale?: string;
};

export function MoneyCell({ amount, currency = 'USD', locale = 'en-US' }: MoneyCellProps) {
  const formatted = new Intl.NumberFormat(locale, { style: 'currency', currency }).format(amount);
  return (
    <span style={{ fontVariantNumeric: 'tabular-nums', fontFamily: 'var(--cos-font-sans)' }}>{formatted}</span>
  );
}

export type DateCellProps = {
  value: string | Date | number | null | undefined;
  format?: Intl.DateTimeFormatOptions;
  locale?: string;
};

export function DateCell({
  value,
  format = { year: 'numeric', month: 'short', day: 'numeric' },
  locale = 'en-US',
}: DateCellProps) {
  if (value == null || value === '') {
    return <span style={{ color: 'var(--cos-color-fg-muted)' }}>—</span>;
  }
  const d = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(d.getTime())) {
    return <span style={{ color: 'var(--cos-color-fg-muted)' }}>—</span>;
  }
  return (
    <time dateTime={d.toISOString()} style={{ fontFamily: 'var(--cos-font-sans)' }}>
      {new Intl.DateTimeFormat(locale, format).format(d)}
    </time>
  );
}

export type StatusCellProps = {
  status: string;
  tone?: 'neutral' | 'success' | 'warning' | 'danger' | 'info';
};

export function StatusCell({ status, tone = 'neutral' }: StatusCellProps) {
  return <Badge tone={tone}>{status}</Badge>;
}

export type AvatarCellProps = {
  name: string;
  src?: string;
  subtitle?: string;
};

export function AvatarCell({ name, src, subtitle }: AvatarCellProps) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: '0.5rem', fontFamily: 'var(--cos-font-sans)' }}>
      <Avatar name={name} src={src} size="sm" />
      <span>
        <span style={{ display: 'block', color: 'var(--cos-color-fg)', fontWeight: 500 }}>{name}</span>
        {subtitle ? (
          <span style={{ display: 'block', fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>{subtitle}</span>
        ) : null}
      </span>
    </span>
  );
}

export type LinkCellProps = {
  href: string;
  children: ReactNode;
  external?: boolean;
};

export function LinkCell({ href, children, external }: LinkCellProps) {
  const linkStyle: CSSProperties = {
    color: 'var(--cos-color-accent)',
    fontFamily: 'var(--cos-font-sans)',
    textDecoration: 'underline',
    textUnderlineOffset: 2,
  };
  if (external) {
    return (
      <a href={href} target="_blank" rel="noopener noreferrer" style={linkStyle}>
        {children}
      </a>
    );
  }
  return (
    <a href={href} style={linkStyle}>
      {children}
    </a>
  );
}
