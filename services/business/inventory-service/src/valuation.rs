//! Weighted-average inventory valuation (see `docs/adrs/023-inventory-valuation-wavg.md`).
//!
//! Pure math only — no DB access. [`crate::stock`] owns the transaction /
//! ledger orchestration and calls into this module for the arithmetic.
//!
//! - **Receipt** (qty > 0 arriving at a known unit cost): blend into the
//!   existing average — `new_avg = (on_hand*avg + qty*unit_cost) / (on_hand+qty)`.
//! - **Issue** (qty leaving): valued at the *current* average — never changes
//!   the average itself. `cogs_minor = qty * avg_unit_cost_minor`.
//!
//! All rounding is half-up via `companyos_money::Money::round_half_up` — no
//! floating point anywhere on this path.

use companyos_money::Money;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValuationError {
    #[error("quantity must be positive")]
    InvalidQuantity,
    #[error("arithmetic overflow in valuation calculation")]
    Overflow,
}

/// Blend a receipt of `receipt_qty` units at `receipt_unit_cost_minor` into
/// the existing `(on_hand_qty, on_hand_avg_minor)` position.
///
/// Returns `(new_qty_on_hand, new_avg_unit_cost_minor)`. `on_hand_qty` may be
/// negative (backorder covered by this receipt) — the blend still holds
/// because it operates on total *value*, not the (possibly negative) qty
/// alone; when the resulting position nets to exactly zero the average is
/// left unchanged (nothing to divide by, and there is no remaining position
/// to mis-value).
pub fn weighted_average_receipt(
    on_hand_qty: i64,
    on_hand_avg_minor: i64,
    receipt_qty: i64,
    receipt_unit_cost_minor: i64,
) -> Result<(i64, i64), ValuationError> {
    if receipt_qty <= 0 {
        return Err(ValuationError::InvalidQuantity);
    }
    let new_qty = on_hand_qty
        .checked_add(receipt_qty)
        .ok_or(ValuationError::Overflow)?;
    if new_qty == 0 {
        return Ok((0, on_hand_avg_minor));
    }
    let existing_value = (on_hand_qty as i128) * (on_hand_avg_minor as i128);
    let receipt_value = (receipt_qty as i128) * (receipt_unit_cost_minor as i128);
    let total_value = existing_value
        .checked_add(receipt_value)
        .ok_or(ValuationError::Overflow)?;
    let new_avg =
        Money::round_half_up(total_value, new_qty as i128).map_err(|_| ValuationError::Overflow)?;
    Ok((new_qty, new_avg))
}

/// Cost of goods sold for issuing `qty` units at the current average cost.
/// The average itself is unaffected by an issue.
pub fn issue_cost_minor(qty: i64, avg_unit_cost_minor: i64) -> Result<i64, ValuationError> {
    if qty <= 0 {
        return Err(ValuationError::InvalidQuantity);
    }
    qty.checked_mul(avg_unit_cost_minor)
        .ok_or(ValuationError::Overflow)
}

/// Straight-line depreciation expense for one period, capped so accumulated
/// depreciation never exceeds `acquisition_cost_minor - salvage_minor`.
///
/// `months_elapsed` is the number of whole months since `last_depreciated_at`
/// (or acquisition) through the requested `as_of` date.
pub fn straight_line_depreciation_minor(
    acquisition_cost_minor: i64,
    salvage_minor: i64,
    useful_life_months: i32,
    accumulated_depreciation_minor: i64,
    months_elapsed: i32,
) -> Result<i64, ValuationError> {
    if useful_life_months <= 0 || months_elapsed <= 0 {
        return Ok(0);
    }
    let depreciable_base = acquisition_cost_minor
        .checked_sub(salvage_minor)
        .ok_or(ValuationError::Overflow)?
        .max(0);
    let monthly = Money::round_half_up(depreciable_base as i128, useful_life_months as i128)
        .map_err(|_| ValuationError::Overflow)?;
    let raw = monthly
        .checked_mul(i64::from(months_elapsed))
        .ok_or(ValuationError::Overflow)?;
    let remaining = depreciable_base
        .checked_sub(accumulated_depreciation_minor)
        .unwrap_or(0)
        .max(0);
    Ok(raw.min(remaining))
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)] // grouped as {major}_{minor} for readability
mod tests {
    use super::*;

