'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import { createRecord, listRecords, type CustomRecord } from '../../../lib/custom-api';
import { useCapabilities } from '../../../lib/capabilities';

export default function CustomRecordsPage() {
  const params = useParams<{ slug: string }>();
  const slug = params.slug;
  const { can, loading } = useCapabilities();
  const canRead = can(`custom.${slug}.read`) || can('custom.builder.read');
  const canWrite = can(`custom.${slug}.write`) || can('custom.builder.manage');
  const [rows, setRows] = useState<CustomRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState('');

  const refresh = useCallback(async () => {
    setError(null);
    setRows(await listRecords(slug));
  }, [slug]);

  useEffect(() => {
    if (!loading && canRead) void refresh();
  }, [loading, canRead, refresh]);

  if (loading) return <p>Loading…</p>;
  if (!canRead) {
    return <PermissionDeniedState requiredPermission={`custom.${slug}.read`} />;
  }

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    if (!canWrite) return;
    const rec = await createRecord(slug, { title: title.trim() });
    if (!rec) {
      setError('Create failed');
      return;
    }
    setTitle('');
    await refresh();
  }

  return (
    <main id="main-content" style={{ padding: '1.5rem', maxWidth: 960 }}>
      <p style={{ marginBottom: '0.5rem' }}>
        <Link href="/settings/custom">← Custom apps</Link>
      </p>
      <h1 style={{ fontSize: '1.5rem' }}>{slug}</h1>
      <p style={{ color: 'var(--cos-color-fg-muted)' }}>Records for this custom entity.</p>

      {error ? (
        <InlineAlert tone="danger" title="Error">
          {error}
        </InlineAlert>
      ) : null}

      {canWrite ? (
        <form
          onSubmit={onCreate}
          style={{ display: 'flex', gap: '0.75rem', alignItems: 'end', margin: '1rem 0' }}
        >
          <Input label="title" value={title} onChange={(e) => setTitle(e.target.value)} required />
          <Button type="submit">Add record</Button>
        </form>
      ) : null}

      {rows.length === 0 ? (
        <EmptyState title="No records" description="Create a record to populate this list." />
      ) : (
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'id', header: 'ID', cell: (r) => r.id },
            {
              key: 'values',
              header: 'Values',
              cell: (r) => <code>{JSON.stringify(r.values)}</code>,
            },
            { key: 'updated', header: 'Updated', cell: (r) => r.updated_at },
          ]}
          rows={rows}
        />
      )}
    </main>
  );
}
