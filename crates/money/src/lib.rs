//! Money: integer minor units + ISO 4217 currency.
//!
//! **Never use `f64` on the finance path.** All amounts are `amount_minor: i64`.
//! Rounding for document totals uses half-up (away from zero on .5).
//! Allocation across lines uses the largest-remainder method.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from money operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MoneyError {
    #[error("currency mismatch: {left} vs {right}")]
    CurrencyMismatch { left: Currency, right: Currency },
    #[error("overflow in money arithmetic")]
    Overflow,
    #[error("invalid currency code: {0}")]
    InvalidCurrency(String),
    #[error("allocation weights must be non-negative and sum > 0")]
    InvalidAllocationWeights,
    #[error("negative decimal places for currency")]
    InvalidExponent,
}

/// ISO 4217 currency code (3-letter alphabetic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Currency(pub [u8; 3]);

impl Currency {
    pub const USD: Self = Self(*b"USD");
    pub const EUR: Self = Self(*b"EUR");
    pub const GBP: Self = Self(*b"GBP");
    pub const JPY: Self = Self(*b"JPY");

    pub fn new(code: &str) -> Result<Self, MoneyError> {
        let b = code.as_bytes();
        if b.len() != 3 || !b.iter().all(|c| c.is_ascii_uppercase()) {
            return Err(MoneyError::InvalidCurrency(code.to_string()));
        }
        Ok(Self([b[0], b[1], b[2]]))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("currency is ascii")
    }

    /// Default minor-unit exponent (decimal places). JPY/KRW-like = 0; most = 2.
    pub fn default_exponent(self) -> u8 {
        match self.as_str() {
            "JPY" | "KRW" | "VND" | "CLP" => 0,
            "BHD" | "KWD" | "OMR" => 3,
            _ => 2,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Currency {
    type Err = MoneyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// A monetary amount in integer minor units of a currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    pub amount_minor: i64,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount_minor: i64, currency: Currency) -> Self {
        Self {
            amount_minor,
            currency,
        }
    }

    pub fn zero(currency: Currency) -> Self {
        Self {
            amount_minor: 0,
            currency,
        }
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            });
        }
        let amount_minor = self
            .amount_minor
            .checked_add(other.amount_minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            amount_minor,
            currency: self.currency,
        })
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            });
        }
        let amount_minor = self
            .amount_minor
            .checked_sub(other.amount_minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            amount_minor,
            currency: self.currency,
        })
    }

    /// Half-up rounding of a rational `numerator / denominator` into minor units.
    ///
    /// Used at document totals. Never uses floating point.
    /// Half-up means: for positive values, |remainder| * 2 >= |denominator| rounds away from zero.
    pub fn round_half_up(numerator: i128, denominator: i128) -> Result<i64, MoneyError> {
        if denominator == 0 {
            return Err(MoneyError::Overflow);
        }
        let neg = (numerator < 0) ^ (denominator < 0);
        let n = numerator.unsigned_abs();
        let d = denominator.unsigned_abs();
        let q = n / d;
        let r = n % d;
        // half-up: round away from zero when fraction >= 0.5
        let rounded = if r * 2 >= d { q + 1 } else { q };
        let signed = if neg {
            -(rounded as i128)
        } else {
            rounded as i128
        };
        i64::try_from(signed).map_err(|_| MoneyError::Overflow)
    }

    /// Scale amount by `factor_num / factor_den` with half-up rounding.
    pub fn scale_half_up(self, factor_num: i64, factor_den: i64) -> Result<Self, MoneyError> {
        if factor_den == 0 {
            return Err(MoneyError::Overflow);
        }
        let product = (self.amount_minor as i128)
            .checked_mul(factor_num as i128)
            .ok_or(MoneyError::Overflow)?;
        let amount_minor = Self::round_half_up(product, factor_den as i128)?;
        Ok(Self {
            amount_minor,
            currency: self.currency,
        })
    }
}

