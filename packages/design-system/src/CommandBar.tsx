'use client';

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

export type CommandItem = {
  id: string;
  label: string;
  group?: string;
  shortcut?: string;
  disabled?: boolean;
  onSelect: () => void;
};

export type CommandBarProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  query: string;
  onQueryChange: (query: string) => void;
  items: CommandItem[];
  placeholder?: string;
  emptyMessage?: ReactNode;
};

export function CommandBar({
  open,
  onOpenChange,
  query,
  onQueryChange,
  items,
  placeholder = 'Type a command…',
  emptyMessage = 'No results',
}: CommandBarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [active, setActive] = useState(0);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((i) => i.label.toLowerCase().includes(q) || i.group?.toLowerCase().includes(q));
  }, [items, query]);

  const groups = useMemo(() => {
    const map = new Map<string, CommandItem[]>();
    for (const item of filtered) {
      const g = item.group ?? 'Commands';
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(item);
    }
    return map;
  }, [filtered]);

  const flat = filtered;

  useEffect(() => {
    if (open) {
      setActive(0);
      inputRef.current?.focus();
    }
  }, [open]);

  useEffect(() => {
    setActive(0);
  }, [query]);

  if (!open) return null;

  const select = (item: CommandItem) => {
    if (item.disabled) return;
    item.onSelect();
    onOpenChange(false);
  };

  return (
    <div
      role="presentation"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 'var(--cos-z-command)',
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'center',
        paddingTop: '12vh',
        background: 'var(--cos-color-overlay)',
      }}
      onClick={() => onOpenChange(false)}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command bar"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%',
          maxWidth: 560,
          background: 'var(--cos-color-bg-elevated)',
          border: '1px solid var(--cos-color-border)',
          borderRadius: 'var(--cos-radius-md)',
          boxShadow: 'var(--cos-shadow-md)',
          fontFamily: 'var(--cos-font-sans)',
          overflow: 'hidden',
        }}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder={placeholder}
          aria-autocomplete="list"
          aria-controls="cos-command-list"
          style={{
            width: '100%',
            border: 'none',
            borderBottom: '1px solid var(--cos-color-border)',
            padding: 'var(--cos-space-4)',
            fontSize: '1rem',
            fontFamily: 'inherit',
            background: 'transparent',
            color: 'var(--cos-color-fg)',
            outline: 'none',
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.preventDefault();
              onOpenChange(false);
            } else if (e.key === 'ArrowDown') {
              e.preventDefault();
              setActive((i) => Math.min(flat.length - 1, i + 1));
            } else if (e.key === 'ArrowUp') {
              e.preventDefault();
              setActive((i) => Math.max(0, i - 1));
            } else if (e.key === 'Enter') {
              e.preventDefault();
              const item = flat[active];
              if (item) select(item);
            }
          }}
        />
        <ul
          id="cos-command-list"
          role="listbox"
          style={{ listStyle: 'none', margin: 0, padding: 'var(--cos-space-2)', maxHeight: 320, overflow: 'auto' }}
        >
          {flat.length === 0 ? (
            <li style={{ padding: 'var(--cos-space-3)', color: 'var(--cos-color-fg-muted)' }}>{emptyMessage}</li>
          ) : (
            Array.from(groups.entries()).map(([group, groupItems]) => (
              <li key={group}>
                <div
                  style={{
                    fontSize: '0.7rem',
                    fontWeight: 700,
                    color: 'var(--cos-color-fg-muted)',
                    padding: '0.35rem 0.5rem',
                    textTransform: 'uppercase',
                    letterSpacing: '0.04em',
                  }}
                >
                  {group}
                </div>
                <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
                  {groupItems.map((item) => {
                    const idx = flat.indexOf(item);
                    const selected = idx === active;
                    return (
                      <li key={item.id} role="option" aria-selected={selected} aria-disabled={item.disabled || undefined}>
                        <button
                          type="button"
                          disabled={item.disabled}
                          onMouseEnter={() => setActive(idx)}
                          onClick={() => select(item)}
                          style={{
                            width: '100%',
                            display: 'flex',
                            justifyContent: 'space-between',
                            alignItems: 'center',
                            gap: 8,
                            textAlign: 'left',
                            padding: '0.55rem 0.65rem',
                            border: 'none',
                            borderRadius: 'var(--cos-radius-sm)',
                            background: selected ? 'var(--cos-color-accent-muted)' : 'transparent',
                            color: 'var(--cos-color-fg)',
                            fontFamily: 'inherit',
                            fontSize: '0.9rem',
                            cursor: item.disabled ? 'not-allowed' : 'pointer',
                            opacity: item.disabled ? 0.5 : 1,
                          }}
                        >
                          <span>{item.label}</span>
                          {item.shortcut ? (
                            <kbd
                              style={{
                                fontSize: '0.7rem',
                                color: 'var(--cos-color-fg-muted)',
                                border: '1px solid var(--cos-color-border)',
                                borderRadius: 4,
                                padding: '0.1rem 0.35rem',
                              }}
                            >
                              {item.shortcut}
                            </kbd>
                          ) : null}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  );
}
