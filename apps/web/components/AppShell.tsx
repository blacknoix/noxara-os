'use client';

import type { CSSProperties, ReactNode } from 'react';
import { useCallback, useEffect, useState } from 'react';
import { usePathname } from 'next/navigation';
import { TopBar } from './TopBar';
import { Sidebar } from './Sidebar';
import { ContextPanel } from './ContextPanel';
import { CommandBarHost } from './CommandBarHost';

const AUTH_PATHS = [
  '/login',
  '/signup',
  '/verify-email',
  '/magic-link',
  '/mfa',
  '/reset-password',
  '/onboarding',
  '/invite',
];

const SIDEBAR_KEY = 'cos-sidebar-collapsed';
const PANEL_KEY = 'cos-context-panel-open';

function readBool(key: string, fallback: boolean): boolean {
  if (typeof window === 'undefined') return fallback;
  try {
    const v = window.localStorage.getItem(key);
    if (v === null) return fallback;
    return v === '1' || v === 'true';
  } catch {
    return fallback;
  }
}

function writeBool(key: string, value: boolean) {
  try {
    window.localStorage.setItem(key, value ? '1' : '0');
  } catch {
    /* ignore */
  }
}

export function AppShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const isAuth = AUTH_PATHS.some((p) => pathname?.startsWith(p));
  const [mounted, setMounted] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarOverlayOpen, setSidebarOverlayOpen] = useState(false);
  const [panelOpen, setPanelOpen] = useState(true);
  const [isNarrow, setIsNarrow] = useState(false);
  const [isCompact, setIsCompact] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);

  useEffect(() => {
    const mqNav = window.matchMedia('(max-width: 1023px)');
    const mqPanel = window.matchMedia('(max-width: 767px)');
    const apply = () => {
      const narrow = mqNav.matches;
      const compact = mqPanel.matches;
      setIsNarrow(narrow);
      setIsCompact(compact);
      // Below ~1024 collapse sidebar by default; desktop uses persisted preference.
      if (narrow) {
        setSidebarCollapsed(true);
        setSidebarOverlayOpen(false);
      } else {
        setSidebarCollapsed(readBool(SIDEBAR_KEY, false));
      }
      if (compact) {
        setPanelOpen(false);
      } else {
        setPanelOpen(readBool(PANEL_KEY, true));
      }
    };
    apply();
    setMounted(true);
    mqNav.addEventListener('change', apply);
    mqPanel.addEventListener('change', apply);
    return () => {
      mqNav.removeEventListener('change', apply);
      mqPanel.removeEventListener('change', apply);
    };
  }, []);

  const toggleSidebar = useCallback(() => {
    if (isNarrow) {
      setSidebarOverlayOpen((v) => !v);
      return;
    }
    setSidebarCollapsed((v) => {
      const next = !v;
      writeBool(SIDEBAR_KEY, next);
      return next;
    });
  }, [isNarrow]);

  const togglePanel = useCallback(() => {
    if (isCompact) {
      setPanelOpen((v) => !v);
      return;
    }
    setPanelOpen((v) => {
      const next = !v;
      writeBool(PANEL_KEY, next);
      return next;
    });
  }, [isCompact]);

  useEffect(() => {
    setSidebarOverlayOpen(false);
  }, [pathname]);

  if (isAuth) {
    return (
      <div
        style={{
          minHeight: '100vh',
          opacity: mounted ? 1 : 0,
          transition: 'opacity 280ms ease',
        }}
      >
        {children}
      </div>
    );
  }

  const sidebarWidth = sidebarCollapsed && !isNarrow ? 64 : 220;
  const showDesktopSidebar = !isNarrow;
  const showOverlaySidebar = isNarrow && sidebarOverlayOpen;
  const panelWidth = panelOpen ? 380 : 0;

  return (
    <div
      style={
        {
          minHeight: '100vh',
          display: 'grid',
          gridTemplateRows: '56px 1fr',
          opacity: mounted ? 1 : 0,
          transform: mounted ? 'none' : 'translateY(4px)',
          transition: 'opacity 280ms ease, transform 320ms ease',
        } satisfies CSSProperties
      }
    >
      <a href="#main-content" className="cos-skip-link">
        Skip to content
      </a>
      <TopBar
        onTogglePanel={togglePanel}
        onToggleSidebar={toggleSidebar}
        onOpenCommand={() => setCommandOpen(true)}
        sidebarCollapsed={sidebarCollapsed && !isNarrow}
        panelOpen={panelOpen}
      />
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: showDesktopSidebar
            ? `${sidebarWidth}px 1fr ${panelWidth}px`
            : `1fr ${panelWidth}px`,
          minHeight: 0,
          transition: 'grid-template-columns 240ms ease',
          position: 'relative',
        }}
      >
        {showDesktopSidebar ? (
          <Sidebar collapsed={sidebarCollapsed} />
        ) : null}

        {showOverlaySidebar ? (
          <>
            <div
              className="cos-shell-overlay"
              role="presentation"
              onClick={() => setSidebarOverlayOpen(false)}
            />
            <div
              style={{
                position: 'fixed',
                top: 56,
                left: 0,
                bottom: 0,
                width: 220,
                zIndex: 50,
                boxShadow: 'var(--cos-shadow-md)',
              }}
            >
              <Sidebar collapsed={false} onNavigate={() => setSidebarOverlayOpen(false)} />
            </div>
          </>
        ) : null}

        <main
          id="main-content"
          tabIndex={-1}
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

      <CommandBarHost
        open={commandOpen}
        onOpenChange={setCommandOpen}
        onToggleSidebar={toggleSidebar}
      />
    </div>
  );
}
