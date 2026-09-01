'use client';

import { useCallback, useEffect, useState } from 'react';
import { authFetch } from '../auth-client';
import {
  cacheRead,
  enqueueMutation,
  isNetworkError,
  loadQueue,
  openConflicts,
  readCache,
  replayQueue,
  type ConflictRecord,
  type QueuedMutation,
} from './queue';

export type OfflineStatus = {
  online: boolean;
  queued: number;
  conflicts: number;
};

const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((l) => l());
}

/** Cached GET for dashboard / tasks / approvals when offline. */
export async function offlineGet<T = unknown>(path: string): Promise<{
  data: T | null;
  fromCache: boolean;
  asOf?: number;
}> {
  try {
    const res = await authFetch(path);
    if (!res.ok) {
      const cached = readCache<T>(path);
      return { data: cached?.data ?? null, fromCache: !!cached, asOf: cached?.asOf };
    }
    const data = (await res.json()) as T;
    cacheRead(path, data);
    return { data, fromCache: false };
  } catch (err) {
    if (isNetworkError(err)) {
      const cached = readCache<T>(path);
      return { data: cached?.data ?? null, fromCache: !!cached, asOf: cached?.asOf };
    }
    throw err;
  }
}

/**
 * Mutation helper: when offline (or network fails), enqueue with a stable
 * Idempotency-Key and optional If-Match. On reconnect, {@link flushOfflineQueue}
 * replays without duplicating.
 */
export async function offlineMutate(opts: {
  method: 'POST' | 'PATCH' | 'DELETE';
  path: string;
  body?: unknown;
  label: string;
  idempotencyKey?: string;
  ifMatch?: number;
}): Promise<{ ok: boolean; queued: boolean; response?: Response; status?: number }> {
  const idempotencyKey = opts.idempotencyKey ?? crypto.randomUUID();
  const headers: Record<string, string> = {
    'Idempotency-Key': idempotencyKey,
  };
  if (opts.ifMatch != null) {
    headers['If-Match'] = String(opts.ifMatch);
  }

  const online = typeof navigator === 'undefined' ? true : navigator.onLine;
  if (!online) {
    enqueueMutation({
      method: opts.method,
      path: opts.path,
      body: opts.body,
      idempotencyKey,
      ifMatch: opts.ifMatch,
      label: opts.label,
    });
    notify();
    return { ok: true, queued: true };
  }

  try {
    const res = await authFetch(opts.path, {
      method: opts.method,
      body: opts.body != null ? JSON.stringify(opts.body) : undefined,
      headers,
    });
    return { ok: res.ok, queued: false, response: res, status: res.status };
  } catch (err) {
    if (isNetworkError(err)) {
      enqueueMutation({
        method: opts.method,
        path: opts.path,
        body: opts.body,
        idempotencyKey,
        ifMatch: opts.ifMatch,
        label: opts.label,
      });
      notify();
      return { ok: true, queued: true };
    }
    throw err;
  }
}

export async function flushOfflineQueue() {
  const result = await replayQueue(async (path, init) => {
    const headers = new Headers(init.headers);
    if (init.idempotencyKey) headers.set('Idempotency-Key', init.idempotencyKey);
    if (init.ifMatch != null) headers.set('If-Match', String(init.ifMatch));
    return authFetch(path, {
      method: init.method,
      body: init.body,
      headers,
    });
  });
  notify();
  return result;
}

export function useOfflineStatus(): OfflineStatus & {
  queue: QueuedMutation[];
  conflictList: ConflictRecord[];
  refresh: () => void;
} {
  const [online, setOnline] = useState(true);
  const [queue, setQueue] = useState<QueuedMutation[]>([]);
  const [conflictList, setConflictList] = useState<ConflictRecord[]>([]);

  const refresh = useCallback(() => {
    setQueue(loadQueue());
    setConflictList(openConflicts());
    setOnline(typeof navigator === 'undefined' ? true : navigator.onLine);
  }, []);

  useEffect(() => {
    refresh();
    const onChange = () => {
      refresh();
      if (navigator.onLine) {
        void flushOfflineQueue();
      }
    };
    window.addEventListener('online', onChange);
    window.addEventListener('offline', onChange);
    listeners.add(refresh);
    return () => {
      window.removeEventListener('online', onChange);
      window.removeEventListener('offline', onChange);
      listeners.delete(refresh);
    };
  }, [refresh]);

  return {
    online,
    queued: queue.length,
    conflicts: conflictList.length,
    queue,
    conflictList,
    refresh,
  };
}
