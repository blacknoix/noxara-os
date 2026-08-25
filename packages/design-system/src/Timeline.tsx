import type { ReactNode } from 'react';

export type TimelineItem = {
  id: string;
  title: ReactNode;
  description?: ReactNode;
  timestamp?: ReactNode;
  icon?: ReactNode;
};

export type TimelineProps = {
  items: TimelineItem[];
};

export function Timeline({ items }: TimelineProps) {
  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        fontFamily: 'var(--cos-font-sans)',
        position: 'relative',
      }}
    >
      {items.map((item, i) => (
        <li
          key={item.id}
          style={{
            display: 'grid',
            gridTemplateColumns: '24px 1fr',
            gap: 'var(--cos-space-3)',
            paddingBottom: i === items.length - 1 ? 0 : 'var(--cos-space-6)',
            position: 'relative',
          }}
        >
          <div style={{ position: 'relative', display: 'flex', justifyContent: 'center' }}>
            <span
              aria-hidden="true"
              style={{
                width: 12,
                height: 12,
                borderRadius: '50%',
                background: 'var(--cos-color-accent)',
                marginTop: 4,
                zIndex: 1,
              }}
            >
              {item.icon}
            </span>
            {i < items.length - 1 ? (
              <span
                aria-hidden="true"
                style={{
                  position: 'absolute',
                  top: 16,
                  bottom: -8,
                  width: 2,
                  background: 'var(--cos-color-border)',
                }}
              />
            ) : null}
          </div>
          <div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap' }}>
              <strong style={{ color: 'var(--cos-color-fg)' }}>{item.title}</strong>
              {item.timestamp ? (
                <time style={{ fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>{item.timestamp}</time>
              ) : null}
            </div>
            {item.description ? (
              <p style={{ margin: '0.25rem 0 0', fontSize: '0.875rem', color: 'var(--cos-color-fg-muted)' }}>
                {item.description}
              </p>
            ) : null}
          </div>
        </li>
      ))}
    </ol>
  );
}
