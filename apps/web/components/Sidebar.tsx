'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';

const NAV = [
  { href: '/', label: 'Work' },
  { href: '/sales', label: 'Sales' },
  { href: '/finance', label: 'Finance' },
  { href: '/ops', label: 'Ops' },
  { href: '/insights', label: 'Insights' },
  { href: '/settings', label: 'Settings' },
];

export function Sidebar() {
  const pathname = usePathname();
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
      {NAV.map((item, index) => {
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
