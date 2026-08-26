import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import axe from 'axe-core';
import {
  EmptyState,
  Input,
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
          empty={<EmptyState title="Nothing here yet" description="When activity exists, it will show up here." />}
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
            { key: 'status', header: 'Status', cell: (r) => <StatusCell status={r.status} tone="info" /> },
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
});
