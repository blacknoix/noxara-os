'use client';

import type { CSSProperties, ReactNode } from 'react';
import Link from 'next/link';
import { Badge } from '@companyos/design-system';
import { humanize, statusTone } from '../lib/marketplace';

export function MarketplacePage({
  eyebrow = 'Marketplace',
  title,
  description,
  actions,
  children,
}: {
  eyebrow?: string;
  title: string;
  description: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div style={page}>
      <header style={header}>
        <div>
          <p style={eyebrowStyle}>{eyebrow}</p>
          <h1 style={titleStyle}>{title}</h1>
          <p style={muted}>{description}</p>
        </div>
        {actions ? <div style={actionsStyle}>{actions}</div> : null}
      </header>
      {children}
    </div>
  );
}

export function MarketplaceNav({
  canWrite = false,
  canReview = false,
}: {
  canWrite?: boolean;
  canReview?: boolean;
}) {
  return (
    <nav aria-label="Marketplace sections" style={nav}>
      <Link href="/marketplace" style={navLink}>
        Catalogue
      </Link>
      <Link href="/marketplace/installs" style={navLink}>
        Installed apps
      </Link>
      {canWrite ? (
        <Link href="/marketplace/publish" style={navLink}>
          Publisher
        </Link>
      ) : null}
      {canReview ? (
        <Link href="/marketplace/review" style={navLink}>
          Review queue
        </Link>
      ) : null}
    </nav>
  );
}

export function ScopeBadges({ scopes }: { scopes: string[] }) {
  if (scopes.length === 0) return <span style={muted}>No scopes</span>;
  return (
    <span style={scopeList}>
      {scopes.map((scope) => (
        <Badge key={scope} tone="neutral">
          {scope}
        </Badge>
      ))}
    </span>
  );
}

export function StatusBadge({ status }: { status?: string }) {
  return <Badge tone={statusTone(status)}>{humanize(status ?? 'unknown')}</Badge>;
}

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  lineHeight: 1.5,
};

export const marketplaceStyles = {
  section: {
    display: 'grid',
    gap: 'var(--cos-space-4)',
  } satisfies CSSProperties,
  form: {
    display: 'grid',
    gap: 'var(--cos-space-4)',
    maxWidth: 640,
  } satisfies CSSProperties,
  fieldset: {
    border: '1px solid var(--cos-color-border)',
    borderRadius: 'var(--cos-radius-sm)',
    padding: 'var(--cos-space-4)',
    display: 'grid',
    gap: 'var(--cos-space-3)',
    margin: 0,
  } satisfies CSSProperties,
  legend: {
    padding: '0 var(--cos-space-2)',
    fontWeight: 600,
    fontSize: '0.875rem',
  } satisfies CSSProperties,
  muted,
  linkButton: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '0.55rem 0.9rem',
    borderRadius: 'var(--cos-radius-sm)',
    background: 'var(--cos-color-accent)',
    color: 'var(--cos-color-accent-fg)',
    fontSize: '0.875rem',
    fontWeight: 600,
    lineHeight: 1.2,
    whiteSpace: 'nowrap',
  } satisfies CSSProperties,
  cardGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
    gap: 'var(--cos-space-4)',
  } satisfies CSSProperties,
  cardHeader: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 'var(--cos-space-3)',
  } satisfies CSSProperties,
  row: {
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 'var(--cos-space-3)',
  } satisfies CSSProperties,
};

const page: CSSProperties = {
  display: 'grid',
  gap: 'var(--cos-space-5)',
  maxWidth: 1040,
  margin: '0 auto',
};

const header: CSSProperties = {
  display: 'flex',
  alignItems: 'flex-end',
  justifyContent: 'space-between',
  gap: 'var(--cos-space-4)',
  flexWrap: 'wrap',
};

const eyebrowStyle: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const titleStyle: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.75rem',
  fontWeight: 650,
};

const actionsStyle: CSSProperties = {
  display: 'flex',
  gap: 'var(--cos-space-2)',
  flexWrap: 'wrap',
};

const nav: CSSProperties = {
  display: 'flex',
  gap: 'var(--cos-space-2)',
  flexWrap: 'wrap',
  borderBottom: '1px solid var(--cos-color-border)',
  paddingBottom: 'var(--cos-space-3)',
};

const navLink: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.4rem 0.75rem',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
  fontSize: '0.875rem',
  fontWeight: 500,
};

const scopeList: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 'var(--cos-space-1)',
};
