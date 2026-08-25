import type { ReactNode } from 'react';

export type Column<T> = {
  key: string;
  header: string;
  cell: (row: T) => ReactNode;
};

export type TableProps<T> = {
  columns: Column<T>[];
  rows: T[];
  empty?: ReactNode;
};

/** Lightweight table skeleton — no card chrome. */
export function Table<T>({ columns, rows, empty }: TableProps<T>) {
  if (rows.length === 0) {
    return <>{empty ?? null}</>;
  }
  return (
    <div style={{ overflowX: 'auto' }}>
      <table
        style={{
          width: '100%',
          borderCollapse: 'collapse',
          fontFamily: 'var(--cos-font-sans)',
          fontSize: '0.9rem',
        }}
      >
        <thead>
          <tr>
            {columns.map((c) => (
              <th
                key={c.key}
                style={{
                  textAlign: 'left',
                  padding: '0.65rem 0.5rem',
                  borderBottom: '1px solid var(--cos-color-border)',
                  color: 'var(--cos-color-fg-muted)',
                  fontWeight: 600,
                }}
              >
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i}>
              {columns.map((c) => (
                <td
                  key={c.key}
                  style={{
                    padding: '0.75rem 0.5rem',
                    borderBottom: '1px solid var(--cos-color-border)',
                    color: 'var(--cos-color-fg)',
                  }}
                >
                  {c.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
