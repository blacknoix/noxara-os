/** Minimal kanban column shape used by optimistic board moves. */
export type OptimisticColumn<TCard extends { id: string } = { id: string }> = {
  id: string;
  cards: TCard[];
};

/**
 * Move a card from one column to another without mutating the input.
 * Returns previous columns for rollback plus the optimistic next state.
 */
export function applyOptimisticMove<TCard extends { id: string }>(
  columns: OptimisticColumn<TCard>[],
  cardId: string,
  fromCol: string,
  toCol: string,
): { previous: OptimisticColumn<TCard>[]; next: OptimisticColumn<TCard>[] } {
  const previous = columns.map((col) => ({
    ...col,
    cards: [...col.cards],
  }));

  if (fromCol === toCol) {
    return { previous, next: previous };
  }

  let moved: TCard | undefined;
  const without = previous.map((col) => {
    if (col.id !== fromCol) return col;
    const card = col.cards.find((c) => c.id === cardId);
    if (card) moved = card;
    return { ...col, cards: col.cards.filter((c) => c.id !== cardId) };
  });

  if (!moved) {
    return { previous, next: previous };
  }

  const next = without.map((col) =>
    col.id === toCol ? { ...col, cards: [moved as TCard, ...col.cards] } : col,
  );

  return { previous, next };
}

/** True when the server rejected an optimistic move (conflict / version mismatch). */
export function shouldRollback(status: number): boolean {
  return status === 409 || status === 412;
}
