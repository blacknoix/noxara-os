'use client';

import type { CSSProperties } from 'react';
import Link from 'next/link';
import { useCapabilities } from '../../lib/capabilities';

const LINKS = [
  { href: '/ops/projects', label: 'Projects', desc: 'Delivery workstreams and deal-won projects.', perm: 'operations.project.read' },
  { href: '/ops/tasks', label: 'Tasks', desc: 'Board, list, and calendar for operations work.', perm: 'operations.task.read' },
  { href: '/my-work', label: 'My work', desc: 'Assigned tasks and mentions for you.', perm: 'operations.task.read' },
];

export default function OpsPage() {
  const { can, loading } = useCapabilities();

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Ops</p>
        <h1 style={h1}>Operations</h1>
        <p style={muted}>Projects and tasks for delivery work after a deal is won.</p>
      </header>
      <nav aria-label="Operations" style={{ display: 'grid', gap: '0.75rem', maxWidth: 520 }}>
        {LINKS.filter((l) => loading || can(l.perm)).map((l) => (
          <Link key={l.href} href={l.href} style={cardLink}>
            <strong style={{ fontFamily: 'var(--cos-font-display)', fontSize: '1.05rem' }}>{l.label}</strong>
            <span style={{ color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' }}>{l.desc}</span>
          </Link>
        ))}
      </nav>
    </div>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const h1: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: 'clamp(1.75rem, 2.5vw, 2.25rem)',
  fontWeight: 650,
  letterSpacing: '-0.02em',
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 560,
};

const cardLink: CSSProperties = {
  display: 'grid',
  gap: '0.25rem',
  padding: '0.85rem 1rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  textDecoration: 'none',
  color: 'var(--cos-color-fg)',
  background: 'var(--cos-color-bg-elevated)',
};
