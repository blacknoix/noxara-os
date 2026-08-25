import type { ReactNode } from 'react';

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
};

/** Structural kanban stub — columns + cards; DnD left to consumers. */
export function KanbanBoard({ columns, onCardSelect }: KanbanBoardProps) {
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
          style={{
            flex: '0 0 280px',
            minWidth: 240,
            background: 'var(--cos-color-bg-muted)',
            borderRadius: 'var(--cos-radius-md)',
            padding: 'var(--cos-space-3)',
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
              <li key={card.id}>
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
                    cursor: onCardSelect ? 'pointer' : 'default',
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