/// Allocate `total` across `weights` using the largest-remainder method.
///
/// Each share gets `floor(total * weight / sum_weights)` minor units, then the
/// remaining units are given one-by-one to the largest fractional remainders.
/// Guarantees `sum(parts) == total.amount_minor` when weights are valid.
pub fn allocate_largest_remainder(total: Money, weights: &[u64]) -> Result<Vec<Money>, MoneyError> {
    if weights.is_empty() || weights.iter().all(|&w| w == 0) {
        return Err(MoneyError::InvalidAllocationWeights);
    }
    let sum_w: u128 = weights.iter().map(|&w| w as u128).sum();
    let total_abs = total.amount_minor.unsigned_abs() as u128;
    let sign: i64 = if total.amount_minor < 0 { -1 } else { 1 };

    let mut floors: Vec<u128> = Vec::with_capacity(weights.len());
    let mut remainders: Vec<(usize, u128)> = Vec::with_capacity(weights.len());
    let mut allocated: u128 = 0;

    for (i, &w) in weights.iter().enumerate() {
        let product = total_abs * (w as u128);
        let floor = product / sum_w;
        let rem = product % sum_w;
        floors.push(floor);
        remainders.push((i, rem));
        allocated += floor;
    }

    let mut leftover = total_abs - allocated;
    // Sort by remainder descending; stable index for ties
    remainders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (i, _) in remainders {
        if leftover == 0 {
            break;
        }
        if weights[i] == 0 {
            continue;
        }
        floors[i] += 1;
        leftover -= 1;
    }

    Ok(floors
        .into_iter()
        .map(|f| {
            let minor = (f as i64)
                .checked_mul(sign)
                .expect("sign * floor fits in i64 when total does");
            Money::new(minor, total.currency)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn checked_add_sub() {
        let a = Money::new(100, Currency::USD);
        let b = Money::new(50, Currency::USD);
        assert_eq!(a.checked_add(b).unwrap().amount_minor, 150);
        assert_eq!(a.checked_sub(b).unwrap().amount_minor, 50);
    }

    #[test]
    fn currency_mismatch() {
        let a = Money::new(1, Currency::USD);
        let b = Money::new(1, Currency::EUR);
        assert!(matches!(
            a.checked_add(b),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn overflow_detected() {
        let a = Money::new(i64::MAX, Currency::USD);
        let b = Money::new(1, Currency::USD);
        assert_eq!(a.checked_add(b), Err(MoneyError::Overflow));
    }

    #[test]
    fn half_up_positive() {
        // 1.5 -> 2, 1.4 -> 1
        assert_eq!(Money::round_half_up(3, 2).unwrap(), 2);
        assert_eq!(Money::round_half_up(7, 5).unwrap(), 1); // 1.4
        assert_eq!(Money::round_half_up(5, 2).unwrap(), 3); // 2.5 -> 3
    }

    #[test]
    fn half_up_negative() {
        // away from zero on .5: -1.5 -> -2
        assert_eq!(Money::round_half_up(-3, 2).unwrap(), -2);
        assert_eq!(Money::round_half_up(-7, 5).unwrap(), -1);
    }

    #[test]
    fn allocate_sums_to_total() {
        let total = Money::new(100, Currency::USD);
        let parts = allocate_largest_remainder(total, &[1, 1, 1]).unwrap();
        assert_eq!(parts.iter().map(|m| m.amount_minor).sum::<i64>(), 100);
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn allocate_classic_example() {
        // 100 cents across 1:1:1 => 34, 33, 33 or similar depending on tie-break
        let parts = allocate_largest_remainder(Money::new(100, Currency::USD), &[1, 1, 1]).unwrap();
        assert_eq!(parts.iter().map(|p| p.amount_minor).sum::<i64>(), 100);
        assert!(parts
            .iter()
            .all(|p| p.amount_minor == 33 || p.amount_minor == 34));
    }

    #[test]
    fn allocate_uneven_weights() {
        let parts =
            allocate_largest_remainder(Money::new(100, Currency::EUR), &[70, 20, 10]).unwrap();
        assert_eq!(parts.iter().map(|p| p.amount_minor).sum::<i64>(), 100);
        assert_eq!(parts[0].amount_minor, 70);
        assert_eq!(parts[1].amount_minor, 20);
        assert_eq!(parts[2].amount_minor, 10);
    }

    #[test]
    fn no_f64_in_api_surface() {
        // Compile-time / design assertion: Money fields are i64 + Currency only.
        let m = Money::new(199, Currency::USD);
        let _: i64 = m.amount_minor;
        assert_eq!(m.currency.as_str(), "USD");
    }

    #[test]
    fn serde_uses_integer() {
        let m = Money::new(1999, Currency::USD);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("1999"));
        assert!(!json.contains('.'));
    }

    #[test]
    fn currency_parse() {
        assert_eq!(Currency::new("USD").unwrap(), Currency::USD);
        assert!(Currency::new("usd").is_err());
        assert!(Currency::new("US").is_err());
    }

    #[test]
    fn scale_half_up() {
        let m = Money::new(100, Currency::USD);
        // 10% tax: 100 * 10 / 100 = 10
        assert_eq!(m.scale_half_up(10, 100).unwrap().amount_minor, 10);
        // 33.5% of 100 = 33.5 -> 34
        assert_eq!(m.scale_half_up(335, 1000).unwrap().amount_minor, 34);
    }

    proptest! {
        #[test]
        fn prop_add_sub_inverse(a in -1_000_000i64..1_000_000, b in -1_000_000i64..1_000_000) {
            let x = Money::new(a, Currency::USD);
            let y = Money::new(b, Currency::USD);
            let sum = x.checked_add(y).unwrap();
            assert_eq!(sum.checked_sub(y).unwrap(), x);
        }

        #[test]
        fn prop_allocate_preserves_total(
            total in -10_000i64..10_000,
            w0 in 0u64..1000,
            w1 in 0u64..1000,
            w2 in 0u64..1000,
        ) {
            prop_assume!(w0 + w1 + w2 > 0);
            let money = Money::new(total, Currency::USD);
            let parts = allocate_largest_remainder(money, &[w0, w1, w2]).unwrap();
            let sum: i64 = parts.iter().map(|p| p.amount_minor).sum();
            prop_assert_eq!(sum, total);
        }

        #[test]
        fn prop_half_up_within_one(n in -10_000i128..10_000, d in 1i128..500) {
            let r = Money::round_half_up(n, d).unwrap();
            // |r - n/d| <= 0.5 + epsilon in integer terms: |r*d - n| <= d/2 + something
            // For half-up, distance to exact is at most 0.5 in the rounded unit.
            let exact_num = n;
            let diff = (r as i128) * d - exact_num;
            prop_assert!(diff.abs() <= d); // always within one unit of true quotient region
        }

        #[test]
        fn prop_checked_add_matches_i64(a in any::<i64>(), b in any::<i64>()) {
            let x = Money::new(a, Currency::EUR);
            let y = Money::new(b, Currency::EUR);
            match a.checked_add(b) {
                Some(s) => prop_assert_eq!(x.checked_add(y).unwrap().amount_minor, s),
                None => prop_assert_eq!(x.checked_add(y), Err(MoneyError::Overflow)),
            }
        }
    }
}
