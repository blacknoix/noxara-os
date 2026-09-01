import { describe, expect, it, beforeEach } from 'vitest';
import {
  clearOfflineState,
  enqueueMutation,
  explainConflictRule,
  loadQueue,
  openConflicts,
  recordConflict,
  replayQueue,
  resolveConflict,
  shouldSurfaceConflict,
} from './queue';

class MemoryStore {
  private data = new Map<string, string>();
  getItem(key: string) {
    return this.data.has(key) ? this.data.get(key)! : null;
  }
  setItem(key: string, value: string) {
    this.data.set(key, value);
  }
  removeItem(key: string) {
    this.data.delete(key);
  }
}

describe('offline queue', () => {
  let store: MemoryStore;

  beforeEach(() => {
    store = new MemoryStore();
    clearOfflineState(store);
  });

  it('queues mutations and replays with the same idempotency key (no duplicate)', async () => {
    const key = 'idem-expense-1';
    enqueueMutation(
      {
        method: 'POST',
        path: '/api/v1/finance/expenses',
        body: { amount_minor: 1000, currency: 'USD', description: 'Taxi' },
        idempotencyKey: key,
        label: 'Submit expense',
      },
      store,
    );
    expect(loadQueue(store)).toHaveLength(1);

    const seenKeys: string[] = [];
    const result = await replayQueue(async (_path, init) => {
      const headers = new Headers(init.headers);
      seenKeys.push(headers.get('Idempotency-Key') ?? '');
      return new Response(JSON.stringify({ id: 'exp_1' }), { status: 201 });
    }, store);

    expect(result.replayed).toBe(1);
    expect(seenKeys).toEqual([key]);
    expect(loadQueue(store)).toHaveLength(0);

    // Second enqueue with same logical key still sends that key (server dedupes).
    enqueueMutation(
      {
        method: 'POST',
        path: '/api/v1/finance/expenses',
        body: { amount_minor: 1000, currency: 'USD', description: 'Taxi' },
        idempotencyKey: key,
        label: 'Submit expense',
      },
      store,
    );
    await replayQueue(async (_path, init) => {
      const headers = new Headers(init.headers);
      seenKeys.push(headers.get('Idempotency-Key') ?? '');
      // Idempotent replay — same resource, not a duplicate create.
      return new Response(JSON.stringify({ id: 'exp_1' }), { status: 200 });
    }, store);
    expect(seenKeys).toEqual([key, key]);
  });

  it('surfaces conflicts instead of silently dropping the loser', async () => {
    enqueueMutation(
      {
        method: 'PATCH',
        path: '/api/v1/operations/tasks/tsk_1',
        body: { title: 'stale edit' },
        idempotencyKey: 'idem-task-1',
        ifMatch: 1,
        label: 'Update task',
      },
      store,
    );

    const result = await replayQueue(async () => {
      return new Response(JSON.stringify({ detail: 'version mismatch: expected 1, current 2' }), {
        status: 409,
      });
    }, store);

    expect(result.conflicts).toHaveLength(1);
    expect(result.conflicts[0].serverDetail).toContain('version mismatch');
    expect(openConflicts(store)).toHaveLength(1);
    expect(loadQueue(store)).toHaveLength(0);
    expect(shouldSurfaceConflict(409)).toBe(true);
    expect(explainConflictRule()).toContain('last-write-wins');

    const resolved = resolveConflict(result.conflicts[0].id, 'kept-server', store);
    expect(resolved?.resolvedValue).toBe('kept-server');
    expect(openConflicts(store)).toHaveLength(0);
  });

  it('records an explicit conflict for UI', () => {
    const c = recordConflict(
      {
        mutationId: 'm1',
        path: '/api/v1/custom/records/engagement/x',
        label: 'Update engagement',
        serverStatus: 409,
        serverDetail: 'version mismatch',
        clientBody: { title: 'local' },
      },
      store,
    );
    expect(c.resolutionRule).toBe('last-write-wins-if-match');
    expect(openConflicts(store)[0].id).toBe(c.id);
  });
});
