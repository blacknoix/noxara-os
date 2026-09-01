'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useCapabilities } from '../lib/capabilities';

type NavItem = {
  href: string;
  label: string;
  short: string;
  perm: string | null;
  anyPerm?: string[];
  counter?: number;
};

type NavGroup = {
  id: string;
  label: string;
  items: NavItem[];
};

const GROUPS: NavGroup[] = [
  {
    id: 'work',
    label: 'Work',
    items: [
      { href: '/', label: 'Dashboard', short: 'D', perm: 'workspace.dashboard.read' },
      { href: '/inbox', label: 'Inbox', short: 'I', perm: 'workspace.dashboard.read', counter: 0 },
      { href: '/my-work', label: 'My work', short: 'M', perm: 'operations.task.read', counter: 0 },
      {
        href: '/approvals',
        label: 'Approvals',
        short: 'A',
        perm: 'operations.approval.read',
        counter: 0,
      },
    ],
  },
  {
    id: 'sales',
    label: 'Sales',
    items: [
      { href: '/sales', label: 'Pipeline', short: 'S', perm: 'sales.deal.read' },
      { href: '/sales/deals', label: 'Deals', short: 'D', perm: 'sales.deal.read' },
      { href: '/sales/leads', label: 'Leads', short: 'L', perm: 'sales.lead.read' },
      { href: '/sales/customers', label: 'Customers', short: 'C', perm: 'sales.customer.read' },
      { href: '/sales/quotes', label: 'Quotes', short: 'Q', perm: 'sales.quote.read' },
      { href: '/sales/orders', label: 'Orders', short: 'O', perm: 'sales.order.read' },
      { href: '/sales/contracts', label: 'Contracts', short: 'N', perm: 'sales.contract.read' },
      { href: '/sales/territories', label: 'Territories', short: 'Y', perm: 'sales.territory.read' },
      { href: '/sales/reports', label: 'Reports', short: 'R', perm: 'sales.report.read' },
    ],
  },
  {
    id: 'finance',
    label: 'Finance',
    items: [
      { href: '/finance', label: 'Overview', short: 'F', perm: 'finance.invoice.read' },
      { href: '/finance/invoices', label: 'Invoices', short: 'I', perm: 'finance.invoice.read' },
      { href: '/finance/expenses', label: 'Expenses', short: 'E', perm: 'finance.expense.read' },
      {
        href: '/finance/accounts',
        label: 'Chart of accounts',
        short: 'A',
        perm: 'finance.ledger.read',
      },
      { href: '/finance/journals', label: 'Journals', short: 'J', perm: 'finance.ledger.read' },
      { href: '/finance/periods', label: 'Periods', short: 'P', perm: 'finance.period.read' },
      { href: '/finance/bank', label: 'Bank', short: 'B', perm: 'finance.bank.read' },
      {
        href: '/finance/expense-policy',
        label: 'Expense policy',
        short: 'X',
        perm: 'finance.expense_policy.manage',
      },
      {
        href: '/finance/settings',
        label: 'Depth settings',
        short: 'D',
        perm: 'finance.tax.read',
        anyPerm: ['finance.tax.read', 'finance.dunning.read', 'finance.entity.read'],
      },
      { href: '/finance/reports', label: 'Reports', short: 'R', perm: 'finance.report.read' },
    ],
  },
  {
    id: 'ops',
    label: 'Ops',
    items: [
      { href: '/ops/projects', label: 'Projects', short: 'P', perm: 'operations.project.read' },
      { href: '/ops/tasks', label: 'Tasks', short: 'T', perm: 'operations.task.read' },
      {
        href: '/ops/timesheets',
        label: 'Timesheets',
        short: 'H',
        perm: 'operations.timesheet.read',
      },
      {
        href: '/ops/capacity',
        label: 'Capacity',
        short: 'C',
        perm: 'operations.capacity.read',
      },
      {
        href: '/workflows',
        label: 'Workflows',
        short: 'W',
        perm: 'operations.workflow.read',
      },
    ],
  },
  {
    id: 'inventory',
    label: 'Inventory',
    items: [
      { href: '/inventory/items', label: 'Items', short: 'I', perm: 'inventory.item.read' },
      {
        href: '/inventory/warehouses',
        label: 'Warehouses',
        short: 'W',
        perm: 'inventory.warehouse.read',
      },
      { href: '/inventory/stock', label: 'Stock', short: 'K', perm: 'inventory.stock.read' },
      {
        href: '/inventory/suppliers',
        label: 'Suppliers',
        short: 'U',
        perm: 'inventory.supplier.read',
      },
      {
        href: '/inventory/purchase-requests',
        label: 'Purchase requests',
        short: 'R',
        perm: 'inventory.purchase_request.read',
      },
      {
        href: '/inventory/purchase-orders',
        label: 'Purchase orders',
        short: 'O',
        perm: 'inventory.purchase_order.read',
      },
      {
        href: '/inventory/goods-receipts',
        label: 'Goods receipts',
        short: 'G',
        perm: 'inventory.goods_receipt.read',
      },
      { href: '/inventory/assets', label: 'Assets', short: 'A', perm: 'inventory.asset.read' },
    ],
  },
  {
    id: 'people',
    label: 'People',
    items: [
      { href: '/people', label: 'Directory', short: 'E', perm: 'hr.employee.read' },
      { href: '/people/me', label: 'My profile', short: 'Y', perm: null },
      { href: '/people/attendance', label: 'Attendance', short: 'A', perm: 'hr.attendance.read' },
      { href: '/people/leave', label: 'My leave', short: 'L', perm: 'hr.leave.read' },
      { href: '/people/leave/calendar', label: 'Team leave', short: 'C', perm: 'hr.leave.read' },
      { href: '/people/leave/balances', label: 'Balances', short: 'B', perm: 'hr.leave.read' },
      { href: '/people/leave/types', label: 'Leave types', short: 'T', perm: 'hr.leave.write' },
      { href: '/people/schedules', label: 'Schedules', short: 'S', perm: 'hr.attendance.read' },
      { href: '/people/payroll', label: 'Payroll', short: 'P', perm: 'hr.payroll.read' },
      { href: '/people/me/payslips', label: 'My payslips', short: 'W', perm: null },
    ],
  },
  {
    id: 'insights',
    label: 'Insights',
    items: [
      {
        href: '/insights',
        label: 'Benchmarks & trends',
        short: 'B',
        perm: null,
        anyPerm: ['analytics.dashboard.read', 'analytics.report.read'],
      },
      {
        href: '/insights/reports',
        label: 'Reports',
        short: 'R',
        perm: 'analytics.report.read',
      },
      {
        href: '/insights/dashboards',
        label: 'Dashboards',
        short: 'D',
        perm: 'analytics.dashboard.read',
      },
      {
        href: '/insights/forecasts',
        label: 'Forecasts',
        short: 'F',
        perm: 'analytics.report.run',
      },
    ],
  },
  {
    id: 'settings',
    label: 'Settings',
    items: [
      { href: '/settings', label: 'Organization', short: 'G', perm: 'workspace.org.read' },
      {
        href: '/settings/security',
        label: 'Security',
        short: 'Z',
        perm: 'admin.access_review.read',
      },
      {
        href: '/marketplace',
        label: 'Marketplace',
        short: 'M',
        perm: 'admin.marketplace.read',
      },
      {
        href: '/settings/integrations',
        label: 'Integrations',
        short: 'I',
        perm: 'admin.marketplace.read',
      },
      {
        href: '/settings/custom',
        label: 'Custom apps',
        short: 'C',
        perm: 'custom.builder.read',
      },
      {
        href: '/settings/industry-packs',
        label: 'Industry packs',
        short: 'Y',
        perm: 'custom.builder.read',
      },
      { href: '/developers', label: 'Developers', short: 'D', perm: null },
      { href: '/members', label: 'Members', short: 'P', perm: 'workspace.member.read' },
    ],
  },
];