    #[test]
    fn first_receipt_sets_avg_to_unit_cost() {
        let (qty, avg) = weighted_average_receipt(0, 0, 10, 100).unwrap();
        assert_eq!(qty, 10);
        assert_eq!(avg, 100);
    }

    #[test]
    fn second_receipt_blends_average() {
        let (qty, avg) = weighted_average_receipt(10, 100, 10, 200).unwrap();
        assert_eq!(qty, 20);
        // (10*100 + 10*200) / 20 = 150
        assert_eq!(avg, 150);
    }

    #[test]
    fn uneven_blend_rounds_half_up() {
        // (3*100 + 1*101) / 4 = 401/4 = 100.25 -> rounds to 100
        let (qty, avg) = weighted_average_receipt(3, 100, 1, 101).unwrap();
        assert_eq!(qty, 4);
        assert_eq!(avg, 100);

        // (1*100 + 1*101) / 2 = 201/2 = 100.5 -> rounds away from zero to 101
        let (qty2, avg2) = weighted_average_receipt(1, 100, 1, 101).unwrap();
        assert_eq!(qty2, 2);
        assert_eq!(avg2, 101);
    }

    #[test]
    fn issue_uses_current_average_and_never_changes_it() {
        let cogs = issue_cost_minor(5, 150).unwrap();
        assert_eq!(cogs, 750);
    }

    #[test]
    fn issue_rejects_non_positive_qty() {
        assert_eq!(
            issue_cost_minor(0, 100).unwrap_err(),
            ValuationError::InvalidQuantity
        );
        assert_eq!(
            issue_cost_minor(-1, 100).unwrap_err(),
            ValuationError::InvalidQuantity
        );
    }

    #[test]
    fn receipt_rejects_non_positive_qty() {
        assert_eq!(
            weighted_average_receipt(0, 0, 0, 100).unwrap_err(),
            ValuationError::InvalidQuantity
        );
    }

    #[test]
    fn receipt_covering_backorder_nets_correctly() {
        // On hand is -5 (backorder allowed) valued at avg 100 (from before it
        // went negative); a receipt of 5 @ 120 brings it back to 0.
        let (qty, avg) = weighted_average_receipt(-5, 100, 5, 120).unwrap();
        assert_eq!(qty, 0);
        // Average is irrelevant at qty 0 but must not panic / divide by zero.
        assert_eq!(avg, 100);
    }

    #[test]
    fn straight_line_depreciation_basic() {
        // 36-month life, 3_600_00 minor acquisition, 0 salvage -> 10_000/mo.
        let dep = straight_line_depreciation_minor(3_600_00, 0, 36, 0, 1).unwrap();
        assert_eq!(dep, 10_000);
    }

    #[test]
    fn straight_line_depreciation_caps_at_depreciable_base() {
        // monthly = 3_600_00 / 36 = 10_000; 5 months would be 50_000, but only
        // 1_000 remains of the depreciable base (3_600_00 - 3_590_00), so the
        // result is capped there instead of over-depreciating the asset.
        let dep = straight_line_depreciation_minor(3_600_00, 0, 36, 3_590_00, 5).unwrap();
        assert_eq!(dep, 1_000);
    }

    #[test]
    fn straight_line_depreciation_respects_salvage() {
        // base = 3_600_00 - 600_00 = 3_000_00 over 36mo = 8_333.33 -> rounds to 8_333
        let dep = straight_line_depreciation_minor(3_600_00, 600_00, 36, 0, 1).unwrap();
        assert_eq!(dep, 8_333);
    }
}
