'use client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Checkbox } from './Checkbox';

export type TableDensity = 'compact' | 'comfortable' | 'spacious';
export type SortDir = 'asc' | 'desc';

export type Column<T> = {
  key: string;
  header: string;
  cell: (row: T) => ReactNode;
  width?: number | string;
  minWidth?: number;
  sortable?: boolean;
  pin?: 'left' | 'right';
  hideable?: boolean;
  align?: 'left' | 'center' | 'right';
};

export type BulkAction = {
  id: string;
  label: string;
  onSelect: (selectedKeys: string[]) => void;
  danger?: boolean;
};

export type TableProps<T> = {
  columns: Column<T>[];
  rows: T[];
  empty?: ReactNode;
  density?: TableDensity;
  sortKey?: string;
  sortDir?: SortDir;
  onSortChange?: (key: string, dir: SortDir) => void;
  columnOrder?: string[];
  onColumnOrderChange?: (order: string[]) => void;
  hiddenColumns?: string[];
  onHiddenColumnsChange?: (keys: string[]) => void;
  columnWidths?: Record<string, number>;
  onColumnWidthsChange?: (widths: Record<string, number>) => void;
  selectedKeys?: string[];
  onSelectionChange?: (keys: string[]) => void;
  getRowKey?: (row: T, index: number) => string;
  bulkActions?: BulkAction[] | ((selectedKeys: string[]) => ReactNode);
  rowActions?: (row: T) => ReactNode;
  estimateRowHeight?: number;
  maxHeight?: number | string;
};

const DENSITY_PAD: Record<TableDensity, string> = {
  compact: 'var(--cos-density-compact)',
  comfortable: 'var(--cos-density-comfortable)',
  spacious: 'var(--cos-density-spacious)',
};

const VIRTUALIZE_THRESHOLD = 200;

function defaultRowKey<T>(_row: T, index: number) {
  return String(index);
}

