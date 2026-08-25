'use client';

import { useState } from 'react';
import {
  Avatar,
  Badge,
  Button,
  EmptyState,
  ErrorState,
  FilterBar,
  type FilterClause,
  Input,
  LoadingState,
  PermissionDeniedState,
  Select,
  Skeleton,
  StatusCell,
  StaleDataState,
  Table,
  Widget,
} from '@companyos/design-system';

export default function DevComponentsPage() {
  const [filters, setFilters] = useState<FilterClause[]>([
    { id: '1', field: 'status', operator: 'is', value: 'active', label: 'Status' },
  ]);
  const [q, setQ] = useState('');

  const sampleRows = [
    { id: '1', name: 'Ada Lovelace', email: 'ada@example.com', status: 'active' },
    { id: '2', name: 'Grace Hopper', email: 'grace@example.com', status: 'invited' },
    { id: '3', name: 'Alan Turing', email: 'alan@example.com', status: 'suspended' },
  ];

  return (
    <div style={{ display: 'grid', gap: '2rem', maxWidth: 960 }}>
      <header>
        <h1
          style={{
            margin: 0,
            fontFamily: 'var(--cos-font-display)',
            fontSize: '1.75rem',
            fontWeight: 650,
          }}
        >
          Component gallery
        </h1>
        <p style={{ color: 'var(--cos-color-fg-muted)', margin: '0.4rem 0 0' }}>
          Local Storybook-equivalent for `@companyos/design-system`. No auth required.
        </p>
      </header>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Buttons</h2>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          <Button>Primary</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="danger">Danger</Button>
          <Button size="sm">Small</Button>
          <Button loading>Loading</Button>
        </div>
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Inputs</h2>
        <div style={{ display: 'grid', gap: 12, maxWidth: 360 }}>
          <Input label="Email" placeholder="you@company.com" />
          <Select
            label="Role"
            options={[
              { value: 'member', label: 'Member' },
              { value: 'admin', label: 'Admin' },
            ]}
            defaultValue="member"
          />
        </div>
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Badge, avatar, status</h2>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
          <Badge>Neutral</Badge>
          <Badge tone="success">Success</Badge>
          <Badge tone="warning">Warning</Badge>
          <Badge tone="danger">Danger</Badge>
          <Badge tone="info">Info</Badge>
          <StatusCell status="active" tone="success" />
          <Avatar name="Company OS" />
          <Avatar name="Dev User" size="sm" />
        </div>
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Filter bar</h2>
        <FilterBar
          q={q}
          onQueryChange={setQ}
          filters={filters}
          onFiltersChange={setFilters}
          onClearAll={() => {
            setQ('');
            setFilters([]);
          }}
          onSaveView={() => undefined}
        />
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Table</h2>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r) => r.name, sortable: true },
            { key: 'email', header: 'Email', cell: (r) => r.email },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
          ]}
          rows={sampleRows}
        />
      </section>

      <section style={{ display: 'grid', gap: '0.75rem' }}>
        <h2 style={h2}>Widget</h2>
        <div
          style={{
            padding: '1rem',
            border: '1px solid var(--cos-color-border)',
            borderRadius: 'var(--cos-radius-md)',
            background: 'var(--cos-color-bg-elevated)',
          }}
        >
          <Widget title="Sample widget" range="Last 30 days" footer="As of just now" menu={<Badge>Live</Badge>}>
            <p style={{ margin: 0, color: 'var(--cos-color-fg-muted)' }}>Widget body content.</p>
          </Widget>
        </div>
        <Skeleton height={16} />
      </section>

      <section style={{ display: 'grid', gap: '1rem' }}>
        <h2 style={h2}>States</h2>
        <EmptyState title="Nothing here yet" description="Honest empty — no fake records." />
        <ErrorState message="Example error" requestId="req_dev_example" />
        <PermissionDeniedState requiredPermission="workspace.dashboard.read" />
        <LoadingState label="Loading example" />
        <StaleDataState asOf={new Date().toISOString()} onRefresh={() => undefined} />
      </section>
    </div>
  );
}

const h2 = {
  margin: 0,
  fontFamily: 'var(--cos-font-display)' as const,
  fontSize: '1.15rem',
  fontWeight: 550,
};
