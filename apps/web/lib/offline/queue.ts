/** Offline mutation queue + conflict resolution (Phase 4.5).
 *
 * Rule: last-write-wins via optimistic `If-Match` version.
 * On 409/412 the loser is NOT silently dropped — a conflict record is surfaced
 * for the user to acknowledge and optionally retry with the server version.
 */

export type QueuedMutation = {
  id: string;
  createdAt: number;
  method: 'POST' | 'PATCH' | 'DELETE';
  path: string;
  body?: unknown;
  /** Stable Idempotency-Key — replay must reuse the same key. */
  idempotencyKey: string;
  /** Optimistic concurrency version when applicable. */
  ifMatch?: number;
  /** Human label for conflict UI. */
  label: string;
};

export type ConflictRecord = {
  id: string;
  mutationId: string;
  path: string;
  label: string;
  clientBody?: unknown;
  serverStatus: number;
  serverDetail: string;
  /** Deterministic rule applied when user accepts server / retries. */
  resolutionRule: 'last-write-wins-if-match';
  createdAt: number;
  resolvedAt?: number;
  resolvedValue?: 'kept-server' | 'retried-client';
};

const QUEUE_KEY = 'cos-offline-mutation-queue';
const CONFLICT_KEY = 'cos-offline-conflicts';
const CACHE_PREFIX = 'cos-offline-cache:';

type StorageLike = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
};

