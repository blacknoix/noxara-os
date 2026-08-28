# ADR 023: Inventory valuation — Weighted Average (Phase 2.5)

- Status: **Accepted**
- Date: 2026-08-28

## Context

Phase 2.5 adds `companyos-inventory`: warehouses, items, stock movements, and a
procure-to-pay flow (purchase request → purchase order → goods receipt) that must
cost every unit issued from stock and post the matching journal to Finance
(Dr COGS / Cr Inventory). We need one inventory costing method for v1. The usual
options are FIFO, LIFO, Standard Cost, and Weighted Average (moving average).

Constraints that shaped the choice:

- `inventory_stock_movement` is already designed as an **append-only ledger** (the
  source of truth) with `inventory_stock_level` as a derived cache — this favors a
  method that can be computed incrementally, one movement at a time, without
  replaying cost layers.
- Every receipt and issue happens inside a single DB transaction that also updates
  the cache and (for issues) posts a journal to Finance; the costing math must be
  cheap, deterministic, and side-effect-free (pure functions, unit-testable without
  a database).
- Money is `amount_minor: i64` — no floating point on the posting path.

## Decision

**Weighted Average (moving average) cost**, computed per `(warehouse_id, item_id)`
and stored as `inventory_stock_level.avg_unit_cost_minor`:

1. **Receipt** (`qty_delta > 0`, from a goods receipt, return, or transfer-in) blends
   into the existing position by total *value*, not by layer:

   ```text
   new_avg = (qty_on_hand * avg_unit_cost_minor + receipt_qty * receipt_unit_cost_minor)
             / (qty_on_hand + receipt_qty)
   ```

   Rounding is half-up via `companyos_money::Money::round_half_up` on `i128`
   intermediates (`weighted_average_receipt` in `src/valuation.rs`) — no floats. If
   the resulting quantity is exactly zero (a receipt exactly covering a negative/backorder
   position), the average is left unchanged rather than dividing by zero.

2. **Issue** (`qty_delta < 0`, from an issue or transfer-out) is costed at the
   *current* average and does **not** change it:

   ```text
   cogs_minor = issued_qty * avg_unit_cost_minor
   ```

   (`issue_cost_minor` in `src/valuation.rs`). The issuing movement carries the COGS
   amount back to the caller, which posts Dr COGS (`5200`) / Cr Inventory (`1200`) to
   Finance over HTTP — inventory-service never writes `finance_*` tables.

3. **Adjustment** movements (either sign, e.g. a manual correction) are treated as a
   receipt when increasing quantity and an issue when decreasing it, using the same
   two functions — there is no third code path.

4. `inventory_stock_level` is a **cache**, updated in the same transaction as every
   movement insert. [`stock::reconcile_stock`] independently sums
   `inventory_stock_movement.qty_delta` for a `(warehouse_id, item_id)` pair and
   compares it to the cached `qty_on_hand`. On mismatch it inserts an
   `inventory_drift_alert` row plus a `stock.drift_detected` outbox event — **it
   never silently rewrites the cache**. An operator (or a follow-up adjustment
   movement) must resolve the drift explicitly; this keeps the cache/ledger
   relationship auditable instead of self-healing in a way that could mask a bug.

5. **Negative stock** is a per-item policy flag (`allow_negative_stock`), not a
   costing decision: a movement that would drive `qty_on_hand` negative is rejected
   with `409 Conflict` unless the item explicitly opts in (e.g. for backorder-style
   flows). When negative stock is allowed, a subsequent receipt still blends
   correctly per (1) above, including the zero-crossing case.

## Alternatives considered

- **FIFO / LIFO** — require tracking discrete cost layers per receipt and consuming
  them in order on issue. This is a natural fit for a lot/serial-tracked system but
  adds a `inventory_cost_layer`-style table and materially more complexity for v1,
  where warehouses hold fungible, non-serialized stock. Deferred; the append-only
  movement ledger does retain enough history (`unit_cost_minor` per movement) to
  backfill FIFO/LIFO layers in a later phase if needed.
- **Standard Cost** — requires a separate standard-cost-setting workflow and
  variance accounts; out of scope until manufacturing/BOM costing is on the roadmap.
- **Rewriting the cache on drift** — rejected because it would hide the root cause of
  a mismatch (e.g. a concurrent-write bug, or a movement inserted outside
  `stock::post_movement`) instead of surfacing it for investigation.

## Consequences

- One costing method, one pair of pure functions (`weighted_average_receipt`,
  `issue_cost_minor`), fully unit-tested without a database — `stock::post_movement`
  is the only caller and owns the transaction/ledger orchestration.
- Every issue's Dr COGS / Cr Inventory journal amount is exactly
  `issued_qty * avg_unit_cost_minor` at the moment of issue, so it is reproducible
  from the movement row alone (`qty_delta`, and the stock level's average at that
  point in time via replay of the ledger) — useful for audit and for the
  `valuation journal` DoD test.
- A future FIFO/LIFO costing method would need a new `inventory_cost_layer` table
  and a different `post_movement` code path; this ADR does not block that, but it is
  explicitly out of scope for Phase 2.5.
- `reconcile_stock` / `reconcile_all` are the only supported way to detect
  cache/ledger drift; there is intentionally no "auto-heal" endpoint.
