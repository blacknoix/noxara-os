'use client';

import { useState, type DragEvent, type ReactNode } from 'react';

export type KanbanCard = {
  id: string;
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
};

export type KanbanColumn = {
  id: string;
  title: string;
  cards: KanbanCard[];
};

export type KanbanBoardProps = {
  columns: KanbanColumn[];
  onCardSelect?: (cardId: string, columnId: string) => void;
  /** Called after a card is dropped on a different column (HTML5 DnD). */
  onCardMove?: (cardId: string, fromColumnId: string, toColumnId: string) => void;
};

const DRAG_MIME = 'application/x-companyos-kanban-card';

/** Kanban board with HTML5 drag-and-drop between columns; click-to-select preserved. */
export function KanbanBoard({ columns, onCardSelect, onCardMove }: KanbanBoardProps) {
  const [draggingCardId, setDraggingCardId] = useState<string | null>(null);
  const [dragOverColumnId, setDragOverColumnId] = useState<string | null>(null);
  const draggable = Boolean(onCardMove);

  const handleDragStart = (e: DragEvent<HTMLLIElement>, cardId: string, columnId: string) => {
    e.dataTransfer.setData(DRAG_MIME, JSON.stringify({ cardId, columnId }));
    e.dataTransfer.effectAllowed = 'move';
    setDraggingCardId(cardId);
  };

  const handleDragEnd = () => {
    setDraggingCardId(null);
    setDragOverColumnId(null);
  };

  const handleColumnDragOver = (e: DragEvent<HTMLElement>, columnId: string) => {
    if (!draggable) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOverColumnId(columnId);
  };

  const handleColumnDragLeave = (columnId: string) => {
    setDragOverColumnId((current) => (current === columnId ? null : current));
  };

  const handleColumnDrop = (e: DragEvent<HTMLElement>, toColumnId: string) => {
    if (!draggable) return;
    e.preventDefault();
    setDragOverColumnId(null);
    setDraggingCardId(null);
    let raw: string;
    try {
      raw = e.dataTransfer.getData(DRAG_MIME);
    } catch {
      return;
    }
    if (!raw) return;
    let parsed: { cardId: string; columnId: string } | null = null;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return;
    }
    if (!parsed || !parsed.cardId || !parsed.columnId) return;
    if (parsed.columnId === toColumnId) return;
    onCardMove?.(parsed.cardId, parsed.columnId, toColumnId);
  };

  return (
    <div
      style={{
        display: 'flex',
        gap: 'var(--cos-space-4)',
        overflowX: 'auto',
        fontFamily: 'var(--cos-font-sans)',
        alignItems: 'flex-start',
      }}
    >
      {columns.map((col) => (
        <section
          key={col.id}
          aria-label={col.title}
          onDragOver={(e) => handleColumnDragOver(e, col.id)}
          onDragLeave={() => handleColumnDragLeave(col.id)}
          onDrop={(e) => handleColumnDrop(e, col.id)}
          style={{
            flex: '0 0 280px',
            minWidth: 240,
            background:
              dragOverColumnId === col.id ? 'var(--cos-color-accent-muted)' : 'var(--cos-color-bg-muted)',
            borderRadius: 'var(--cos-radius-md)',
            padding: 'var(--cos-space-3)',
            outline: dragOverColumnId === col.id ? '2px dashed var(--cos-color-accent)' : undefined,
            outlineOffset: -2,
            transition: 'background var(--cos-duration-fast) var(--cos-ease-standard)',
          }}
        >
          <h3
            style={{
              margin: '0 0 var(--cos-space-3)',
              fontSize: '0.8125rem',
              fontWeight: 700,
              color: 'var(--cos-color-fg-muted)',
              textTransform: 'none',
            }}
          >
            {col.title}{' '}
            <span style={{ fontWeight: 500 }}>({col.cards.length})</span>
          </h3>
          <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
            {col.cards.map((card) => (
              <li
                key={card.id}
                draggable={draggable}
                onDragStart={(e) => handleDragStart(e, card.id, col.id)}
                onDragEnd={handleDragEnd}
                style={{
                  opacity: draggingCardId === card.id ? 0.5 : 1,
                  cursor: draggable ? 'grab' : undefined,
                }}
              >
                <button
                  type="button"
                  onClick={() => onCardSelect?.(card.id, col.id)}
                  style={{
                    width: '100%',
                    textAlign: 'left',
                    background: 'var(--cos-color-bg-elevated)',
                    border: '1px solid var(--cos-color-border)',
                    borderRadius: 'var(--cos-radius-sm)',
                    padding: 'var(--cos-space-3)',
                    cursor: onCardSelect ? 'pointer' : draggable ? 'grab' : 'default',
                    fontFamily: 'inherit',
                    color: 'var(--cos-color-fg)',
                  }}
                >
                  <div style={{ fontWeight: 600 }}>{card.title}</div>
                  {card.description ? (
                    <div style={{ marginTop: 4, fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)' }}>
                      {card.description}
                    </div>
                  ) : null}
                  {card.meta ? (
                    <div style={{ marginTop: 6, fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>
                      {card.meta}
                    </div>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
