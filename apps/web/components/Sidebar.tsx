'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useCapabilities } from '../lib/capabilities';

const NAV = [
  { href: '/', label: 'Work', perm: 'workspace.dashboard.read' },
  { href: '/sales', label: 'Sales', perm: null },
  { href: '/finance', label: 'Finance', perm: null },
  { href: '/ops', label: 'Ops', perm: null },
  { href: '/insights', label: 'Insights', perm: null },
  { href: '/settings', label: 'Settings', perm: 'workspace.org.read' },
];

export function Sidebar() {
  const pathname = usePathname();
  const { can, loading } = useCapabilities();

  const items = NAV.filter((item) => {
    if (!item.perm) return true; // Phase later modules stay visible as placeholders
    if (loading) return item.href === '/' || item.href === '/settings';
    return can(item.perm);
  });

  return (
    <aside
      style={{
        borderRight: '1px solid var(--cos-color-border)',
        background: 'var(--cos-color-sidebar)',
        padding: '1rem 0.75rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '0.25rem',
      }}
    >
      {items.map((item, index) => {
        const active = pathname === item.href;
        return (
          <Link
            key={item.href}
            href={item.href}
            style={{
              padding: '0.55rem 0.75rem',
              borderRadius: 'var(--cos-radius-sm)',
              background: active ? 'var(--cos-color-bg-elevated)' : 'transparent',
              color: active ? 'var(--cos-color-fg)' : 'var(--cos-color-fg-muted)',
              fontWeight: active ? 600 : 500,
              fontSize: '0.92rem',
              opacity: 0,
              animation: `cos-nav-in 320ms ease ${80 + index * 40}ms forwards`,
            }}
          >
            {item.label}
          </Link>
        );
      })}
      <style>{`
        @keyframes cos-nav-in {
          from { opacity: 0; transform: translateX(-6px); }
          to { opacity: 1; transform: none; }
        }
      `}</style>
    </aside>
  );
}
