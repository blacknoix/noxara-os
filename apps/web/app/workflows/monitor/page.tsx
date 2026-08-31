'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Summary = {
  running: number;
  waiting: number;
  failed: number;
  completed: number;
  cancelled: number;
  sla_breached: number;
};

type Instance = {
  id: string;
  definition_id: string;
  version_number: number;
  status: string;
  temporal_workflow_id: string;
  step_count: number;
  error_message?: string | null;
  sla_deadline?: string | null;
  updated_at: string;
};

export default function WorkflowMonitorPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const canRead = can('operations.workflow.read');
  const [summary, setSummary] = useState<Summary | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) return;
    setError(null);
    const res = await authFetch('/api/v1/workflows/monitor');
    if (!res.ok) {
      setError('Could not load monitor.');
      return;
    }
    const data = await res.json();
    setSummary(data.summary);
    setInstances(data.instances ?? []);
  }, []);

  useEffect(() => {
    if (!capsLoading && canRead) void load();
  }, [capsLoading, canRead, load]);

  if (capsLoading) {
    return (
      <main id="main-content" tabIndex={-1} style={{ padding: '1.5rem' }}>
        <p>Loading…</p>
      </main>
    );
  }
  if (!canRead) {
    return (
      <main id="main-content" tabIndex={-1} style={{ padding: '1.5rem' }}>
        <PermissionDeniedState title="Workflow monitor" requiredPermission="operations.workflow.read" />
      </main>
    );
  }

  return (
    <main
      id="main-content"
      tabIndex={-1}
      style={{
        padding: '1.5rem',
        maxWidth: '56rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '1.25rem',
      }}
    >
      <header style={{ display: 'flex', justifyContent: 'space-between', gap: '1rem' }}>
        <div>
          <p style={{ margin: 0 }}>
            <Link href="/workflows">← Workflows</Link>
          </p>
          <h1 style={{ margin: '0.35rem 0 0', fontFamily: 'var(--cos-font-display)' }}>
            Workflow monitor
          </h1>
        </div>
        <Button variant="secondary" onClick={() => void load()}>
          Refresh
        </Button>
      </header>

      {error ? <ErrorState title="Error" message={error} /> : null}

      {summary ? (
        <section aria-labelledby="summary-heading">
          <h2 id="summary-heading" style={{ fontSize: '1rem', margin: '0 0 0.75rem' }}>
            Summary
          </h2>
          <dl
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(7rem, 1fr))',
              gap: '0.75rem',
              margin: 0,
            }}
          >
            {(
              [
                ['Running', summary.running],
                ['Waiting', summary.waiting],
                ['Failed', summary.failed],
                ['SLA breached', summary.sla_breached],
                ['Completed', summary.completed],
                ['Cancelled', summary.cancelled],
              ] as const
            ).map(([label, value]) => (
              <div key={label}>
                <dt style={{ color: 'var(--cos-color-muted)', fontSize: '0.8rem' }}>{label}</dt>
                <dd style={{ margin: 0, fontSize: '1.35rem', fontWeight: 600 }}>{value}</dd>
              </div>
            ))}
          </dl>
        </section>
      ) : null}

      <section aria-labelledby="active-heading">
        <h2 id="active-heading" style={{ fontSize: '1rem', margin: '0 0 0.75rem' }}>
          Active & failed
        </h2>
        {instances.length === 0 ? (
          <EmptyState title="Nothing to show" description="No running, waiting, failed, or SLA-breached instances." />
        ) : (
          <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
            {instances.map((inst) => (
              <li
                key={inst.id}
                style={{
                  padding: '0.75rem 0',
                  borderBottom: '1px solid var(--cos-color-border)',
                }}
              >
                <div>
                  <strong>{inst.status}</strong> · {inst.id} · v{inst.version_number}
                </div>
                <div style={{ fontSize: '0.8rem', color: 'var(--cos-color-muted)' }}>
                  {inst.temporal_workflow_id} · steps {inst.step_count}
                </div>
                {inst.error_message ? (
                  <div style={{ fontSize: '0.875rem', marginTop: '0.25rem' }}>{inst.error_message}</div>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
