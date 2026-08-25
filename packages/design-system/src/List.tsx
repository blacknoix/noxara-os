import type { HTMLAttributes, ReactNode } from 'react';

export type ListItem = {
  id: string;
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
};

export type ListProps = HTMLAttributes<HTMLUListElement> & {
  items: ListItem[];
  onItemSelect?: (id: string) => void;
};

export function List({ items, onItemSelect, style, ...rest }: ListProps) {
  return (
    <ul
      {...rest}
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        fontFamily: 'var(--cos-font-sans)',
        ...style,
      }}
    >
      {items.map((item) => (
        <li
          key={item.id}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 'var(--cos-space-3)',
            padding: 'var(--cos-space-3) 0',
            borderBottom: '1px solid var(--cos-color-border)',
            cursor: onItemSelect ? 'pointer' : undefined,
          }}
          onClick={() => onItemSelect?.(item.id)}
          onKeyDown={(e) => {
            if (onItemSelect && (e.key === 'Enter' || e.key === ' ')) {
              e.preventDefault();
              onItemSelect(item.id);
            }
          }}
          tabIndex={onItemSelect ? 0 : undefined}
          role={onItemSelect ? 'button' : undefined}
        >
          {item.leading}
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 600, color: 'var(--cos-color-fg)' }}>{item.title}</div>
            {item.description ? (
              <div style={{ fontSize: '0.8125rem', color: 'var(--cos-color-fg-muted)', marginTop: 2 }}>
                {item.description}
              </div>
            ) : null}
          </div>
          {item.meta ? (
            <div style={{ fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>{item.meta}</div>
          ) : null}
          {item.trailing}
        </li>
      ))}
    </ul>
  );
}
