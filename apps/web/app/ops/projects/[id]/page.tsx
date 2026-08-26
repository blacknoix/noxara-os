'use client';

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

type Project = {
  id: string;
  name: string;
  description: string | null;
  status: string;
  owner_user_id: string;
  customer_id: string | null;
  deal_id: string | null;
  starts_at: string | null;
  due_at: string | null;
  created_at: string;
};

type Task = {
  id: string;
  title: string;
  status: string;
  priority: string;
  assignee_id: string | null;
  due_at: string | null;
};

export default function ProjectDetailPage() {
  const params = useParams();
  const id = typeof params?.id === 'string' ? params.id : '';
  const { can, loading: capsLoading } = useCapabilities();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();

  const load = useCallback(async () => {
    if (!id || !getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const [projRes, taskRes] = await Promise.all([
      authFetch(`/api/v1/operations/projects/${id}`),
      authFetch(`/api/v1/operations/tasks?project_id=${encodeURIComponent(id)}&limit=100`),
    ]);
    setRequestId(projRes.headers.get('x-request-id') ?? undefined);
    if (projRes.status === 401) {
      setError('Sign in required');
      setLoading(false);
      return;
    }
    if (projRes.status === 403) {
      setDenied(true);
      setLoading(false);
      return;
    }
    if (!projRes.ok) {
      setError(projRes.status === 404 ? 'Project not found' : 'Could not load project');
      setLoading(false);
      return;
    }
    setProject((await projRes.json()) as Project);
    if (taskRes.ok) {
      const body = await taskRes.json();
      setTasks(body.items ?? []);
    }
    setLoading(false);
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  if (capsLoading || loading) {
    return <LoadingState label="Loading project" rows={3} />;
  }
  if (denied || !can('operations.project.read')) {
    return <PermissionDeniedState requiredPermission="operations.project.read" />;
  }
  if (error) {
    return <ErrorState message={error} requestId={requestId} />;
  }
  if (!project) {
    return <EmptyState title="Project not found" description="It may have been deleted." />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>
          <Link href="/ops/projects" style={{ color: 'var(--cos-color-accent)' }}>
            Projects
          </Link>
        </p>
        <h1 style={h1}>{project.name}</h1>
        <p style={muted}>{project.description || 'No description.'}</p>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.75rem', marginTop: '0.75rem' }}>
          <StatusCell status={project.status} />
          {project.due_at ? <span style={meta}>Due {project.due_at}</span> : null}
          {project.deal_id ? <span style={meta}>Deal {project.deal_id}</span> : null}
          {project.customer_id ? <span style={meta}>Customer {project.customer_id}</span> : null}
        </div>
      </header>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'baseline',
            gap: '1rem',
            flexWrap: 'wrap',
          }}
        >
          <h2 style={h2}>Tasks</h2>
          <Link
            href={`/ops/tasks?project_id=${encodeURIComponent(project.id)}`}
            style={{ color: 'var(--cos-color-accent)', fontSize: '0.9rem' }}
          >
            Open board / list
          </Link>
        </div>
        {tasks.length === 0 ? (
          <EmptyState
            title="No tasks yet"
            description="Create tasks from the Tasks board to track work on this project."
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
              { key: 'assignee', header: 'Assignee', cell: (t: Task) => t.assignee_id ?? '—' },
              { key: 'due', header: 'Due', cell: (t: Task) => t.due_at ?? '—' },
            ]}
            rows={tasks}
          />
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
  fontSize: '1.15rem',
  fontWeight: 650,
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
  maxWidth: 640,
};

const meta: CSSProperties = {
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
