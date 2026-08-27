'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Button,
  Card,
  EmptyState,
  ErrorState,
  LoadingState,
  MoneyCell,
  PermissionDeniedState,
  StatusCell,
  Table,
  Tabs,
  Timeline,
  type TimelineItem,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Employee = {
  id: string;
  display_name: string;
  work_email: string | null;
  personal_email: string | null;
  phone: string | null;
  title: string | null;
  status: string;
  start_date: string | null;
  end_date: string | null;
  location: string | null;
  department_id: string | null;
  manager_employee_id: string | null;
  user_id: string | null;
  created_at: string;
  updated_at: string;
};

type Compensation = {
  id: string;
  label: string;
  component_type: string;
  amount_minor: number;
  currency: string;
  effective_from: string;
  effective_to: string | null;
};

type Document = {
  id: string;
  title: string;
  doc_type: string;
  file_id: string | null;
  expires_at: string | null;
  collected: boolean;
};

type TimelineEvent = {
  id: string;
  event_type: string;
  summary: string;
  occurred_at: string;
};

type LoadState =
  | { status: 'loading' }
  | { status: 'signed_out' }
  | { status: 'denied' }
  | { status: 'error'; message: string; requestId?: string }
  | { status: 'ready'; employee: Employee };

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'active':
      return 'success';
    case 'onboarding':
      return 'info';
    case 'on_leave':
      return 'warning';
    case 'offboarding':
    case 'terminated':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function EmployeeRecordPage() {
  const params = useParams<{ id: string }>();
  const employeeId = params.id;
  const { can, loading: capsLoading } = useCapabilities();

  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [tab, setTab] = useState('overview');
  const [compensation, setCompensation] = useState<Compensation[]>([]);
  const [compensationError, setCompensationError] = useState<string | null>(null);
  const [compensationDenied, setCompensationDenied] = useState(false);
  const [documents, setDocuments] = useState<Document[]>([]);
  const [documentsError, setDocumentsError] = useState<string | null>(null);
  const [documentsDenied, setDocumentsDenied] = useState(false);
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
  const [timelineError, setTimelineError] = useState<string | null>(null);
  const [offboarding, setOffboarding] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const canReadSensitive = can('hr.employee.read_sensitive');
  const canOffboard = can('hr.employee.offboard');

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setState({ status: 'signed_out' });
      return;
    }
    setState({ status: 'loading' });
    const res = await authFetch(`/api/v1/people/employees/${employeeId}`);
    const requestId = res.headers.get('x-request-id') ?? undefined;
    if (res.status === 401) {
      setState({ status: 'signed_out' });
      return;
    }
    if (res.status === 403) {
      setState({ status: 'denied' });
      return;
    }
    if (!res.ok) {
      let message = 'Could not load this employee.';
      try {
        const body = await res.json();
        if (typeof body.detail === 'string') message = body.detail;
      } catch {
        /* ignore */
      }
      setState({ status: 'error', message, requestId });
      return;
    }
    const employee = (await res.json()) as Employee;
    setState({ status: 'ready', employee });
  }, [employeeId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (state.status !== 'ready' || !canReadSensitive || tab !== 'compensation') return;
    void (async () => {
      setCompensationError(null);
      setCompensationDenied(false);
      const res = await authFetch(`/api/v1/people/employees/${employeeId}/compensation`);
      if (res.status === 403) {
        setCompensationDenied(true);
        return;
      }
      if (!res.ok) {
        setCompensationError('Could not load compensation');
        return;
      }
      const body = await res.json();
      setCompensation(body.items ?? []);
    })();
  }, [state.status, canReadSensitive, tab, employeeId]);

  useEffect(() => {
    if (state.status !== 'ready' || tab !== 'documents') return;
    void (async () => {
      setDocumentsError(null);
      setDocumentsDenied(false);
      const res = await authFetch(`/api/v1/people/employees/${employeeId}/documents`);
      if (res.status === 403) {
        setDocumentsDenied(true);
        return;
      }
      if (!res.ok) {
        setDocumentsError('Could not load documents');
        return;
      }
      const body = await res.json();
      setDocuments(body.items ?? []);
    })();
  }, [state.status, tab, employeeId]);

  useEffect(() => {
    if (state.status !== 'ready' || tab !== 'timeline') return;
    void (async () => {
      setTimelineError(null);
      const res = await authFetch(`/api/v1/people/employees/${employeeId}/timeline`);
      if (!res.ok) {
        setTimelineError('Could not load timeline');
        return;
      }
      const body = await res.json();
      setTimeline(body.items ?? []);
    })();
  }, [state.status, tab, employeeId]);

  const timelineItems: TimelineItem[] = useMemo(
    () =>
      timeline.map((t) => ({
        id: t.id,
        title: t.summary,
        description: t.event_type,
        timestamp: new Date(t.occurred_at).toLocaleString(),
      })),
    [timeline],
  );

  const tabItems = useMemo(() => {
    const items = [
      { id: 'overview', label: 'Overview' },
      ...(canReadSensitive ? [{ id: 'compensation', label: 'Compensation' }] : []),
      { id: 'documents', label: 'Documents' },
      { id: 'timeline', label: 'Timeline' },
    ];
    return items;
  }, [canReadSensitive]);

  async function offboard() {
    if (!canOffboard || state.status !== 'ready') return;
    setOffboarding(true);
    setActionError(null);
    try {
      const res = await authFetch(`/api/v1/people/employees/${employeeId}/offboard`, {
        method: 'POST',
        headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: JSON.stringify({}),
      });
      if (!res.ok) {
        let message = 'Could not offboard employee.';
        try {
          const body = await res.json();
          if (typeof body.detail === 'string') message = body.detail;
        } catch {
          /* ignore */
        }
        setActionError(message);
        return;
      }
      await load();
    } finally {
      setOffboarding(false);
    }
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (!can('hr.employee.read')) {
    return <PermissionDeniedState requiredPermission="hr.employee.read" />;
  }

  if (state.status === 'loading') {
    return <LoadingState label="Loading employee" rows={5} />;
  }

  if (state.status === 'signed_out') {
    return <ErrorState title="Sign in required" message="Open /login to view this employee." />;
  }

  if (state.status === 'denied') {
    return <PermissionDeniedState requiredPermission="hr.employee.read" />;
  }

  if (state.status === 'error') {
    return <ErrorState message={state.message} requestId={state.requestId} />;
  }

  const { employee } = state;

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '1rem',
          alignItems: 'flex-end',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <p style={eyebrow}>
            <Link href="/people" style={{ color: 'inherit', textDecoration: 'none' }}>
              People / Directory
            </Link>
          </p>
          <h1 style={h1}>{employee.display_name}</h1>
          <p style={muted}>
            <StatusCell status={employee.status} tone={statusTone(employee.status)} />
            {employee.title ? ` · ${employee.title}` : null}
          </p>
        </div>
        {canOffboard && employee.status !== 'terminated' && employee.status !== 'offboarding' ? (
          <Button
            type="button"
            variant="danger"
            disabled={offboarding}
            onClick={() => void offboard()}
          >
            {offboarding ? 'Offboarding…' : 'Offboard'}
          </Button>
        ) : null}
      </header>

      {actionError ? <ErrorState message={actionError} /> : null}

      <Tabs items={tabItems} value={tab} onChange={setTab}>
        {tab === 'overview' ? (
          <Card>
            <dl style={dlStyle}>
              <dt style={dtStyle}>Display name</dt>
              <dd style={ddStyle}>{employee.display_name}</dd>
              <dt style={dtStyle}>Work email</dt>
              <dd style={ddStyle}>{employee.work_email ?? '—'}</dd>
              <dt style={dtStyle}>Personal email</dt>
              <dd style={ddStyle}>{employee.personal_email ?? '—'}</dd>
              <dt style={dtStyle}>Phone</dt>
              <dd style={ddStyle}>{employee.phone ?? '—'}</dd>
              <dt style={dtStyle}>Title</dt>
              <dd style={ddStyle}>{employee.title ?? '—'}</dd>
              <dt style={dtStyle}>Status</dt>
              <dd style={ddStyle}>
                <StatusCell status={employee.status} tone={statusTone(employee.status)} />
              </dd>
              <dt style={dtStyle}>Start date</dt>
              <dd style={ddStyle}>{employee.start_date ?? '—'}</dd>
              <dt style={dtStyle}>End date</dt>
              <dd style={ddStyle}>{employee.end_date ?? '—'}</dd>
              <dt style={dtStyle}>Location</dt>
              <dd style={ddStyle}>{employee.location ?? '—'}</dd>
              <dt style={dtStyle}>Department</dt>
              <dd style={ddStyle}>{employee.department_id ?? '—'}</dd>
              <dt style={dtStyle}>Manager</dt>
              <dd style={ddStyle}>
                {employee.manager_employee_id ? (
                  <Link
                    href={`/people/${employee.manager_employee_id}`}
                    style={{ color: 'var(--cos-color-accent)' }}
                  >
                    {employee.manager_employee_id}
                  </Link>
                ) : (
                  '—'
                )}
              </dd>
              <dt style={dtStyle}>Linked user</dt>
              <dd style={ddStyle}>{employee.user_id ?? '—'}</dd>
            </dl>
          </Card>
        ) : null}

        {tab === 'compensation' && canReadSensitive ? (
          compensationDenied ? (
            <PermissionDeniedState requiredPermission="hr.employee.read_sensitive" />
          ) : compensationError ? (
            <ErrorState message={compensationError} />
          ) : compensation.length === 0 ? (
            <EmptyState
              title="No compensation components"
              description="Salary and other components appear here when recorded."
            />
          ) : (
            <Table
              getRowKey={(c: Compensation) => c.id}
              columns={[
                { key: 'label', header: 'Label', cell: (c: Compensation) => c.label },
                {
                  key: 'type',
                  header: 'Type',
                  cell: (c: Compensation) => c.component_type,
                },
                {
                  key: 'amount',
                  header: 'Amount',
                  align: 'right',
                  cell: (c: Compensation) => (
                    <MoneyCell amount={c.amount_minor / 100} currency={c.currency} />
                  ),
                },
                {
                  key: 'effective_from',
                  header: 'From',
                  cell: (c: Compensation) => c.effective_from,
                },
                {
                  key: 'effective_to',
                  header: 'To',
                  cell: (c: Compensation) => c.effective_to ?? '—',
                },
              ]}
              rows={compensation}
            />
          )
        ) : null}

        {tab === 'documents' ? (
          documentsDenied ? (
            <PermissionDeniedState requiredPermission="hr.document.read" />
          ) : documentsError ? (
            <ErrorState message={documentsError} />
          ) : documents.length === 0 ? (
            <EmptyState
              title="No documents"
              description="HR documents linked to this employee appear here."
            />
          ) : (
            <Table
              getRowKey={(d: Document) => d.id}
              columns={[
                { key: 'title', header: 'Title', cell: (d: Document) => d.title },
                { key: 'doc_type', header: 'Type', cell: (d: Document) => d.doc_type },
                {
                  key: 'collected',
                  header: 'Collected',
                  cell: (d: Document) => (d.collected ? 'Yes' : 'No'),
                },
                {
                  key: 'expires_at',
                  header: 'Expires',
                  cell: (d: Document) => d.expires_at ?? '—',
                },
              ]}
              rows={documents}
            />
          )
        ) : null}

        {tab === 'timeline' ? (
          timelineError ? (
            <ErrorState message={timelineError} />
          ) : timelineItems.length === 0 ? (
            <EmptyState
              title="No timeline events"
              description="Onboarding, status changes, and other HR events appear here."
            />
          ) : (
            <Timeline items={timelineItems} />
          )
        ) : null}
      </Tabs>
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
  display: 'flex',
  alignItems: 'center',
  gap: '0.35rem',
  flexWrap: 'wrap',
};

const dlStyle: CSSProperties = {
  margin: 0,
  display: 'grid',
  gridTemplateColumns: '160px 1fr',
  gap: '0.6rem 1rem',
};

const dtStyle: CSSProperties = {
  fontWeight: 600,
  color: 'var(--cos-color-fg-muted)',
  fontSize: '0.8125rem',
};

const ddStyle: CSSProperties = {
  margin: 0,
  color: 'var(--cos-color-fg)',
};
