'use client';

import { Button, Input } from '@companyos/design-system';

export function TopBar({ onTogglePanel }: { onTogglePanel: () => void }) {
  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '0.75rem',
        padding: '0 1rem',
        borderBottom: '1px solid var(--cos-color-border)',
        background: 'color-mix(in srgb, var(--cos-color-topbar) 92%, transparent)',
        backdropFilter: 'blur(8px)',
      }}
    >
      <div
        style={{
          fontFamily: 'var(--cos-font-display)',
          fontWeight: 650,
          fontSize: '1.15rem',
          letterSpacing: '-0.02em',
          minWidth: 140,
        }}
      >
        CompanyOS
      </div>
      <button
        type="button"
        aria-label="Organization switcher placeholder"
        style={{
          border: '1px solid var(--cos-color-border)',
          background: 'var(--cos-color-bg-elevated)',
          borderRadius: 'var(--cos-radius-sm)',
          padding: '0.35rem 0.65rem',
          color: 'var(--cos-color-fg-muted)',
          cursor: 'pointer',
        }}
      >
        Acme Demo ▾
      </button>
      <div style={{ flex: 1, maxWidth: 420 }}>
        <Input placeholder="Command bar placeholder…" aria-label="Command bar placeholder" readOnly />
      </div>
      <Button variant="secondary">Create</Button>
      <Button variant="ghost" aria-label="Notifications">
        Alerts
      </Button>
      <Button variant="ghost" onClick={onTogglePanel} aria-label="Toggle context panel">
        Panel
      </Button>
      <div
        aria-label="User avatar"
        style={{
          width: 32,
          height: 32,
          borderRadius: '50%',
          background: 'var(--cos-color-accent)',
          color: 'var(--cos-color-accent-fg)',
          display: 'grid',
          placeItems: 'center',
          fontSize: '0.75rem',
          fontWeight: 600,
        }}
      >
        YO
      </div>
    </header>
  );
}
