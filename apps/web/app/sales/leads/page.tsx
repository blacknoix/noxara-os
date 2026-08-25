'use client';

import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  StatusCell,
  Table,
  useToast,
} from '@companyos/design-system';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

type Lead = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  company_name: string | null;
  source: string | null;
  status: string;
  score: number;
  converted_customer_id: string | null;
  converted_deal_id: string | null;
};

function statusTone(status: string): 'success' | 'warning' | 'danger' | 'neutral' | 'info' {
  switch (status) {
    case 'qualified':
      return 'info';
    case 'converted':
      return 'success';
    case 'disqualified':
      return 'danger';
    default:
      return 'neutral';
  }
}

export default function LeadsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const { toast } = useToast();

  const [leads, setLeads] = useState<Lead[]>([]);
  const [loading, setLoading] = useState(true);
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<string | undefined>();
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setLoading(false);
      setError('Sign in required');
      return;
    }
    setLoading(true);
    setError(null);
    setDenied(false);
    const res = await authFetch('/api/v1/sales/leads?limit=200');
    const rid = res.headers.get('x-request-id') ?? undefined;
    setRequestId(rid);
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
      setError('Could not load leads');
      setLoading(false);
      return;
    }
    const body = await res.json();
    setLeads(body.items ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const qualify = useCallback(
    async (lead: Lead) => {
      setBusyId(lead.id);
      try {
        const res = await authFetch(`/api/v1/sales/leads/${lead.id}/qualify`, { method: 'POST' });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          throw new Error(typeof body.detail === 'string' ? body.detail : 'Could not qualify lead');
        }
        await load();
        toast({ title: 'Lead qualified', description: lead.name });
      } catch (err) {
        toast({
          title: 'Could not qualify lead',
          description: err instanceof Error ? err.message : 'Please try again.',
        });
      } finally {
        setBusyId(null);
      }
    },
    [load, toast],
  );

  const convert = useCallback(
    async (lead: Lead) => {
      setBusyId(lead.id);
      try {
        const res = await authFetch(`/api/v1/sales/leads/${lead.id}/convert`, {
          method: 'POST',
          body: JSON.stringify({}),
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          const count = Array.isArray(body.matches) ? body.matches.length : undefined;
          const message =
            typeof body.detail === 'string'
              ? body.detail
              : count
                ? `Found ${count} potential duplicate customer(s). Resolve manually before converting.`
                : 'Could not convert lead';
          throw new Error(message);
        }
        await load();
        toast({ title: 'Lead converted', description: `${lead.name} is now a customer + deal.` });
      } catch (err) {
        toast({
          title: 'Could not convert lead',
          description: err instanceof Error ? err.message : 'Please try again.',
        });
      } finally {
        setBusyId(null);
      }
    },
    [load, toast],
  );

  const sorted = useMemo(() => [...leads].sort((a, b) => a.name.localeCompare(b.name)), [leads]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to view leads." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading workspace…</p>;
  }

  if (denied || (!can('sales.lead.read') && !loading)) {
    return <PermissionDeniedState requiredPermission="sales.lead.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <header>
        <p style={eyebrow}>Sales</p>
        <h1 style={h1}>Leads</h1>
        <p style={muted}>
          Qualify promising leads, then convert them into a customer and deal.{' '}
          <Link href="/sales" style={{ color: 'var(--cos-color-accent)' }}>
            View pipeline
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} requestId={requestId} /> : null}

      {loading ? (
        <LoadingState label="Loading leads" rows={4} />
      ) : sorted.length === 0 ? (
        <EmptyState title="No leads yet" description="Captured leads appear here." />
      ) : (
        <Table
          getRowKey={(l: Lead) => l.id}
          columns={[
            { key: 'name', header: 'Name', cell: (l: Lead) => l.name },
            { key: 'company', header: 'Company', cell: (l: Lead) => l.company_name ?? '—' },
            { key: 'email', header: 'Email', cell: (l: Lead) => l.email ?? '—' },
            { key: 'source', header: 'Source', cell: (l: Lead) => l.source ?? '—' },
            { key: 'score', header: 'Score', align: 'right', cell: (l: Lead) => l.score },
            {
              key: 'status',
              header: 'Status',
              cell: (l: Lead) => <StatusCell status={l.status} tone={statusTone(l.status)} />,
            },
            {
              key: 'actions',
              header: 'Actions',
              cell: (l: Lead) => (
                <span style={{ display: 'flex', gap: '0.35rem', flexWrap: 'wrap' }}>
                  {l.status === 'converted' && l.converted_customer_id ? (
                    <Link href={`/sales/customers/${l.converted_customer_id}`} style={{ color: 'var(--cos-color-accent)' }}>
                      View customer
                    </Link>
                  ) : (
                    <>
                      {can('sales.lead.update') && l.status === 'new' ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          disabled={busyId === l.id}
                          onClick={() => void qualify(l)}
                        >
                          Qualify
                        </Button>
                      ) : null}
                      {can('sales.lead.convert') && l.status !== 'disqualified' ? (
                        <Button
                          type="button"
                          size="sm"
                          disabled={busyId === l.id}
                          onClick={() => void convert(l)}
                        >
                          Convert
                        </Button>
                      ) : null}
                    </>
                  )}
                </span>
              ),
            },
          ]}
          rows={sorted}
        />
      )}
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
