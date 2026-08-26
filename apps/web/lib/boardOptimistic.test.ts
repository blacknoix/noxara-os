import { describe, expect, it } from 'vitest';
import { applyOptimisticMove, shouldRollback } from './boardOptimistic';

describe('boardOptimistic', () => {
  const columns = [
    {
      id: 'todo',
      cards: [
        { id: 'tsk_1', title: 'Alpha' },
        { id: 'tsk_2', title: 'Beta' },
      ],
    },
    { id: 'in_progress', cards: [{ id: 'tsk_3', title: 'Gamma' }] },
    { id: 'done', cards: [] as { id: string; title: string }[] },
  ];

  it('applyOptimisticMove moves a card between columns', () => {
    const { previous, next } = applyOptimisticMove(columns, 'tsk_1', 'todo', 'in_progress');

    expect(previous[0].cards.map((c) => c.id)).toEqual(['tsk_1', 'tsk_2']);
    expect(next[0].cards.map((c) => c.id)).toEqual(['tsk_2']);
    expect(next[1].cards.map((c) => c.id)).toEqual(['tsk_1', 'tsk_3']);
  });

  it('rollback restores previous columns after a 409', () => {
    const { previous, next } = applyOptimisticMove(columns, 'tsk_2', 'todo', 'done');
    expect(next[2].cards.map((c) => c.id)).toEqual(['tsk_2']);
    expect(shouldRollback(409)).toBe(true);

    // Simulate UI rollback: restore previous snapshot.
    const rolledBack = previous;
    expect(rolledBack[0].cards.map((c) => c.id)).toEqual(['tsk_1', 'tsk_2']);
    expect(rolledBack[2].cards).toEqual([]);
  });

  it('shouldRollback is true for 409/412 and false otherwise', () => {
    expect(shouldRollback(409)).toBe(true);
    expect(shouldRollback(412)).toBe(true);
    expect(shouldRollback(200)).toBe(false);
    expect(shouldRollback(403)).toBe(false);
  });

  it('no-ops when from and to columns match', () => {
    const { previous, next } = applyOptimisticMove(columns, 'tsk_1', 'todo', 'todo');
    expect(next).toEqual(previous);
  });
});