export function Table<T>({
  columns,
  rows,
  empty,
  density = 'comfortable',
  sortKey,
  sortDir = 'asc',
  onSortChange,
  columnOrder,
  onColumnOrderChange,
  hiddenColumns = [],
  onHiddenColumnsChange,
  columnWidths,
  onColumnWidthsChange,
  selectedKeys,
  onSelectionChange,
  getRowKey = defaultRowKey,
  bulkActions,
  rowActions,
  estimateRowHeight = 44,
  maxHeight = 560,
}: TableProps<T>) {
  const pad = DENSITY_PAD[density];
  const selectable = Boolean(onSelectionChange && selectedKeys);
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [hoverRow, setHoverRow] = useState<string | null>(null);
  const [draggingCol, setDraggingCol] = useState<string | null>(null);
  const resizeRef = useRef<{ key: string; startX: number; startW: number } | null>(null);
  const parentRef = useRef<HTMLDivElement>(null);

  const orderedColumns = useMemo(() => {
    const order = columnOrder ?? columns.map((c) => c.key);
    const byKey = new Map(columns.map((c) => [c.key, c]));
    const visible = order
      .map((k) => byKey.get(k))
      .filter((c): c is Column<T> => Boolean(c) && !hiddenColumns.includes(c!.key));
    const left = visible.filter((c) => c.pin === 'left');
    const mid = visible.filter((c) => !c.pin);
    const right = visible.filter((c) => c.pin === 'right');
    return [...left, ...mid, ...right];
  }, [columns, columnOrder, hiddenColumns]);

  const keys = useMemo(() => rows.map((r, i) => getRowKey(r, i)), [rows, getRowKey]);
  const allSelected = selectable && rows.length > 0 && keys.every((k) => selectedKeys!.includes(k));
  const someSelected = selectable && keys.some((k) => selectedKeys!.includes(k)) && !allSelected;

  const toggleAll = () => {
    if (!onSelectionChange || !selectedKeys) return;
    onSelectionChange(allSelected ? [] : keys);
  };

  const toggleOne = (key: string) => {
    if (!onSelectionChange || !selectedKeys) return;
    onSelectionChange(
      selectedKeys.includes(key) ? selectedKeys.filter((k) => k !== key) : [...selectedKeys, key],
    );
  };

  const handleSort = (col: Column<T>) => {
    if (!col.sortable || !onSortChange) return;
    if (sortKey === col.key) {
      onSortChange(col.key, sortDir === 'asc' ? 'desc' : 'asc');
    } else {
      onSortChange(col.key, 'asc');
    }
  };

  const onResizeStart = (key: string, e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startW = columnWidths?.[key] ?? 140;
    resizeRef.current = { key, startX: e.clientX, startW };
    const onMove = (ev: MouseEvent) => {
      if (!resizeRef.current || !onColumnWidthsChange) return;
      const delta = ev.clientX - resizeRef.current.startX;
      const next = Math.max(60, resizeRef.current.startW + delta);
      onColumnWidthsChange({ ...columnWidths, [resizeRef.current.key]: next });
    };
    const onUp = () => {
      resizeRef.current = null;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  };

  const onHeaderDragStart = (key: string) => setDraggingCol(key);
  const onHeaderDrop = (targetKey: string) => {
    if (!draggingCol || !onColumnOrderChange || draggingCol === targetKey) {
      setDraggingCol(null);
      return;
    }
    const order = columnOrder ?? columns.map((c) => c.key);
    const next = order.filter((k) => k !== draggingCol);
    const idx = next.indexOf(targetKey);
    next.splice(idx < 0 ? next.length : idx, 0, draggingCol);
    onColumnOrderChange(next);
    setDraggingCol(null);
  };

  const shouldVirtualize = rows.length >= VIRTUALIZE_THRESHOLD;
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => estimateRowHeight,
    overscan: 8,
    enabled: shouldVirtualize,
  });

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      if (rows.length === 0) return;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setFocusedIndex((i) => Math.min(rows.length - 1, i + 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setFocusedIndex((i) => Math.max(0, i - 1));
      } else if (e.key === 'Home') {
        e.preventDefault();
        setFocusedIndex(0);
      } else if (e.key === 'End') {
        e.preventDefault();
        setFocusedIndex(rows.length - 1);
      } else if (e.key === ' ' && selectable) {
        e.preventDefault();
        toggleOne(keys[focusedIndex]!);
      }
    },
    [rows.length, selectable, keys, focusedIndex],
  );

  useEffect(() => {
    if (shouldVirtualize) {
      rowVirtualizer.scrollToIndex(focusedIndex, { align: 'auto' });
    }
  }, [focusedIndex, shouldVirtualize, rowVirtualizer]);

  if (rows.length === 0) {
    return <>{empty ?? null}</>;
  }

  const thStyle = (col: Column<T>): CSSProperties => ({
    textAlign: col.align ?? 'left',
    padding: pad,
    borderBottom: '1px solid var(--cos-color-border)',
    color: 'var(--cos-color-fg-muted)',
    fontWeight: 600,
    position: 'sticky',
    top: 0,
    zIndex: col.pin ? 'calc(var(--cos-z-sticky) + 1)' : 'var(--cos-z-sticky)',
    background: 'var(--cos-color-bg-elevated)',
    width: columnWidths?.[col.key] ?? col.width,
    minWidth: col.minWidth ?? 80,
    whiteSpace: 'nowrap',
    userSelect: 'none',
    left: col.pin === 'left' ? 0 : undefined,
    right: col.pin === 'right' ? 0 : undefined,
  });

  const tdStyle = (col: Column<T>): CSSProperties => ({
    textAlign: col.align ?? 'left',
    padding: pad,
    borderBottom: '1px solid var(--cos-color-border)',
    color: 'var(--cos-color-fg)',
    width: columnWidths?.[col.key] ?? col.width,
    minWidth: col.minWidth ?? 80,
    position: col.pin ? 'sticky' : undefined,
    left: col.pin === 'left' ? 0 : undefined,
    right: col.pin === 'right' ? 0 : undefined,
    background: col.pin ? 'var(--cos-color-bg-elevated)' : undefined,
    zIndex: col.pin ? 1 : undefined,
  });

  const renderHeader = () => (
    <tr>
      {selectable ? (
        <th style={{ ...thStyle({ key: '_sel', header: '', cell: () => null }), width: 40, minWidth: 40 }}>
          <Checkbox
            aria-label="Select all rows"
            checked={allSelected}
            onChange={toggleAll}
            style={{ gap: 0 }}
          />
          {someSelected ? (
            <span
              style={{
                position: 'absolute',
                width: 1,
                height: 1,
                overflow: 'hidden',
                clip: 'rect(0,0,0,0)',
              }}
            >
              Some rows selected
            </span>
          ) : null}
        </th>
      ) : null}
      {orderedColumns.map((col) => (
        <th
          key={col.key}
          draggable={Boolean(onColumnOrderChange)}
          onDragStart={() => onHeaderDragStart(col.key)}
          onDragOver={(e) => e.preventDefault()}
          onDrop={() => onHeaderDrop(col.key)}
          style={thStyle(col)}
          aria-sort={
            col.sortable && sortKey === col.key ? (sortDir === 'asc' ? 'ascending' : 'descending') : undefined
          }
        >
          <button
            type="button"
            onClick={() => handleSort(col)}
            disabled={!col.sortable}
            style={{
              all: 'unset',
              cursor: col.sortable ? 'pointer' : 'default',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 4,
              font: 'inherit',
              color: 'inherit',
            }}
          >
            {col.header}
            {col.sortable && sortKey === col.key ? (sortDir === 'asc' ? ' ↑' : ' ↓') : null}
          </button>
          {onColumnWidthsChange ? (
            <span
              role="separator"
              aria-orientation="vertical"
              onMouseDown={(e) => onResizeStart(col.key, e)}
              style={{
                position: 'absolute',
                right: 0,
                top: 0,
                bottom: 0,
                width: 6,
                cursor: 'col-resize',
              }}
            />
          ) : null}
          {col.hideable && onHiddenColumnsChange ? (
            <button
              type="button"
              aria-label={`Hide ${col.header}`}
              onClick={() => onHiddenColumnsChange([...hiddenColumns, col.key])}
              style={{
                all: 'unset',
                cursor: 'pointer',
                marginLeft: 6,
                opacity: 0.6,
                fontSize: '0.75rem',
              }}
            >
              ×
            </button>
          ) : null}
        </th>
      ))}
      {rowActions ? (
        <th style={{ ...thStyle({ key: '_actions', header: '', cell: () => null }), width: 1 }} />
      ) : null}
    </tr>
  );

  const renderRow = (row: T, index: number, style?: CSSProperties) => {
    const key = keys[index]!;
    const selected = selectedKeys?.includes(key);
    const focused = focusedIndex === index;
    return (
      <tr
        key={key}
        tabIndex={-1}
        data-focused={focused || undefined}
        onMouseEnter={() => setHoverRow(key)}
        onMouseLeave={() => setHoverRow((h) => (h === key ? null : h))}
        onClick={() => setFocusedIndex(index)}
        style={{
          background: selected
            ? 'var(--cos-color-accent-muted)'
            : focused
              ? 'var(--cos-color-bg-muted)'
              : undefined,
          outline: focused ? '2px solid var(--cos-color-focus-ring)' : undefined,
          outlineOffset: -2,
          ...style,
        }}
      >
        {selectable ? (
          <td style={{ padding: pad, borderBottom: '1px solid var(--cos-color-border)', width: 40 }}>
            <Checkbox
              aria-label={`Select row ${index + 1}`}
              checked={Boolean(selected)}
              onChange={() => toggleOne(key)}
              style={{ gap: 0 }}
            />
          </td>
        ) : null}
        {orderedColumns.map((col) => (
          <td key={col.key} style={tdStyle(col)}>
            {col.cell(row)}
          </td>
        ))}
        {rowActions ? (
          <td
            style={{
              padding: pad,
              borderBottom: '1px solid var(--cos-color-border)',
              whiteSpace: 'nowrap',
              opacity: hoverRow === key || focused ? 1 : 0,
            }}
          >
            {rowActions(row)}
          </td>
        ) : null}
      </tr>
    );
  };

  const selectedCount = selectedKeys?.length ?? 0;

  return (
    <div style={{ fontFamily: 'var(--cos-font-sans)', fontSize: '0.9rem' }}>
      {selectable && selectedCount > 0 ? (
        <div
          role="toolbar"
          aria-label="Bulk actions"
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 'var(--cos-space-3)',
            padding: 'var(--cos-space-2) var(--cos-space-3)',
            marginBottom: 'var(--cos-space-2)',
            background: 'var(--cos-color-accent-muted)',
            borderRadius: 'var(--cos-radius-sm)',
            color: 'var(--cos-color-fg)',
          }}
        >
          <span style={{ fontWeight: 600 }}>{selectedCount} selected</span>
          {typeof bulkActions === 'function'
            ? bulkActions(selectedKeys!)
            : bulkActions?.map((a) => (
                <button
                  key={a.id}
                  type="button"
                  onClick={() => a.onSelect(selectedKeys!)}
                  style={{
                    fontFamily: 'inherit',
                    fontWeight: 600,
                    fontSize: '0.8125rem',
                    padding: '0.3rem 0.6rem',
                    borderRadius: 'var(--cos-radius-sm)',
                    border: '1px solid var(--cos-color-border)',
                    background: a.danger ? 'var(--cos-color-danger)' : 'var(--cos-color-bg-elevated)',
                    color: a.danger ? 'var(--cos-color-danger-fg)' : 'var(--cos-color-fg)',
                    cursor: 'pointer',
                  }}
                >
                  {a.label}
                </button>
              ))}
        </div>
      ) : null}

      <div
        ref={parentRef}
        tabIndex={0}
        onKeyDown={onKeyDown}
        data-virtualized={shouldVirtualize ? 'true' : 'false'}
        style={{
          overflow: 'auto',
          maxHeight,
          border: '1px solid var(--cos-color-border)',
          borderRadius: 'var(--cos-radius-sm)',
        }}
      >
        {shouldVirtualize ? (
          <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative', width: '100%' }}>
            <table
              aria-rowcount={rows.length}
              style={{
                width: '100%',
                borderCollapse: 'collapse',
                tableLayout: 'fixed',
              }}
            >
              <thead>{renderHeader()}</thead>
            </table>
            {rowVirtualizer.getVirtualItems().map((vRow) => {
              const row = rows[vRow.index]!;
              return (
                <div
                  key={keys[vRow.index]}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${vRow.start}px)`,
                  }}
                >
                  <table
                    aria-hidden
                    style={{ width: '100%', borderCollapse: 'collapse', tableLayout: 'fixed' }}
                  >
                    <tbody>{renderRow(row, vRow.index)}</tbody>
                  </table>
                </div>
              );
            })}
          </div>
        ) : (
          <table aria-rowcount={rows.length} style={{ width: '100%', borderCollapse: 'collapse' }}>
            <thead>{renderHeader()}</thead>
            <tbody>{rows.map((row, i) => renderRow(row, i))}</tbody>
          </table>
        )}
      </div>
    </div>
  );
}

export {
  MoneyCell,
  DateCell,
  StatusCell,
  AvatarCell,
  LinkCell,
} from './TableCells';