function storage(): StorageLike | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadQueue(store: StorageLike | null = storage()): QueuedMutation[] {
  if (!store) return [];
  try {
    const raw = store.getItem(QUEUE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as QueuedMutation[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function saveQueue(queue: QueuedMutation[], store: StorageLike | null = storage()) {
  if (!store) return;
  store.setItem(QUEUE_KEY, JSON.stringify(queue));
}

export function enqueueMutation(
  mutation: Omit<QueuedMutation, 'id' | 'createdAt'> & { id?: string; createdAt?: number },
  store: StorageLike | null = storage(),
): QueuedMutation {
  const item: QueuedMutation = {
    id: mutation.id ?? cryptoRandomId(),
    createdAt: mutation.createdAt ?? Date.now(),
    method: mutation.method,
    path: mutation.path,
    body: mutation.body,
    idempotencyKey: mutation.idempotencyKey,
    ifMatch: mutation.ifMatch,
    label: mutation.label,
  };
  const queue = loadQueue(store);
  queue.push(item);
  saveQueue(queue, store);
  return item;
}

export function loadConflicts(store: StorageLike | null = storage()): ConflictRecord[] {
  if (!store) return [];
  try {
    const raw = store.getItem(CONFLICT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as ConflictRecord[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function saveConflicts(conflicts: ConflictRecord[], store: StorageLike | null = storage()) {
  if (!store) return;
  store.setItem(CONFLICT_KEY, JSON.stringify(conflicts));
}

export function openConflicts(store: StorageLike | null = storage()): ConflictRecord[] {
  return loadConflicts(store).filter((c) => !c.resolvedAt);
}

export function recordConflict(
  conflict: Omit<ConflictRecord, 'id' | 'createdAt' | 'resolutionRule'> & {
    id?: string;
    createdAt?: number;
  },
  store: StorageLike | null = storage(),
): ConflictRecord {
  const item: ConflictRecord = {
    id: conflict.id ?? cryptoRandomId(),
    createdAt: conflict.createdAt ?? Date.now(),
    resolutionRule: 'last-write-wins-if-match',
    mutationId: conflict.mutationId,
    path: conflict.path,
    label: conflict.label,
    clientBody: conflict.clientBody,
    serverStatus: conflict.serverStatus,
    serverDetail: conflict.serverDetail,
  };
  const all = loadConflicts(store);
  all.push(item);
  saveConflicts(all, store);
  return item;
}

export function resolveConflict(
  conflictId: string,
  resolvedValue: ConflictRecord['resolvedValue'],
  store: StorageLike | null = storage(),
): ConflictRecord | null {
  const all = loadConflicts(store);
  const idx = all.findIndex((c) => c.id === conflictId);
  if (idx < 0) return null;
  all[idx] = {
    ...all[idx],
    resolvedAt: Date.now(),
    resolvedValue,
  };
  saveConflicts(all, store);
  return all[idx];
}

export function cacheRead(path: string, data: unknown, store: StorageLike | null = storage()) {
  if (!store) return;
  store.setItem(
    `${CACHE_PREFIX}${path}`,
    JSON.stringify({ asOf: Date.now(), data }),
  );
}

export function readCache<T = unknown>(
  path: string,
  store: StorageLike | null = storage(),
): { asOf: number; data: T } | null {
  if (!store) return null;
  try {
    const raw = store.getItem(`${CACHE_PREFIX}${path}`);
    if (!raw) return null;
    return JSON.parse(raw) as { asOf: number; data: T };
  } catch {
    return null;
  }
}

export function isNetworkError(err: unknown): boolean {
  if (err instanceof TypeError) return true;
  if (err && typeof err === 'object' && 'message' in err) {
    const msg = String((err as { message: unknown }).message).toLowerCase();
    return msg.includes('failed to fetch') || msg.includes('network');
  }
  return false;
}

export function shouldSurfaceConflict(status: number): boolean {
  return status === 409 || status === 412;
}

/** Deterministic: matching If-Match wins; stale version loses with visible conflict. */
export function explainConflictRule(): string {
  return 'last-write-wins with If-Match version — stale writes surface a conflict and are not silently dropped';
}

export type ReplayFetch = (
  path: string,
  init: RequestInit & { idempotencyKey?: string; ifMatch?: number },
) => Promise<Response>;

export type ReplayResult = {
  replayed: number;
  conflicts: ConflictRecord[];
  failures: Array<{ mutationId: string; status: number; detail: string }>;
};

/**
 * Replay queued mutations in FIFO order.
 * Same Idempotency-Key is sent on every attempt so POST/PATCH/DELETE cannot duplicate.
 */
export async function replayQueue(
  fetchImpl: ReplayFetch,
  store: StorageLike | null = storage(),
): Promise<ReplayResult> {
  const queue = loadQueue(store);
  const remaining: QueuedMutation[] = [];
  const conflicts: ConflictRecord[] = [];
  const failures: ReplayResult['failures'] = [];
  let replayed = 0;

  for (const m of queue) {
    const headers: Record<string, string> = {
      'Idempotency-Key': m.idempotencyKey,
    };
    if (m.ifMatch != null) {
      headers['If-Match'] = String(m.ifMatch);
    }
    let res: Response;
    try {
      res = await fetchImpl(m.path, {
        method: m.method,
        body: m.body != null ? JSON.stringify(m.body) : undefined,
        headers,
        idempotencyKey: m.idempotencyKey,
        ifMatch: m.ifMatch,
      });
    } catch (err) {
      remaining.push(m);
      failures.push({
        mutationId: m.id,
        status: 0,
        detail: err instanceof Error ? err.message : 'network error',
      });
      continue;
    }

    if (res.ok || res.status === 201) {
      replayed += 1;
      continue;
    }

    const detail = await res.text().catch(() => res.statusText);
    if (shouldSurfaceConflict(res.status)) {
      const conflict = recordConflict(
        {
          mutationId: m.id,
          path: m.path,
          label: m.label,
          clientBody: m.body,
          serverStatus: res.status,
          serverDetail: detail,
        },
        store,
      );
      conflicts.push(conflict);
      // Do not re-queue — user must resolve visibly.
      continue;
    }

    remaining.push(m);
    failures.push({ mutationId: m.id, status: res.status, detail });
  }

  saveQueue(remaining, store);
  return { replayed, conflicts, failures };
}

function cryptoRandomId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `offline-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

/** Test helper: force-clear offline state. */
export function clearOfflineState(store: StorageLike | null = storage()) {
  if (!store) return;
  store.removeItem(QUEUE_KEY);
  store.removeItem(CONFLICT_KEY);
}
