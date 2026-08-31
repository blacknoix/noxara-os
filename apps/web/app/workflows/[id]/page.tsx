'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import {
  Button,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Select,
  Textarea,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Trigger = { event_key: string; description: string };
type Action = { key: string; description: string; required_permission: string; high_risk: boolean };

type GraphNode =
  | { type: 'action'; id: string; action: string; params: Record<string, unknown>; next?: string }
  | { type: 'end'; id: string }
  | { type: 'timer'; id: string; duration_secs: number; next: string }
  | {
      type: 'condition';
      id: string;
      path: string;
      equals: unknown;
      then_next: string;
      else_next: string;
    };

type Graph = {
  entry: string;
  trigger: { kind: 'domain_event'; event_key: string } | { kind: 'manual' };
  nodes: GraphNode[];
  sla_seconds?: number;
};

function emptyGraph(): Graph {
  return {
    entry: 'step_1',
    trigger: { kind: 'manual' },
    nodes: [
      {
        type: 'action',
        id: 'step_1',
        action: 'create_task',
        params: { title: 'New task' },
        next: 'done',
      },
      { type: 'end', id: 'done' },
    ],
  };
}

export default function WorkflowDetailPage() {
  const params = useParams();
  const router = useRouter();
  const idParam = String(params?.id ?? '');
  const isNew = idParam === 'new';
  const { can, loading: capsLoading } = useCapabilities();
  const canRead = can('operations.workflow.read');
  const canWrite = can('operations.workflow.write');
  const canPublish = can('operations.workflow.publish');
  const canRun = can('operations.workflow.run');

  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [graph, setGraph] = useState<Graph>(emptyGraph());
  const [defId, setDefId] = useState<string | null>(isNew ? null : idParam);
  const [status, setStatus] = useState('draft');
  const [triggers, setTriggers] = useState<Trigger[]>([]);
  const [actions, setActions] = useState<Action[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [simResult, setSimResult] = useState<string | null>(null);

  const actionNode = useMemo(
    () => graph.nodes.find((n) => n.type === 'action') as Extract<GraphNode, { type: 'action' }> | undefined,
    [graph],
  );

  const loadCatalogues = useCallback(async () => {
    const [t, a] = await Promise.all([
      authFetch('/api/v1/workflows/catalogue/triggers'),
      authFetch('/api/v1/workflows/catalogue/actions'),
    ]);
    if (t.ok) {
      const data = await t.json();
      setTriggers(data.items ?? []);
    }
    if (a.ok) {
      const data = await a.json();
      setActions(data.items ?? []);
    }
  }, []);

  const loadDef = useCallback(async () => {
    if (isNew || !getAccessToken()) return;
    const res = await authFetch(`/api/v1/workflows/definitions/${idParam}`);
    if (!res.ok) {
      setError('Could not load definition.');
      return;
    }
    const data = await res.json();
    setDefId(data.id);
    setName(data.name ?? '');
    setDescription(data.description ?? '');
    setStatus(data.status ?? 'draft');
    if (data.graph) setGraph(data.graph);
  }, [idParam, isNew]);

  useEffect(() => {
    if (!capsLoading && canRead) {
      void loadCatalogues();
      void loadDef();
    }
  }, [capsLoading, canRead, loadCatalogues, loadDef]);

  const save = async () => {
    setError(null);
    setMessage(null);
    if (isNew || !defId) {
      const res = await authFetch('/api/v1/workflows/definitions', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name, description, graph }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setError(body.detail ?? 'Save failed (permission or validation).');
        return;
      }
      const data = await res.json();
      setMessage('Draft saved.');
      router.replace(`/workflows/${data.id}`);
      return;
    }
    const res = await authFetch(`/api/v1/workflows/definitions/${defId}`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name, description, graph }),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Update failed.');
      return;
    }
    setMessage('Draft updated.');
  };

  const publish = async () => {
    if (!defId) return;
    setError(null);
    const res = await authFetch(`/api/v1/workflows/definitions/${defId}/publish`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'idempotency-key': `pub-${crypto.randomUUID()}`,
      },
      body: JSON.stringify({}),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Publish failed.');
      return;
    }
    const data = await res.json();
    setStatus('published');
    setMessage(`Published version ${data.version}. In-flight runs keep their version.`);
  };

  const simulate = async () => {
    setSimResult(null);
    const res = await authFetch('/api/v1/workflows/simulate', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ graph, payload: { deal_id: 'dl_demo', id: 'demo' } }),
    });
    const data = await res.json();
    setSimResult(JSON.stringify(data, null, 2));
  };

  const start = async () => {
    if (!defId) return;
    const res = await authFetch(`/api/v1/workflows/definitions/${defId}/start`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'idempotency-key': `start-${crypto.randomUUID()}`,
      },
      body: JSON.stringify({ payload: { deal_id: 'dl_demo', id: 'demo' } }),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      setError(body.detail ?? 'Start failed.');
      return;
    }
    setMessage('Instance started. See Monitor for status.');
  };

  const setTriggerKey = (eventKey: string) => {
    setGraph((g) => ({
      ...g,
      trigger:
        eventKey === 'manual'
          ? { kind: 'manual' }
          : { kind: 'domain_event', event_key: eventKey },
    }));
  };

  const setActionKey = (action: string) => {
    setGraph((g) => ({
      ...g,
      nodes: g.nodes.map((n) =>
        n.type === 'action' && n.id === g.entry ? { ...n, action } : n,
      ),
    }));
  };

  const setActionTitle = (title: string) => {
    setGraph((g) => ({
      ...g,
      nodes: g.nodes.map((n) =>
        n.type === 'action' && n.id === g.entry
          ? { ...n, params: { ...n.params, title } }
          : n,
      ),
    }));
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
        <PermissionDeniedState title="Workflow" requiredPermission="operations.workflow.read" />
      </main>
    );
  }

  const triggerValue =
    graph.trigger.kind === 'manual' ? 'manual' : graph.trigger.event_key;

  return (
    <main
      id="main-content"
      tabIndex={-1}
      style={{
        padding: '1.5rem',
        maxWidth: '40rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '1rem',
      }}
    >
      <p style={{ margin: 0 }}>
        <Link href="/workflows">← Workflows</Link>
      </p>
      <h1 style={{ margin: 0, fontFamily: 'var(--cos-font-display)' }}>
        {isNew ? 'New workflow' : name || 'Workflow'}
      </h1>
      <p style={{ margin: 0, color: 'var(--cos-color-muted)' }}>Status: {status}</p>

      {error ? <ErrorState title="Error" message={error} /> : null}
      {message ? <InlineAlert tone="success">{message}</InlineAlert> : null}

      <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} disabled={!canWrite} />
      <Textarea
        label="Description"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        disabled={!canWrite}
      />

      <Select
        label="Trigger"
        value={triggerValue}
        onChange={(e) => setTriggerKey(e.target.value)}
        disabled={!canWrite}
        options={[
          { value: 'manual', label: 'Manual / API start' },
          ...triggers.map((t) => ({
            value: t.event_key,
            label: `${t.event_key} — ${t.description}`,
          })),
        ]}
      />

      <Select
        label="Primary action"
        value={actionNode?.action ?? 'create_task'}
        onChange={(e) => setActionKey(e.target.value)}
        disabled={!canWrite}
        options={actions.map((a) => ({
          value: a.key,
          label: `${a.key}${a.high_risk ? ' (high-risk)' : ''} — needs ${a.required_permission}`,
        }))}
      />

      <Input
        label="Action title / subject"
        value={String(actionNode?.params?.title ?? actionNode?.params?.notes ?? '')}
        onChange={(e) => setActionTitle(e.target.value)}
        disabled={!canWrite}
      />

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem' }}>
        {canWrite ? (
          <Button onClick={() => void save()}>Save draft</Button>
        ) : null}
        {canPublish && defId ? (
          <Button variant="secondary" onClick={() => void publish()}>
            Publish
          </Button>
        ) : null}
        <Button variant="secondary" onClick={() => void simulate()}>
          Simulate (dry-run)
        </Button>
        {canRun && defId && status === 'published' ? (
          <Button variant="secondary" onClick={() => void start()}>
            Start instance
          </Button>
        ) : null}
      </div>

      {simResult ? (
        <section aria-labelledby="sim-heading">
          <h2 id="sim-heading" style={{ fontSize: '1rem' }}>
            Simulation result
          </h2>
          <pre
            style={{
              overflow: 'auto',
              padding: '0.75rem',
              background: 'var(--cos-color-surface-muted, #f4f4f5)',
              fontSize: '0.8rem',
            }}
          >
            {simResult}
          </pre>
        </section>
      ) : null}

      <InlineAlert tone="info">
        Actions run on_behalf_of the definition creator. A Member-created workflow cannot
        read payroll or post journals.
      </InlineAlert>
    </main>
  );
}
