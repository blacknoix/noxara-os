'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  InlineAlert,
  PermissionDeniedState,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';

type WorkflowDefinition = {
  id: string;
  name: string;
  description: string;
  status: string;
  current_published_version?: number | null;
  updated_at: string;
};

export default function WorkflowsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [items, setItems] = useState<WorkflowDefinition[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const canRead = can('operations.workflow.read');
  const canWrite = can('operations.workflow.write');

  const load = useCallback(async () => {
    if (!getAccessToken()) return;
    setBusy(true);
    setError(null);
    try {
      const res = await authFetch('/api/v1/workflows/definitions');
      if (!res.ok) {
        setError('Could not load workflows.');
        return;
      }
      const data = await res.json();
      setItems(data.items ?? []);
    } catch {
      setError('Could not load workflows.');
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (!capsLoading && canRead) void load();
  }, [capsLoading, canRead, load]);

  const createFromFixture = async (fixtureIndex: number) => {
    const fixturesRes = await authFetch('/api/v1/workflows/fixtures');
    if (!fixturesRes.ok) {
      setError('Could not load fixtures.');
      return;
    }
    const fixtures = await fixturesRes.json();
    const fx = fixtures.items?.[fixtureIndex];
    if (!fx) return;
    const res = await authFetch('/api/v1/workflows/definitions', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        name: fx.name,
        description: fx.description,
        graph: fx.graph,
      }),
    });
    if (!res.ok) {
      setError('Could not create workflow from fixture.');
      return;
    }
    await load();
  };

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
        <PermissionDeniedState title="Workflows" requiredPermission="operations.workflow.read" />
      </main>
    );
  }

  return (
    <main
      id="main-content"
      tabIndex={-1}
      style={{
        padding: '1.5rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '1.25rem',
        maxWidth: '56rem',
      }}
    >
      <header style={{ display: 'flex', justifyContent: 'space-between', gap: '1rem', flexWrap: 'wrap' }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '1.75rem', fontFamily: 'var(--cos-font-display)' }}>
            Workflows
          </h1>
          <p style={{ margin: '0.35rem 0 0', color: 'var(--cos-color-muted)' }}>
            Configure event-driven automations. Definitions are data — not code.
          </p>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem', alignItems: 'flex-start' }}>
          <Link href="/workflows/monitor">
            <Button variant="secondary">Monitor</Button>
          </Link>
          {canWrite ? (
            <Link href="/workflows/new">
              <Button>New workflow</Button>
            </Link>
          ) : null}
        </div>
      </header>

      {error ? <ErrorState title="Something went wrong" message={error} /> : null}

      {canWrite ? (
        <section aria-labelledby="fixtures-heading">
          <h2 id="fixtures-heading" style={{ fontSize: '1rem', margin: '0 0 0.5rem' }}>
            Start from a fixture
          </h2>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem' }}>
            <Button variant="secondary" onClick={() => void createFromFixture(0)}>
              Deal won → task
            </Button>
            <Button variant="secondary" onClick={() => void createFromFixture(1)}>
              Leave approved → notify
            </Button>
            <Button variant="secondary" onClick={() => void createFromFixture(2)}>
              Low stock → PR draft
            </Button>
          </div>
        </section>
      ) : null}

      <section aria-labelledby="list-heading">
        <h2 id="list-heading" style={{ fontSize: '1rem', margin: '0 0 0.75rem' }}>
          Definitions
        </h2>
        {busy && items.length === 0 ? <p>Loading…</p> : null}
        {!busy && items.length === 0 ? (
          <EmptyState
            title="No workflows yet"
            description="Create a definition or start from a fixture."
          />
        ) : (
          <ul style={{ listStyle: 'none', padding: 0, margin: 0, display: 'flex', flexDirection: 'column', gap: '0.5rem' }}>
            {items.map((item) => (
              <li key={item.id}>
                <Link
                  href={`/workflows/${item.id}`}
                  style={{
                    display: 'block',
                    padding: '0.85rem 1rem',
                    borderBottom: '1px solid var(--cos-color-border)',
                    textDecoration: 'none',
                    color: 'inherit',
                  }}
                >
                  <strong>{item.name}</strong>
                  <span style={{ marginLeft: '0.75rem', color: 'var(--cos-color-muted)', fontSize: '0.875rem' }}>
                    {item.status}
                    {item.current_published_version != null
                      ? ` · v${item.current_published_version}`
                      : ''}
                  </span>
                  {item.description ? (
                    <div style={{ color: 'var(--cos-color-muted)', fontSize: '0.875rem', marginTop: '0.25rem' }}>
                      {item.description}
                    </div>
                  ) : null}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>

      <InlineAlert tone="info">
        Publishing is a human commit. AI may propose graphs but cannot auto-publish.
      </InlineAlert>
    </main>
  );
}
