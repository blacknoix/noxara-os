'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useCapabilities } from '../lib/capabilities';

type NavItem = {
  href: string;
  label: string;
  short: string;
  perm: string | null;
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
      { href: '/my-work', label: 'My work', short: 'M', perm: 'workspace.dashboard.read', counter: 0 },
      {
        href: '/approvals',
        label: 'Approvals',
        short: 'A',
        perm: 'workspace.dashboard.read',
        counter: 0,
      },
    ],
  },
  {
    id: 'sales',
    label: 'Sales',
    items: [{ href: '/sales', label: 'Pipeline', short: 'S', perm: null }],
  },
  {
    id: 'finance',
    label: 'Finance',
    items: [{ href: '/finance', label: 'Finance', short: 'F', perm: null }],
  },
  {
    id: 'ops',
    label: 'Ops',
    items: [{ href: '/ops', label: 'Ops', short: 'O', perm: null }],
  },
  {
    id: 'insights',
    label: 'Insights',
    items: [{ href: '/insights', label: 'Insights', short: 'N', perm: null }],
  },
  {
    id: 'settings',
    label: 'Settings',
    items: [
      { href: '/settings', label: 'Organization', short: 'G', perm: 'workspace.org.read' },
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
            const active =
              item.href === '/'
                ? pathname === '/'
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
