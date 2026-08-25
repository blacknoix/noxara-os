# ADR 018: Table virtualisation with TanStack Virtual

- Status: **Accepted**
- Date: 2026-08-25

## Context

Phase 1.3 requires one product-wide `Table` that stays usable for large
collections (≥200 rows, target ~10k without jank). CompanyOS already owns
`@companyos/design-system`; we need a virtualisation strategy that upgrades
that package in place rather than adopting a second data-grid product.

Full grid libraries (AG Grid, TanStack Table + heavy adapters) bring large
bundles, opinionated styling, and a parallel interaction model that fights the
“one table / one filter grammar” invariant.

## Decision

Use **`@tanstack/react-virtual`** inside `packages/design-system` `Table` when
`rows.length >= 200`. Below that threshold, render a normal DOM table for
simpler a11y and debugging.

Column show/hide/reorder/resize/pin, sort, multi-select, density, keyboard
navigation, and URL-bound saved views remain first-party in the design system.
We intentionally do **not** depend on TanStack Table for column state in v1.

## Consequences

- Small dependency focused on windowing only.
- Sticky headers + virtualised body need careful layout (already handled in
  `Table.tsx`); smoke-tested in `apps/web` vitest.
- If a future slice needs spreadsheet editing, re-evaluate; prefer extending
  this Table before adopting a competing grid.
