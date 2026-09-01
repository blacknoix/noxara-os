import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import axe from 'axe-core';
import {
  Chart,
  EmptyState,
  Input,
  InlineAlert,
  KanbanBoard,
  MoneyCell,
  StatusCell,
  Table,
  Tabs,
  Textarea,
  Timeline,
  Widget,
} from '@companyos/design-system';

async function expectNoSeriousAxeViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: {
      type: 'tag',
      values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'],
    },
  });
  const serious = results.violations.filter(
    (v) => v.impact === 'serious' || v.impact === 'critical',
  );
  expect(serious, JSON.stringify(serious, null, 2)).toEqual([]);
}

describe('a11y', () => {
  it('shell landmark structure has no serious/critical violations', async () => {
    const { container } = render(
      <div>
        <a href="#main-content">Skip to content</a>
        <header>
          <nav aria-label="Organization">Org switcher</nav>
        </header>
        <div style={{ display: 'flex' }}>
          <aside aria-label="Primary">
            <nav>
              <a href="/">Dashboard</a>
            </nav>
          </aside>
          <main id="main-content" tabIndex={-1}>
            <h1>Dashboard</h1>
          </main>
          <aside aria-label="Copilot">
            <h2>Copilot</h2>
          </aside>
        </div>
      </div>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('login form structure has no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Sign in</h1>
        <form
          onSubmit={(e) => {
            e.preventDefault();
          }}
        >
          <Input label="Email" type="email" name="email" autoComplete="username" />
          <Input label="Password" type="password" name="password" autoComplete="current-password" />
          <button type="submit">Sign in</button>
        </form>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('dashboard empty widgets have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Dashboard</h1>
        <Widget
          title="My work"
          empty={<EmptyState title="Coming later" description="No placeholder records." />}
          footer="As of now"
        />
        <Widget
          title="Inbox"
          empty={
            <EmptyState
              title="Nothing here yet"
              description="When activity exists, it will show up here."
            />
          }
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('members table with mock rows has no serious/critical violations', async () => {
    const rows = [
      { id: '1', name: 'Ada Lovelace', email: 'ada@example.com', status: 'active' },
      { id: '2', name: 'Grace Hopper', email: 'grace@example.com', status: 'invited' },
    ];
    const { container } = render(
      <main>
        <h1>Members</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r) => r.name },
            { key: 'email', header: 'Email', cell: (r) => r.email },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
          ]}
          rows={rows}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('people directory table has no serious/critical violations', async () => {
    const rows = [
      {
        id: 'emp_1',
        name: 'Ada Lovelace',
        title: 'Engineer',
        status: 'active',
        department_id: 'dep_eng',
        start_date: '2024-01-15',
      },
      {
        id: 'emp_2',
        name: 'Grace Hopper',
        title: 'Director',
        status: 'onboarding',
        department_id: 'dep_ops',
        start_date: '2026-09-01',
      },
    ];
    const { container } = render(
      <main>
        <h1>People</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r) => r.name },
            { key: 'title', header: 'Title', cell: (r) => r.title },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            { key: 'department_id', header: 'Department', cell: (r) => r.department_id },
            { key: 'start_date', header: 'Start date', cell: (r) => r.start_date },
          ]}
          rows={rows}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('people attendance and leave landmarks have no serious/critical violations', async () => {
    const attendance = [
      {
        id: 'att_1',
        local_date: '2026-03-02',
        entry_kind: 'check_in',
        recorded_at: '2026-03-02T09:00:00Z',
        source: 'manual',
      },
    ];
    const leave = [
      {
        id: 'lvr_1',
        start_date: '2026-03-10',
        end_date: '2026-03-12',
        status: 'approved',
        units_days: '3',
      },
    ];
    const { container } = render(
      <main>
        <h1>Attendance</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'local_date', header: 'Date', cell: (r) => r.local_date },
            {
              key: 'entry_kind',
              header: 'Kind',
              cell: (r) => <StatusCell status={r.entry_kind} />,
            },
            { key: 'source', header: 'Source', cell: (r) => r.source },
          ]}
          rows={attendance}
        />
        <h2>My leave</h2>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'start_date', header: 'Start', cell: (r) => r.start_date },
            { key: 'end_date', header: 'End', cell: (r) => r.end_date },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            { key: 'units_days', header: 'Days', cell: (r) => r.units_days },
          ]}
          rows={leave}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('people payroll runs table has no serious/critical violations', async () => {
    const rows = [
      {
        id: 'prun_1',
        period_start: '2026-03-01',
        period_end: '2026-03-31',
        status: 'calculated',
        employee_count: 12,
        net_minor: 5400000,
        currency: 'USD',
        adjustment_of_run_id: null,
      },
      {
        id: 'prun_2',
        period_start: '2026-02-01',
        period_end: '2026-02-28',
        status: 'paid',
        employee_count: 11,
        net_minor: 5100000,
        currency: 'USD',
        adjustment_of_run_id: 'prun_1',
      },
    ];
    const { container } = render(
      <main>
        <h1>Payroll</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            {
              key: 'period',
              header: 'Period',
              cell: (r) => `${r.period_start} → ${r.period_end}`,
            },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            { key: 'employees', header: 'Employees', cell: (r) => String(r.employee_count) },
            {
              key: 'net',
              header: 'Net',
              align: 'right',
              cell: (r) => <MoneyCell amount={r.net_minor} currency={r.currency} />,
            },
            {
              key: 'adj',
              header: 'Adjustment',
              cell: (r) => (r.adjustment_of_run_id ? 'Yes' : '—'),
            },
          ]}
          rows={rows}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('sales pipeline board (kanban columns) has no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Pipeline</h1>
        <KanbanBoard
          columns={[
            {
              id: 'stg_new',
              title: 'New',
              cards: [
                {
                  id: 'dl_1',
                  title: 'Acme Corp expansion',
                  meta: <MoneyCell amount={12000} currency="USD" />,
                },
              ],
            },
            {
              id: 'stg_qualified',
              title: 'Qualified',
              cards: [
                {
                  id: 'dl_2',
                  title: 'Globex renewal',
                  meta: <MoneyCell amount={4500} currency="USD" />,
                },
              ],
            },
            { id: 'stg_won', title: 'Won', cards: [] },
          ]}
          onCardSelect={() => {}}
          onCardMove={() => {}}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('customer record with tabs and timeline has no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Acme Corp</h1>
        <Tabs
          items={[
            { id: 'overview', label: 'Overview' },
            { id: 'timeline', label: 'Timeline' },
            { id: 'deals', label: 'Deals' },
            { id: 'quotes', label: 'Quotes' },
          ]}
          value="timeline"
          onChange={() => {}}
        >
          <Timeline
            items={[
              {
                id: 'act_1',
                title: 'Discovery call',
                description: 'Discussed renewal scope.',
                timestamp: 'Aug 20, 2026',
              },
              {
                id: 'act_2',
                title: 'Sent quote',
                description: 'Quote Q-ABC123 sent for review.',
                timestamp: 'Aug 22, 2026',
              },
            ]}
          />
        </Tabs>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('finance invoices table has no serious/critical violations', async () => {
    const rows = [
      { id: 'inv_1', number: 'INV-2026-000001', status: 'issued', total: 120 },
      { id: 'inv_2', number: 'Draft', status: 'draft', total: 50 },
    ];
    const { container } = render(
      <main>
        <h1>Invoices</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'number', header: 'Number', cell: (r) => r.number },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} tone="info" />,
            },
            {
              key: 'total',
              header: 'Total',
              align: 'right',
              cell: (r) => <MoneyCell amount={r.total} currency="USD" />,
            },
          ]}
          rows={rows}
        />
        <Widget title="Revenue" footer="As of now">
          <MoneyCell amount={120} currency="USD" />
        </Widget>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('finance CoA, journals, and periods landmarks have no serious/critical violations', async () => {
    const accounts = [
      { id: 'acc_1', code: '1000', name: 'Cash', account_type: 'asset', is_active: true },
      { id: 'acc_2', code: '4000', name: 'Revenue', account_type: 'revenue', is_active: true },
    ];
    const journals = [
      {
        id: 'jrn_1',
        entry_date: '2026-03-01',
        memo: 'Manual entry',
        source_type: 'manual',
        debit_minor: 1000,
        currency: 'USD',
      },
    ];
    const periods = [
      {
        id: 'fp_1',
        code: '2026-03',
        name: 'March 2026',
        status: 'open',
        start_date: '2026-03-01',
        end_date: '2026-03-31',
      },
    ];
    const { container } = render(
      <main>
        <h1>Chart of accounts</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'code', header: 'Code', cell: (r) => r.code },
            { key: 'name', header: 'Name', cell: (r) => r.name },
            { key: 'type', header: 'Type', cell: (r) => r.account_type },
            {
              key: 'active',
              header: 'Active',
              cell: (r) => <StatusCell status={r.is_active ? 'active' : 'inactive'} />,
            },
          ]}
          rows={accounts}
        />
        <h2>Journals</h2>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'entry_date', header: 'Date', cell: (r) => r.entry_date },
            { key: 'memo', header: 'Memo', cell: (r) => r.memo },
            {
              key: 'source',
              header: 'Source',
              cell: (r) => <StatusCell status={r.source_type} />,
            },
            {
              key: 'amount',
              header: 'Debits',
              align: 'right',
              cell: (r) => <MoneyCell amount={r.debit_minor / 100} currency={r.currency} />,
            },
          ]}
          rows={journals}
        />
        <h2>Periods</h2>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'code', header: 'Code', cell: (r) => r.code },
            { key: 'name', header: 'Name', cell: (r) => r.name },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
            {
              key: 'range',
              header: 'Range',
              cell: (r) => `${r.start_date} → ${r.end_date}`,
            },
          ]}
          rows={periods}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('projects/tasks ops landmark has no serious/critical violations', async () => {
    const rows = [
      { id: 'prj_1', name: 'Acme rollout', status: 'active' },
      { id: 'prj_2', name: 'Internal tooling', status: 'on_hold' },
    ];
    const { container } = render(
      <main>
        <h1>Projects</h1>
        <Table
          getRowKey={(r) => r.id}
          columns={[
            { key: 'name', header: 'Name', cell: (r) => r.name },
            {
              key: 'status',
              header: 'Status',
              cell: (r) => <StatusCell status={r.status} />,
            },
          ]}
          rows={rows}
        />
        <h2>Tasks board</h2>
        <KanbanBoard
          columns={[
            {
              id: 'backlog',
              title: 'Backlog',
              cards: [{ id: 'tsk_1', title: 'Draft kickoff checklist' }],
            },
            {
              id: 'todo',
              title: 'To do',
              cards: [{ id: 'tsk_2', title: 'Schedule kickoff' }],
            },
            { id: 'in_progress', title: 'In progress', cards: [] },
            { id: 'in_review', title: 'In review', cards: [] },
            { id: 'done', title: 'Done', cards: [] },
          ]}
          onCardSelect={() => {}}
          onCardMove={() => {}}
        />
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('approvals inbox landmark has no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Approvals</h1>
        <p>Pending items assigned to you.</p>
        <label htmlFor="approval-comment">Comment</label>
        <input id="approval-comment" name="comment" />
        <ul>
          <li>
            <input type="checkbox" aria-label="Select Expense: Travel" />
            <strong>Expense: Travel</strong>
            <div aria-label="Routing rationale">
              Routed by policy Default expense approval v1 (any)
            </div>
            <button type="button">Approve</button>
            <button type="button">Reject</button>
          </li>
        </ul>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('copilot panel structure has no serious/critical violations', async () => {
    const { container } = render(
      <div style={{ display: 'flex', height: 480 }}>
        <main style={{ flex: 1 }}>
          <h1>Dashboard</h1>
        </main>
        <aside aria-label="Copilot" style={{ width: 380, borderLeft: '1px solid #ccc' }}>
          <header>
            <h2>Copilot</h2>
          </header>
          <div role="log" aria-live="polite">
            <p>Ask anything about your workspace.</p>
          </div>
          <form>
            <Textarea label="Message Copilot" name="copilot-message" />
            <button type="submit">Send</button>
          </form>
        </aside>
      </div>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('AI settings form structure has no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>AI settings</h1>
        <form>
          <fieldset>
            <legend>Modules</legend>
            <label>
              <input type="checkbox" defaultChecked /> Copilot
            </label>
            <label>
              <input type="checkbox" defaultChecked /> Insights
            </label>
          </fieldset>
          <Input label="Model preference" name="model" defaultValue="mock" />
          <Input label="Monthly token budget" name="budget" type="number" defaultValue="100000" />
          <button type="submit">Save AI settings</button>
        </form>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('AI agents settings landmarks have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Agents</h1>
        <p>Governed autonomous agents — scoped, budgeted, reversible, audited.</p>
        <section aria-labelledby="kill-heading">
          <h2 id="kill-heading">Kill switch</h2>
          <label>
            <input type="checkbox" /> Org-wide kill switch
          </label>
        </section>
        <section aria-labelledby="policy-heading">
          <h2 id="policy-heading">Agent policy</h2>
          <form>
            <Input label="Name" name="policy-name" defaultValue="default" />
            <Input label="Allowed tools" name="allowed-tools" />
            <button type="submit">Publish policy version</button>
          </form>
        </section>
        <section aria-labelledby="monitor-heading">
          <h2 id="monitor-heading">Monitor</h2>
          <EmptyState title="No agent runs yet" description="Publish a policy, then start a run." />
        </section>
        <section aria-labelledby="nl-heading">
          <h2 id="nl-heading">Natural-language workflow</h2>
          <Textarea label="Describe the workflow" name="nl-prompt" />
        </section>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('workflows builder landmarks have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Workflows</h1>
        <p>Configure event-driven automations. Definitions are data — not code.</p>
        <section aria-labelledby="fixtures-heading">
          <h2 id="fixtures-heading">Start from a fixture</h2>
          <button type="button">Deal won → task</button>
        </section>
        <section aria-labelledby="list-heading">
          <h2 id="list-heading">Definitions</h2>
          <EmptyState
            title="No workflows yet"
            description="Create a definition or start from a fixture."
          />
        </section>
        <form>
          <Input label="Name" name="name" />
          <Textarea label="Description" name="description" />
          <label>
            Trigger
            <select name="trigger" aria-label="Trigger">
              <option value="manual">Manual / API start</option>
              <option value="sales.deal.won">sales.deal.won</option>
            </select>
          </label>
          <button type="submit">Save draft</button>
        </form>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('insights analytics landmarks have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <header>
          <h1>Benchmarks & trends</h1>
          <p>Flagship company metrics derived from the governed event stream.</p>
        </header>
        <InlineAlert tone="info" title="Eventually consistent">
          Recent operational changes may take a short time to appear.
        </InlineAlert>
        <section aria-labelledby="flagship-metrics-test-heading">
          <h2 id="flagship-metrics-test-heading">Flagship metrics</h2>
          <Chart title="Issued revenue" description="Current seven-day window">
            <div role="img" aria-label="Issued revenue bar chart">
              <div style={{ width: '75%', height: 12, background: 'currentColor' }} />
            </div>
          </Chart>
        </section>
        <section aria-labelledby="report-results-test-heading">
          <h2 id="report-results-test-heading">Report results</h2>
          <Table
            getRowKey={(row) => row.id}
            rows={[{ id: 'row_1', dimension: 'USD', value: 2500 }]}
            columns={[
              { key: 'dimension', header: 'Currency', cell: (row) => row.dimension },
              { key: 'value', header: 'Value', cell: (row) => String(row.value) },
            ]}
          />
        </section>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('workflow monitor landmarks have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Workflow monitor</h1>
        <section aria-labelledby="summary-heading">
          <h2 id="summary-heading">Summary</h2>
          <dl>
            <div>
              <dt>Running</dt>
              <dd>0</dd>
            </div>
            <div>
              <dt>Waiting</dt>
              <dd>1</dd>
            </div>
            <div>
              <dt>Failed</dt>
              <dd>0</dd>
            </div>
            <div>
              <dt>SLA breached</dt>
              <dd>0</dd>
            </div>
          </dl>
        </section>
        <section aria-labelledby="active-heading">
          <h2 id="active-heading">Active & failed</h2>
          <EmptyState title="Nothing to show" description="No running instances." />
        </section>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('custom apps builder landmarks have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Custom apps</h1>
        <p>Define tenant-scoped entities and publish permissions.</p>
        <section aria-labelledby="entities-heading">
          <h2 id="entities-heading">Entities</h2>
          <EmptyState title="No custom entities" description="Create a draft entity to get started." />
        </section>
        <form>
          <Input label="Slug" name="slug" />
          <button type="submit">Create draft</button>
        </form>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });

  it('industry packs and offline conflict UI have no serious/critical violations', async () => {
    const { container } = render(
      <main>
        <h1>Industry packs</h1>
        <p>Vertical configuration packs — install is data only.</p>
        <InlineAlert tone="danger" title="Sync conflict">
          Stale If-Match was rejected. Last-write-wins with version — loser not silently dropped.
        </InlineAlert>
        <InlineAlert tone="warning" title="Install restricted">
          Member cannot install org-wide packs without custom.package.import.
        </InlineAlert>
      </main>,
    );
    await expectNoSeriousAxeViolations(container);
  });
});
