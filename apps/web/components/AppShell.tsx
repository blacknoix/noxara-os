'use client';

import type { ReactNode } from 'react';
import { useEffect, useState } from 'react';
import { TopBar } from './TopBar';
import { Sidebar } from './Sidebar';
import { ContextPanel } from './ContextPanel';

export function AppShell({ children }: { children: ReactNode }) {
  const [panelOpen, setPanelOpen] = useState(true);
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        gridTemplateRows: '56px 1fr',
        opacity: mounted ? 1 : 0,
        transform: mounted ? 'none' : 'translateY(4px)',
        transition: 'opacity 280ms ease, transform 320ms ease',
      }}
    >
      <TopBar onTogglePanel={() => setPanelOpen((v) => !v)} />
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: panelOpen ? '220px 1fr 300px' : '220px 1fr 0px',
          minHeight: 0,
          transition: 'grid-template-columns 240ms ease',
        }}
      >
        <Sidebar />
        <main
          style={{
            padding: '1.5rem 1.75rem',
            minWidth: 0,
            overflow: 'auto',
          }}
        >
          {children}
        </main>
        <ContextPanel open={panelOpen} />
      </div>
    </div>
  );
}
