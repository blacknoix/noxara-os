//! Pure invoice-line arithmetic on top of `companyos_money`.
//!
//! **No floats.** Every amount is `i64` minor units. Tax is expressed in
//! basis points (`tax_rate_bps`; 1000 == 10.00%). Document totals are the
//! exact sum of line totals.

use companyos_money::{Currency, Money, MoneyError};

#[derive(Debug, Clone, Copy)]
pub struct LineInput {
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub discount_minor: i64,
    pub tax_rate_bps: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineTotals {
    pub gross_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub line_total_minor: i64,
}

pub fn compute_line(line: LineInput, currency: Currency) -> Result<LineTotals, MoneyError> {
    let gross = Money::new(
        line.quantity
            .checked_mul(line.unit_price_minor)
            .ok_or(MoneyError::Overflow)?,
        currency,
    );
    let discount = Money::new(line.discount_minor, currency);
    let taxable_base = gross.checked_sub(discount)?;
    let tax = taxable_base.scale_half_up(line.tax_rate_bps, 10_000)?;
    let line_total = taxable_base.checked_add(tax)?;
    Ok(LineTotals {
        gross_minor: gross.amount_minor,
        discount_minor: discount.amount_minor,
        tax_minor: tax.amount_minor,
        line_total_minor: line_total.amount_minor,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentTotals {
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
}

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

pub fn compute_document_totals(
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

/// Convert document currency total to base currency using rational FX rate
/// with half-up rounding at the document total.
pub fn convert_to_base(
    amount_minor: i64,
    fx_rate_num: i64,
    fx_rate_den: i64,
    base_currency: Currency,
) -> Result<i64, MoneyError> {
    if fx_rate_den <= 0 || fx_rate_num <= 0 {
        return Err(MoneyError::Overflow);
    }
    Money::new(amount_minor, base_currency)
        .scale_half_up(fx_rate_num, fx_rate_den)
        .map(|m| m.amount_minor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn line_and_document_identity() {
        let lines = [
            LineInput {
                quantity: 2,
                unit_price_minor: 1999,
                discount_minor: 100,
                tax_rate_bps: 1000,
            },
            LineInput {
                quantity: 1,
                unit_price_minor: 500,
                discount_minor: 0,
                tax_rate_bps: 550,
            },
        ];
        let (computed, doc) = compute_document_totals(&lines, Currency::USD).unwrap();
        let sum: i64 = computed.iter().map(|l| l.line_total_minor).sum();
        assert_eq!(sum, doc.total_minor);
        for l in &computed {
            assert_eq!(
                l.gross_minor - l.discount_minor + l.tax_minor,
                l.line_total_minor
            );
        }
    }

    #[test]
    fn half_up_tax_matrix() {
        // 101 * 5.5% = 5.555 -> 6
        let t = compute_line(
            LineInput {
                quantity: 1,
                unit_price_minor: 101,
                discount_minor: 0,
                tax_rate_bps: 550,
            },
            Currency::USD,
        )
        .unwrap();
        assert_eq!(t.tax_minor, 6);
    }

    #[test]
    fn fx_convert_half_up() {
        // 10000 * 11/10 = 11000
        assert_eq!(
            convert_to_base(10_000, 11, 10, Currency::USD).unwrap(),
            11_000
        );
        // 100 * 3/2 = 150
        assert_eq!(convert_to_base(100, 3, 2, Currency::USD).unwrap(), 150);
    }

    proptest! {
        #[test]
        fn prop_lines_sum_to_document(
            q0 in 1i64..20,
            p0 in 1i64..10_000,
            d0 in 0i64..500,
            t0 in 0i64..2500,
            q1 in 1i64..20,
            p1 in 1i64..10_000,
            d1 in 0i64..500,
            t1 in 0i64..2500,
        ) {
            prop_assume!(d0 <= q0 * p0);
            prop_assume!(d1 <= q1 * p1);
            let lines = [
                LineInput { quantity: q0, unit_price_minor: p0, discount_minor: d0, tax_rate_bps: t0 },
                LineInput { quantity: q1, unit_price_minor: p1, discount_minor: d1, tax_rate_bps: t1 },
            ];
            let (computed, doc) = compute_document_totals(&lines, Currency::EUR).unwrap();
            let sum: i64 = computed.iter().map(|l| l.line_total_minor).sum();
            prop_assert_eq!(sum, doc.total_minor);
            prop_assert_eq!(
                doc.subtotal_minor - doc.discount_minor + doc.tax_minor,
                doc.total_minor
            );
        }
    }
}
