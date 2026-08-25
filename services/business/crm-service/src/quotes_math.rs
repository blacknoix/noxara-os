//! Pure quote-total arithmetic on top of `companyos_money`.
//!
//! **No floats.** Every amount is `i64` minor units. Tax is expressed in
//! basis points (`tax_rate_bps`; 1000 == 10.00%). Document totals are the
//! exact sum of line totals — we never re-derive them from a rounded
//! percentage of the subtotal, so line/document totals always agree.

use companyos_money::{Currency, Money, MoneyError};

/// One quote line as persisted (before recompute).
#[derive(Debug, Clone, Copy)]
pub struct LineInput {
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub discount_minor: i64,
    pub tax_rate_bps: i64,
}

/// Computed totals for a single line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineTotals {
    /// quantity * unit_price_minor
    pub gross_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    /// gross - discount + tax
    pub line_total_minor: i64,
}

/// Compute one line's totals. Tax is applied to the post-discount base.
pub fn compute_line(line: LineInput, currency: Currency) -> Result<LineTotals, MoneyError> {
    let gross = Money::new(
        line.quantity
            .checked_mul(line.unit_price_minor)
            .ok_or(MoneyError::Overflow)?,
        currency,
    );
    let discount = Money::new(line.discount_minor, currency);
    let taxable_base = gross.checked_sub(discount)?;
    // tax_rate_bps is parts-per-10000.
    let tax = taxable_base.scale_half_up(line.tax_rate_bps, 10_000)?;
    let line_total = taxable_base.checked_add(tax)?;
    Ok(LineTotals {
        gross_minor: gross.amount_minor,
        discount_minor: discount.amount_minor,
        tax_minor: tax.amount_minor,
        line_total_minor: line_total.amount_minor,
    })
}

/// Document-level totals: the exact sum of line totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentTotals {
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
}

/// Sum computed line totals into document totals.
///
/// Invariant asserted by `quote_money_totals_sum` test: for every line,
/// `gross - discount + tax == line_total`, and the document total equals
/// the sum of line totals exactly (integer arithmetic, no re-rounding).
pub fn sum_document(lines: &[LineTotals]) -> Result<DocumentTotals, MoneyError> {
    let mut subtotal: i64 = 0;
    let mut discount: i64 = 0;
    let mut tax: i64 = 0;
    let mut total: i64 = 0;
    for l in lines {
        subtotal = subtotal
            .checked_add(l.gross_minor)
            .ok_or(MoneyError::Overflow)?;
        discount = discount
            .checked_add(l.discount_minor)
            .ok_or(MoneyError::Overflow)?;
        tax = tax.checked_add(l.tax_minor).ok_or(MoneyError::Overflow)?;
        total = total
            .checked_add(l.line_total_minor)
            .ok_or(MoneyError::Overflow)?;
    }
    Ok(DocumentTotals {
        subtotal_minor: subtotal,
        discount_minor: discount,
        tax_minor: tax,
        total_minor: total,
    })
}

/// Convenience: compute all line totals + document totals together.
pub fn compute_quote_totals(
    lines: &[LineInput],
    currency: Currency,
) -> Result<(Vec<LineTotals>, DocumentTotals), MoneyError> {
    let computed: Vec<LineTotals> = lines
        .iter()
        .map(|l| compute_line(*l, currency))
        .collect::<Result<_, _>>()?;
    let doc = sum_document(&computed)?;
    Ok((computed, doc))
}

