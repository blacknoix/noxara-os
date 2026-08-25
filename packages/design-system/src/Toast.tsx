'use client';

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { Button } from './Button';

export type ToastItem = {
  id: string;
  title: string;
  description?: string;
  undo?: () => void;
  durationMs?: number;
};

type ToastContextValue = {
  toast: (item: Omit<ToastItem, 'id'> & { id?: string }) => string;
  dismiss: (id: string) => void;
};

const ToastContext = createContext<ToastContextValue | null>(null);

const MAX_TOASTS = 3;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const dismiss = useCallback((id: string) => {
    setItems((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const toast = useCallback(
    (item: Omit<ToastItem, 'id'> & { id?: string }) => {
      const id = item.id ?? `toast-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      setItems((prev) => {
        const next = [...prev, { ...item, id }];
        return next.slice(-MAX_TOASTS);
      });
      const duration = item.durationMs ?? 5000;
      if (duration > 0) {
        window.setTimeout(() => dismiss(id), duration);
      }
      return id;
    },
    [dismiss],
  );

  const value = useMemo(() => ({ toast, dismiss }), [toast, dismiss]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div
        aria-live="polite"
        aria-relevant="additions text"
        style={{
          position: 'fixed',
          bottom: 'var(--cos-space-4)',
          right: 'var(--cos-space-4)',
          zIndex: 'var(--cos-z-toast)',
          display: 'flex',
          flexDirection: 'column',
          gap: 'var(--cos-space-2)',
          maxWidth: 360,
          fontFamily: 'var(--cos-font-sans)',
        }}
      >
        {items.map((t) => (
          <div
            key={t.id}
            role="status"
            style={{
              background: 'var(--cos-color-bg-elevated)',
              border: '1px solid var(--cos-color-border)',
              borderRadius: 'var(--cos-radius-md)',
              boxShadow: 'var(--cos-shadow-md)',
              padding: 'var(--cos-space-3)',
              display: 'flex',
              gap: 'var(--cos-space-3)',
              alignItems: 'flex-start',
            }}
          >
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 600, color: 'var(--cos-color-fg)' }}>{t.title}</div>
              {t.description ? (
                <div style={{ fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)', marginTop: 2 }}>
                  {t.description}
                </div>
              ) : null}
            </div>
            {t.undo ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  t.undo?.();
                  dismiss(t.id);
                }}
              >
                Undo
              </Button>
            ) : null}
            <button
              type="button"
              aria-label="Dismiss"
              onClick={() => dismiss(t.id)}
              style={{ all: 'unset', cursor: 'pointer', color: 'var(--cos-color-fg-muted)' }}
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used within ToastProvider');
  return ctx;
}
