'use client';

import { useEffect } from 'react';

/** Registers the offline-first service worker (no-op on SSR / unsupported browsers). */
export function ServiceWorkerRegister() {
  useEffect(() => {
    if (typeof window === 'undefined' || !('serviceWorker' in navigator)) return;
    void navigator.serviceWorker.register('/sw.js').catch(() => {
      /* ignore registration failures in CI / private mode */
    });
  }, []);
  return null;
}