/// Allocate a document-level discount across lines proportionally to their
/// gross amount, using the largest-remainder method so the parts sum exactly
/// to the requested discount.
pub fn allocate_document_discount(
    line_gross_minor: &[i64],
    document_discount_minor: i64,
    currency: Currency,
) -> Result<Vec<i64>, MoneyError> {
    if document_discount_minor == 0 || line_gross_minor.is_empty() {
        return Ok(vec![0; line_gross_minor.len()]);
    }
    let weights: Vec<u64> = line_gross_minor
        .iter()
        .map(|g| (*g).max(0) as u64)
        .collect();
    if weights.iter().all(|w| *w == 0) {
        return Ok(vec![0; line_gross_minor.len()]);
    }
    let parts = companyos_money::allocate_largest_remainder(
        Money::new(document_discount_minor, currency),
        &weights,
    )?;
    Ok(parts.into_iter().map(|m| m.amount_minor).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> Currency {
        Currency::USD
    }

    #[test]
    fn line_total_no_discount_no_tax() {
        let l = LineInput {
            quantity: 3,
            unit_price_minor: 1000,
            discount_minor: 0,
            tax_rate_bps: 0,
        };
        let t = compute_line(l, usd()).unwrap();
        assert_eq!(t.gross_minor, 3000);
        assert_eq!(t.tax_minor, 0);
        assert_eq!(t.line_total_minor, 3000);
    }

    #[test]
    fn line_total_with_discount_and_tax() {
        // 2 * 5000 = 10000 gross; discount 1000 -> taxable 9000; tax 10% (1000bps) = 900
        let l = LineInput {
            quantity: 2,
            unit_price_minor: 5000,
            discount_minor: 1000,
            tax_rate_bps: 1000,
        };
        let t = compute_line(l, usd()).unwrap();
        assert_eq!(t.gross_minor, 10_000);
        assert_eq!(t.discount_minor, 1000);
        assert_eq!(t.tax_minor, 900);
        assert_eq!(t.line_total_minor, 9900);
    }

    #[test]
    fn line_total_rounds_half_up() {
        // taxable base 101, tax rate 550 bps (5.5%) -> 5.555 -> rounds to 6
        let l = LineInput {
            quantity: 1,
            unit_price_minor: 101,
            discount_minor: 0,
            tax_rate_bps: 550,
        };
        let t = compute_line(l, usd()).unwrap();
        assert_eq!(t.tax_minor, 6);
        assert_eq!(t.line_total_minor, 107);
    }

    #[test]
    fn document_totals_sum_lines_exactly() {
        let lines = [
            LineInput {
                quantity: 2,
                unit_price_minor: 1999,
                discount_minor: 0,
                tax_rate_bps: 875,
            },
            LineInput {
                quantity: 1,
                unit_price_minor: 4999,
                discount_minor: 500,
                tax_rate_bps: 0,
            },
            LineInput {
                quantity: 5,
                unit_price_minor: 101,
                discount_minor: 0,
                tax_rate_bps: 1000,
            },
        ];
        let (computed, doc) = compute_quote_totals(&lines, usd()).unwrap();
        let sum_line_totals: i64 = computed.iter().map(|l| l.line_total_minor).sum();
        assert_eq!(sum_line_totals, doc.total_minor);
        // gross - discount + tax == total, per line and aggregated.
        for l in &computed {
            assert_eq!(
                l.gross_minor - l.discount_minor + l.tax_minor,
                l.line_total_minor
            );
        }
        assert_eq!(
            doc.subtotal_minor - doc.discount_minor + doc.tax_minor,
            doc.total_minor
        );
    }

    #[test]
    fn allocate_document_discount_sums_exactly() {
        let gross = [10_000i64, 20_000, 5_000];
        let parts = allocate_document_discount(&gross, 999, usd()).unwrap();
        assert_eq!(parts.iter().sum::<i64>(), 999);
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn allocate_document_discount_zero_is_all_zero() {
        let gross = [1_000i64, 2_000];
        let parts = allocate_document_discount(&gross, 0, usd()).unwrap();
        assert_eq!(parts, vec![0, 0]);
    }

    #[test]
    fn negative_discount_beyond_gross_is_rejected_by_caller_not_here() {
        // Pure function does not clamp; overflow/negatives are a caller-level validation concern.
        let l = LineInput {
            quantity: 1,
            unit_price_minor: 100,
            discount_minor: 1000,
            tax_rate_bps: 0,
        };
        let t = compute_line(l, usd()).unwrap();
        assert_eq!(t.line_total_minor, -900);
    }
}
