'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';

type Task = {
  id: string;
  project_id: string;
  title: string;
  status: string;
  priority: string;
  due_at: string | null;
};

type Mention = {
  id: string;
  author_user_id: string;
  body: string;
  created_at: string;
};

type MyWorkResponse = {
  assigned: Task[];
  mentions: Mention[];
  total_assigned: number;
};

export default function MyWorkPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [data, setData] = useState<MyWorkResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/operations/my-work?limit=50');
    setRequestId(res.headers.get('x-request-id') ?? undefined);
    if (res.status === 401) {
      setError('Sign in required');
      setLoading(false);
      return;
    }
    if (res.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!res.ok) {
      setError('Could not load my work');
      setLoading(false);
      return;
    }
    setData((await res.json()) as MyWorkResponse);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading || loading) {
    return <LoadingState label="Loading my work" rows={3} />;
  }
  if (denied || !can('operations.task.read')) {
    return <PermissionDeniedState requiredPermission="operations.task.read" />;
  }
  if (error) {
    return <ErrorState message={error} requestId={requestId} />;
  }

  const assigned = data?.assigned ?? [];
  const mentions = data?.mentions ?? [];

  return (
    <div style={{ display: 'grid', gap: '1.5rem' }}>
      <header>
        <p style={eyebrow}>Work</p>
        <h1 style={h1}>My work</h1>
        <p style={muted}>
          Tasks assigned to you and recent mentions.{' '}
          <Link href="/ops/tasks" style={{ color: 'var(--cos-color-accent)' }}>
            Open tasks board
          </Link>
        </p>
      </header>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Assigned ({data?.total_assigned ?? assigned.length})</h2>
        {assigned.length === 0 ? (
          <EmptyState
            title="No work items yet"
            description="Tasks assigned to you will show up here."
          />
        ) : (
          <Table
            getRowKey={(t) => t.id}
            columns={[
              { key: 'title', header: 'Title', cell: (t: Task) => t.title },
              {
                key: 'status',
                header: 'Status',
                cell: (t: Task) => <StatusCell status={t.status} />,
              },
              { key: 'priority', header: 'Priority', cell: (t: Task) => t.priority },
              {
                key: 'due',
                header: 'Due',
                cell: (t: Task) => (t.due_at ? t.due_at.slice(0, 10) : '—'),
              },
              {
                key: 'project',
                header: 'Project',
                cell: (t: Task) => (
                  <Link href={`/ops/projects/${t.project_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                    {t.project_id}
                  </Link>
                ),
              },
            ]}
            rows={assigned}
          />
        )}
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Mentions</h2>
        {mentions.length === 0 ? (
          <EmptyState title="No mentions" description="When someone @mentions you on a task, it appears here." />
        ) : (
          <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: '0.65rem' }}>
            {mentions.map((m) => (
              <li
                key={m.id}
                style={{
                  borderBottom: '1px solid var(--cos-color-border)',
                  paddingBottom: '0.65rem',
                }}
              >
                <div style={{ fontSize: '0.8rem', color: 'var(--cos-color-fg-muted)' }}>
                  {m.author_user_id} · {new Date(m.created_at).toLocaleString()}
                </div>
                <div style={{ marginTop: '0.25rem' }}>{m.body}</div>
              </li>
            ))}
          </ul>
        )}
      </section>
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

const h2: CSSProperties = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.1rem',
  fontWeight: 650,
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 560,
};
