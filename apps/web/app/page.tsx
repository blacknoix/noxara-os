'use client';

import { useEffect, useState } from 'react';
import { EmptyState, ErrorState } from '@companyos/design-system';

type Status = 'loading' | 'empty' | 'error' | 'ready';

export default function DashboardPage() {
  const [status, setStatus] = useState<Status>('loading');

  useEffect(() => {
    const t = window.setTimeout(() => {
      // Phase 0: no fake CRM data — settle on empty.
      setStatus('empty');
    }, 450);
    return () => window.clearTimeout(t);
  }, []);

  return (
    <section>
      <header style={{ marginBottom: '1.25rem' }}>
        <p
          style={{
            margin: 0,
            textTransform: 'uppercase',
            letterSpacing: '0.08em',
            fontSize: '0.72rem',
            color: 'var(--cos-color-fg-muted)',
            fontWeight: 600,
          }}
        >
          Work
        </p>
        <h1
          style={{
            margin: '0.35rem 0 0',
            fontFamily: 'var(--cos-font-display)',
            fontSize: 'clamp(1.75rem, 2.5vw, 2.25rem)',
            fontWeight: 650,
            letterSpacing: '-0.02em',
          }}
        >
          Dashboard
        </h1>
        <p style={{ margin: '0.4rem 0 0', color: 'var(--cos-color-fg-muted)', maxWidth: 520 }}>
          Phase 0 shell — loading, empty, and error states only. No demo CRM metrics.
        </p>
      </header>

      {status === 'loading' ? (
        <div
          aria-busy="true"
          style={{
            height: 120,
            borderRadius: 'var(--cos-radius-md)',
            background:
              'linear-gradient(90deg, transparent, color-mix(in srgb, var(--cos-color-border) 65%, transparent), transparent)',
            backgroundSize: '200% 100%',
            animation: 'cos-shimmer 1.2s ease-in-out infinite',
          }}
        />
      ) : null}

      {status === 'empty' ? (
        <EmptyState
          title="Nothing here yet"
          description="When deals, tasks, and invoices arrive, they will show up in Work. Seed an org via scripts/dev-up to explore the hello API."
        />
      ) : null}

      {status === 'error' ? (
        <ErrorState message="Could not load the dashboard. Retry once the gateway is up." />
      ) : null}

      {status === 'ready' ? (
        <p style={{ color: 'var(--cos-color-fg-muted)' }}>Ready state reserved for Phase 1 data.</p>
      ) : null}

      <style>{`
        @keyframes cos-shimmer {
          0% { background-position: 100% 0; }
          100% { background-position: -100% 0; }
        }
      `}</style>
    </section>
  );
}