export function Sidebar({
  collapsed,
  onNavigate,
}: {
  collapsed: boolean;
  onNavigate?: () => void;
}) {
  const pathname = usePathname();
  const { can, loading } = useCapabilities();

  const visibleGroups = GROUPS.map((group) => ({
    ...group,
    items: group.items.filter((item) => {
      if (item.anyPerm) {
        return !loading && item.anyPerm.some((permission) => can(permission));
      }
      if (!item.perm) return true;
      if (loading) {
        return (
          item.href === '/' ||
          item.href === '/settings' ||
          item.href === '/members' ||
          item.href === '/inbox' ||
          item.href === '/my-work' ||
          item.href === '/approvals'
        );
      }
      return can(item.perm);
    }),
  })).filter((g) => g.items.length > 0);

  return (
    <aside
      aria-label="Primary"
      style={{
        borderRight: '1px solid var(--cos-color-border)',
        background: 'var(--cos-color-sidebar)',
        padding: collapsed ? '0.75rem 0.4rem' : '1rem 0.75rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '0.75rem',
        height: '100%',
        overflow: 'auto',
      }}
    >
      {visibleGroups.map((group) => (
        <div key={group.id} style={{ display: 'flex', flexDirection: 'column', gap: '0.2rem' }}>
          {!collapsed ? (
            <div
              style={{
                fontSize: '0.68rem',
                fontWeight: 700,
                letterSpacing: '0.06em',
                textTransform: 'uppercase',
                color: 'var(--cos-color-fg-muted)',
                padding: '0.25rem 0.75rem',
              }}
            >
              {group.label}
            </div>
          ) : null}
          {group.items.map((item) => {
            const hasNestedSiblings = group.items.some(
              (other) => other.href !== item.href && other.href.startsWith(`${item.href}/`),
            );
            const active =
              item.href === '/' || hasNestedSiblings
                ? pathname === item.href
                : Boolean(pathname?.startsWith(item.href));
            return (
              <Link
                key={item.href}
                href={item.href}
                title={item.label}
                onClick={() => onNavigate?.()}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: collapsed ? 'center' : 'space-between',
                  gap: '0.5rem',
                  padding: collapsed ? '0.55rem 0' : '0.55rem 0.75rem',
                  borderRadius: 'var(--cos-radius-sm)',
                  background: active ? 'var(--cos-color-bg-elevated)' : 'transparent',
                  color: active ? 'var(--cos-color-fg)' : 'var(--cos-color-fg-muted)',
                  fontWeight: active ? 600 : 500,
                  fontSize: '0.92rem',
                }}
              >
                {collapsed ? (
                  <span aria-hidden style={{ fontWeight: 700, fontSize: '0.85rem' }}>
                    {item.short}
                  </span>
                ) : (
                  <>
                    <span>{item.label}</span>
                    {typeof item.counter === 'number' ? (
                      <span
                        style={{
                          fontSize: '0.72rem',
                          fontVariantNumeric: 'tabular-nums',
                          color: 'var(--cos-color-fg-muted)',
                          minWidth: '1.25rem',
                          textAlign: 'right',
                        }}
                      >
                        {item.counter}
                      </span>
                    ) : null}
                  </>
                )}
              </Link>
            );
          })}
        </div>
      ))}
    </aside>
  );
}
