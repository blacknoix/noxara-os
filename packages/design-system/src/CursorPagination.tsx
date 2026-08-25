'use client';

import { Button } from './Button';

export type CursorPaginationProps = {
  hasMore?: boolean;
  loading?: boolean;
  onLoadMore?: () => void;
  cursor?: string | null;
  label?: string;
};

export function CursorPagination({
  hasMore = false,
  loading,
  onLoadMore,
  cursor,
  label = 'Load more',
}: CursorPaginationProps) {
  if (!hasMore && !cursor) return null;
  return (
    <div
      style={{
        display: 'flex',
        justifyContent: 'center',
        padding: 'var(--cos-space-4) 0',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      <Button variant="secondary" onClick={onLoadMore} loading={loading} disabled={!hasMore || loading}>
        {label}
      </Button>
    </div>
  );
}
