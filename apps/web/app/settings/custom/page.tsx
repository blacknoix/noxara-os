'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import {
  createEntity,
  deleteEntity,
  exportPackage,
  importPackage,
  listEntities,
  publishEntity,
  type EntityDefinition,
} from '../../../lib/custom-api';
import { useCapabilities } from '../../../lib/capabilities';

export default function CustomBuilderPage() {
  const { can, loading } = useCapabilities();
  const canRead = can('custom.builder.read');
  const canManage = can('custom.builder.manage');
  const [entities, setEntities] = useState<EntityDefinition[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [slug, setSlug] = useState('');
  const [label, setLabel] = useState('');
  const [fieldName, setFieldName] = useState('title');
  const [confirmDelete, setConfirmDelete] = useState<Record<string, string>>({});
  const [importJson, setImportJson] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      setEntities(await listEntities());
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load entities');
    }
  }, []);

  useEffect(() => {
    if (!loading && canRead) void refresh();
  }, [loading, canRead, refresh]);

  if (loading) return <p>Loading…</p>;
  if (!canRead) {
    return (
      <PermissionDeniedState
        requiredPermission="custom.builder.read"
        message="Member cannot define entities unless explicitly granted custom.builder.manage."
      />
    );
  }

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    if (!canManage) return;
    setBusy(true);
    setError(null);
    const created = await createEntity({
      slug: slug.trim(),
      label: label.trim() || slug.trim(),
      fields: [{ name: fieldName.trim() || 'title', label: 'Title', type: 'text', required: true }],
    });
    setBusy(false);
    if (!created) {
      setError('Could not create entity (check slug and permissions).');
      return;
    }
    setSlug('');
    setLabel('');
    await refresh();
  }

  return (
    <main id="main-content" style={{ padding: '1.5rem', maxWidth: 960 }}>
      <h1 style={{ fontSize: '1.5rem', marginBottom: '0.25rem' }}>Custom apps</h1>
      <p style={{ color: 'var(--cos-color-fg-muted)', marginBottom: '1.5rem' }}>
        Define tenant-scoped entities, publish permissions, and move packages between environments.
      </p>

      {error ? (
        <InlineAlert tone="danger" title="Error">
          {error}
        </InlineAlert>
      ) : null}

      {canManage ? (
        <form
          onSubmit={onCreate}
          style={{ display: 'grid', gap: '0.75rem', marginBottom: '2rem', maxWidth: 480 }}
        >
          <h2 style={{ fontSize: '1.1rem' }}>New entity</h2>
          <Input
            label="Slug"
            value={slug}
            onChange={(e) => setSlug(e.target.value)}
            placeholder="widget"
            required
          />
          <Input
            label="Label"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Widget"
          />
          <Input
            label="First field name"
            value={fieldName}
            onChange={(e) => setFieldName(e.target.value)}
          />
          <Button type="submit" disabled={busy}>
            Create draft
          </Button>
        </form>
      ) : (
        <InlineAlert tone="info" title="Read only">
          Defining entities requires custom.builder.manage (Owner/Admin by default; Member denied).
        </InlineAlert>
      )}

      <h2 style={{ fontSize: '1.1rem', marginBottom: '0.75rem' }}>Entities</h2>
      {entities.length === 0 ? (
        <EmptyState title="No custom entities" description="Create a draft entity to get started." />
      ) : (
        <Table
          getRowKey={(row) => row.id}
          columns={[
            { key: 'label', header: 'Label', cell: (row) => row.label },
            {
              key: 'slug',
              header: 'Slug',
              cell: (row) => (
                <Link href={`/custom/${row.slug}`} style={{ textDecoration: 'underline' }}>
                  {row.slug}
                </Link>
              ),
            },
            {
              key: 'status',
              header: 'Status',
              cell: (row) => `${row.status} v${row.published_version}`,
            },
          ]}
          rows={entities}
          rowActions={
            canManage
              ? (ent) => (
                  <div
                    style={{
                      display: 'flex',
                      gap: '0.5rem',
                      flexWrap: 'wrap',
                      alignItems: 'center',
                    }}
                  >
                    {ent.status !== 'published' ? (
                      <Button
                        size="sm"
                        type="button"
                        onClick={async () => {
                          const ok = await publishEntity(ent.id);
                          if (!ok) setError('Publish failed');
                          await refresh();
                        }}
                      >
                        Publish
                      </Button>
                    ) : null}
                    <Input
                      aria-label={`Type ${ent.slug} to confirm delete`}
                      placeholder={`Type ${ent.slug} to delete`}
                      value={confirmDelete[ent.id] ?? ''}
                      onChange={(e) =>
                        setConfirmDelete((m) => ({ ...m, [ent.id]: e.target.value }))
                      }
                    />
                    <Button
                      size="sm"
                      type="button"
                      variant="danger"
                      disabled={(confirmDelete[ent.id] ?? '') !== ent.slug}
                      onClick={async () => {
                        const ok = await deleteEntity(ent.id, ent.slug);
                        if (!ok) setError('Delete failed — type the slug to confirm');
                        await refresh();
                      }}
                    >
                      Delete
                    </Button>
                  </div>
                )
              : undefined
          }
        />
      )}

      {can('custom.package.export') || can('custom.package.import') ? (
        <section style={{ marginTop: '2rem' }}>
          <h2 style={{ fontSize: '1.1rem' }}>Packages</h2>
          <div style={{ display: 'flex', gap: '0.75rem', flexWrap: 'wrap', marginTop: '0.75rem' }}>
            {can('custom.package.export') ? (
              <Button
                type="button"
                onClick={async () => {
                  const pkg = await exportPackage();
                  if (!pkg) {
                    setError('Export failed');
                    return;
                  }
                  setImportJson(JSON.stringify(pkg, null, 2));
                }}
              >
                Export package
              </Button>
            ) : null}
          </div>
          {can('custom.package.import') ? (
            <form
              style={{ marginTop: '1rem', display: 'grid', gap: '0.5rem' }}
              onSubmit={async (e) => {
                e.preventDefault();
                try {
                  const pkg = JSON.parse(importJson) as unknown;
                  const ok = await importPackage(pkg);
                  if (!ok) setError('Import failed');
                  else {
                    setError(null);
                    await refresh();
                  }
                } catch {
                  setError('Invalid package JSON');
                }
              }}
            >
              <label htmlFor="pkg-json">Import package JSON</label>
              <textarea
                id="pkg-json"
                value={importJson}
                onChange={(e) => setImportJson(e.target.value)}
                rows={8}
                style={{ fontFamily: 'monospace', width: '100%' }}
              />
              <Button type="submit">Import (additive)</Button>
            </form>
          ) : null}
        </section>
      ) : null}
    </main>
  );
}
