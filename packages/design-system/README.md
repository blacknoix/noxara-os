# @companyos/design-system

CompanyOS UI primitives. Sentence case labels; buttons are verbs. Teal accent (`#0f6b5c`) on warm paper tokens — Sora + Fraunces.

**Storybook equivalent:** browse components at `/dev/components` in `apps/web`.

## Setup

```ts
import '@companyos/design-system/tokens.css';
import '@companyos/design-system/styles.css';
```

Themes via `data-theme="light" | "dark" | "high-contrast"` on a root element. Tokens honour `prefers-color-scheme` and `prefers-reduced-motion`.

## Tokens & styles

| Export | Purpose |
|--------|---------|
| `./tokens.css` | Colour, density, shell widths (sidebar 220 / collapsed 64, topbar 56, context 380), z-index, motion |
| `./styles.css` | Focus-visible rings, skip-link, reduced-motion helpers |

## Inputs

| Component | Key props | A11y / keyboard |
|-----------|-----------|-----------------|
| **Button** | `variant` primary/secondary/ghost/danger, `size` sm/md, `loading`, `leftIcon` | Native button; disabled when loading |
| **Input** | `label`, `hint`, `error` + native input props | Label association; `aria-invalid` on error |
| **Select** | `options`, `label`, `placeholder` | Native `<select>` |
| **Textarea** | `label`, `hint`, `error` | Native textarea |
| **Checkbox** | `label?`, `description`, `aria-label` | Label or aria-label required |
| **Radio** / **RadioGroup** | `name`, `options`, `value`, `onChange` | Fieldset + legend |
| **Switch** | `checked`, `onCheckedChange`, `label` | `role="switch"`, Space/Enter |
| **DatePicker** | native `type="date"` wrapper | Browser date picker |
| **FileUpload** | `onFiles`, `accept`, `multiple` | Dropzone; Enter/Space opens file dialog |

## Data

### Table

Props: `columns`, `rows`, `empty`, `density` (compact/comfortable/spacious), `sortKey`/`sortDir`/`onSortChange`, `columnOrder`/`onColumnOrderChange`, `hiddenColumns`/`onHiddenColumnsChange`, `columnWidths`/`onColumnWidthsChange`, `selectedKeys`/`onSelectionChange`/`getRowKey`, `bulkActions`, `rowActions`, virtualises at ≥200 rows.

Column: `{ key, header, cell, width?, minWidth?, sortable?, pin?, hideable?, align? }` — simple `{ key, header, cell }` still works.

Keyboard: focus the grid, Arrow Up/Down (Home/End), Space toggles selection.

Cell renderers: `MoneyCell`, `DateCell`, `StatusCell`, `AvatarCell`, `LinkCell`.

### FilterBar

Chip filters with grammar `field operator value`. Operators: `is`, `is_not`, `contains`, `gt`, `lt`, `between`, `empty`. Free-text `q`, save/update view, clear all.

### savedViews

`parseViewFromSearchParams` / `viewToSearchParams` — serialise filters, sort, columns, density, `q` under `view`, `q`, `f`.

## Layout & display

| Component | Notes |
|-----------|-------|
| **Card** | Bordered surface when interaction needs a container |
| **StatTile** | `label`, `value`, optional `hint`/`trend` |
| **Widget** | `title`, `range`, `menu`, `body`/`footer`, loading/empty/error slots |
| **List** | `items` with leading/trailing; optional `onItemSelect` |
| **Timeline** | Vertical structure |
| **KanbanBoard** | Column/card stubs (no DnD) |
| **DetailPanel** | Slide-over; Escape closes |
| **Chart** | Empty slot wrapper — pass chart lib as children |

## Misc

**Badge**, **Tag**, **Avatar**, **ProgressBar**, **Tooltip** (hover/focus), **Popover** (Escape + outside click), **Skeleton**.

## Overlay

| Component | Notes |
|-----------|-------|
| **Modal** | Focus trap; Escape closes. **Do not nest >1 modal.** |
| **Drawer** | Left/right sheet |
| **ConfirmDialog** | Modal + confirm/cancel verbs |
| **ToastProvider** / **useToast** | Undo slot; max 3 toasts |
| **InlineAlert** | Inline status |
| **Banner** | Full-width announcement |

## Nav

| Component | Keyboard |
|-----------|----------|
| **Breadcrumb** | Links / buttons |
| **Tabs** | Arrow Left/Right |
| **CursorPagination** | Load more |
| **Stepper** | Visual progress |
| **CommandBar** | ↑↓ Enter Esc; filter by query |

## States

| Component | Props |
|-----------|-------|
| **EmptyState** | `title`, `description?`, `action?` |
| **ErrorState** | `message`, `requestId?` |
| **PermissionDeniedState** | `requiredPermission` |
| **LoadingState** | Skeleton rows; polite live region |
| **StaleDataState** | `asOf`, `onRefresh` |

## Product rules

1. Sentence case everywhere; buttons are verbs.
2. Designed states: loading, empty, error (+ request id), permission-denied (+ permission), stale (as-of + refresh).
3. WCAG 2.2 AA — focus rings, labels, contrast themes, reduced motion.
4. One table/filter grammar across the product.
