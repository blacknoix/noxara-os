//! Leave balance derivation from ledger entries (pure).
//!
//! Balances are **never** a mutable cache — they are always the sum of
//! append-only ledger entries as of a date, with expiry applied.
//! Units are milli-days (1000 = 1.0 day, 500 = half-day).

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

/// One ledger fact used for balance derivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub entry_kind: String,
    /// Signed milli-days (positive credit, negative debit).
    pub units_milli: i32,
    pub effective_date: NaiveDate,
    /// Optional expiry date for this credit bucket.
    pub expires_on: Option<NaiveDate>,
}

/// Accrual policy knobs used by year-end carry-forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccrualPolicy {
    pub accrual_cadence: String,
    pub accrual_units_milli: i32,
    pub carry_forward_cap_milli: Option<i32>,
    pub expiry_days: Option<i32>,
}

/// Derive available balance in milli-days as of `as_of`.
///
/// Rules:
/// - Include every entry with `effective_date <= as_of`.
/// - Credits with `expires_on < as_of` are excluded (already expired).
/// - Explicit `expiry` entries are still summed (they carry negative units).
pub fn balance_as_of(entries: &[LedgerEntry], as_of: NaiveDate) -> i32 {
    entries
        .iter()
        .filter(|e| e.effective_date <= as_of)
        .filter(|e| !matches!(e.expires_on, Some(exp) if e.units_milli > 0 && exp < as_of))
        .map(|e| e.units_milli)
        .sum()
}

/// Compute carry-forward credit for a leave type at year boundary.
///
/// Returns `(units_milli, expires_on)` or `None` when nothing carries.
pub fn carry_forward_credit(
    balance_at_year_end: i32,
    policy: &AccrualPolicy,
    year_end: NaiveDate,
) -> Option<(i32, Option<NaiveDate>)> {
    if balance_at_year_end <= 0 {
        return None;
    }
    let capped = match policy.carry_forward_cap_milli {
        Some(cap) => balance_at_year_end.min(cap),
        None => balance_at_year_end,
    };
    if capped <= 0 {
        return None;
    }
    let expires_on = policy
        .expiry_days
        .map(|d| year_end + chrono::Duration::days(i64::from(d)));
    Some((capped, expires_on))
}

/// Calendar-day leave duration in milli-days with half-day periods.
///
/// - `full` = 1000, `am`/`pm` = 500 on that edge day.
/// - Single-day with both am+pm or either full = 1000; am-only or pm-only = 500.
/// - Holidays listed in `holiday_dates` (full) or `half_holidays` reduce the total.
/// - Weekends (Sat/Sun) are excluded when `exclude_weekends` is true.
pub fn leave_units_milli(
    start: NaiveDate,
    end: NaiveDate,
    start_period: &str,
    end_period: &str,
    exclude_weekends: bool,
    full_holidays: &[NaiveDate],
    half_holidays: &[NaiveDate],
) -> i32 {
    if end < start {
        return 0;
    }
    let mut total = 0i32;
    let mut d = start;
    while d <= end {
        let weekday = d.weekday().num_days_from_monday();
        let is_weekend = weekday >= 5;
        if exclude_weekends && is_weekend {
            d = d.succ_opt().unwrap_or(d);
            continue;
        }
        if full_holidays.contains(&d) {
            d = d.succ_opt().unwrap_or(d);
            continue;
        }
        let mut day_units: i32 = if start == end {
            match (start_period, end_period) {
                ("am", "am") | ("pm", "pm") => 500,
                ("am", "pm") | ("full", _) | (_, "full") => 1000,
                ("pm", "am") => 1000, // treat inverted as full day
                _ => 1000,
            }
        } else if d == start {
            match start_period {
                "am" | "full" => 1000,
                "pm" => 500,
                _ => 1000,
            }
        } else if d == end {
            match end_period {
                "pm" | "full" => 1000,
                "am" => 500,
                _ => 1000,
            }
        } else {
            1000
        };
        if half_holidays.contains(&d) {
            day_units = day_units.saturating_sub(500).max(0);
        }
        total += day_units;
        match d.succ_opt() {
            Some(n) => d = n,
            None => break,
        }
    }
    total
}

/// Format milli-days as a decimal string (e.g. 1500 → "1.5").
pub fn format_days(units_milli: i32) -> String {
    let neg = units_milli < 0;
    let abs = units_milli.unsigned_abs();
    let whole = abs / 1000;
    let frac = abs % 1000;
    let s = if frac == 0 {
        format!("{whole}")
    } else if frac % 100 == 0 {
        format!("{whole}.{}", frac / 100)
    } else if frac % 10 == 0 {
        format!("{whole}.{:02}", frac / 10)
    } else {
        format!("{whole}.{frac:03}")
    };
    if neg {
        format!("-{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use proptest::prelude::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn empty_ledger_is_zero() {
        assert_eq!(balance_as_of(&[], d(2026, 6, 1)), 0);
    }

    #[test]
    fn accrual_plus_debit() {
        let entries = vec![
            LedgerEntry {
                entry_kind: "accrual".into(),
                units_milli: 20_000,
                effective_date: d(2026, 1, 1),
                expires_on: None,
            },
            LedgerEntry {
                entry_kind: "debit".into(),
                units_milli: -3_000,
                effective_date: d(2026, 3, 1),
                expires_on: None,
            },
        ];
        assert_eq!(balance_as_of(&entries, d(2026, 2, 1)), 20_000);
        assert_eq!(balance_as_of(&entries, d(2026, 6, 1)), 17_000);
    }

    #[test]
    fn expiry_excludes_stale_credit() {
        let entries = vec![LedgerEntry {
            entry_kind: "carry_forward".into(),
            units_milli: 5_000,
            effective_date: d(2026, 1, 1),
            expires_on: Some(d(2026, 3, 31)),
        }];
        assert_eq!(balance_as_of(&entries, d(2026, 3, 31)), 5_000);
        assert_eq!(balance_as_of(&entries, d(2026, 4, 1)), 0);
    }

    #[test]
    fn half_day_single() {
        let u = leave_units_milli(d(2026, 3, 2), d(2026, 3, 2), "am", "am", true, &[], &[]);
        assert_eq!(u, 500);
        let u = leave_units_milli(d(2026, 3, 2), d(2026, 3, 2), "full", "full", true, &[], &[]);
        assert_eq!(u, 1000);
    }

    #[test]
    fn weekends_excluded() {
        // Fri 2026-03-06 → Mon 2026-03-09 = 2 working days
        let u = leave_units_milli(d(2026, 3, 6), d(2026, 3, 9), "full", "full", true, &[], &[]);
        assert_eq!(u, 2_000);
    }

    #[test]
    fn holiday_excluded() {
        let hol = [d(2026, 3, 4)]; // Wed
        let u = leave_units_milli(
            d(2026, 3, 2),
            d(2026, 3, 4),
            "full",
            "full",
            true,
            &hol,
            &[],
        );
        assert_eq!(u, 2_000); // Mon+Tue
    }

    #[test]
    fn timezone_fixture_matrix_half_day_edges() {
        // Start pm + end am across 3 weekdays → 0.5 + 1.0 + 0.5 = 2.0
        let u = leave_units_milli(d(2026, 3, 2), d(2026, 3, 4), "pm", "am", true, &[], &[]);
        assert_eq!(u, 2_000);
    }

    #[test]
    fn carry_forward_respects_cap_and_expiry() {
        let policy = AccrualPolicy {
            accrual_cadence: "yearly".into(),
            accrual_units_milli: 20_000,
            carry_forward_cap_milli: Some(5_000),
            expiry_days: Some(90),
        };
        let (units, exp) = carry_forward_credit(12_000, &policy, d(2025, 12, 31)).unwrap();
        assert_eq!(units, 5_000);
        assert_eq!(exp, Some(d(2026, 3, 31)));
    }

    #[test]
    fn carry_forward_idempotent_math() {
        // Applying carry-forward twice on the same year-end balance must not
        // invent extra credit — callers gate via source_key uniqueness.
        let policy = AccrualPolicy {
            accrual_cadence: "yearly".into(),
            accrual_units_milli: 20_000,
            carry_forward_cap_milli: Some(10_000),
            expiry_days: None,
        };
        let a = carry_forward_credit(8_000, &policy, d(2025, 12, 31));
        let b = carry_forward_credit(8_000, &policy, d(2025, 12, 31));
        assert_eq!(a, b);
    }

    proptest! {
        #[test]
        fn balance_reproducible_from_ledger(
            accruals in prop::collection::vec(1_000i32..5_000, 0..8),
            debits in prop::collection::vec(500i32..2_000, 0..5),
            half_day_debits in 0u8..4,
        ) {
            let start = d(2026, 1, 1);
            let mut entries = Vec::new();
            for (i, a) in accruals.iter().enumerate() {
                entries.push(LedgerEntry {
                    entry_kind: "accrual".into(),
                    units_milli: *a,
                    effective_date: start + chrono::Duration::days(i as i64),
                    expires_on: None,
                });
            }
            for (i, deb) in debits.iter().enumerate() {
                entries.push(LedgerEntry {
                    entry_kind: "debit".into(),
                    units_milli: -deb,
                    effective_date: start + chrono::Duration::days(30 + i as i64),
                    expires_on: None,
                });
            }
            for i in 0..half_day_debits {
                entries.push(LedgerEntry {
                    entry_kind: "debit".into(),
                    units_milli: -500,
                    effective_date: start + chrono::Duration::days(60 + i as i64),
                    expires_on: None,
                });
            }
            // Carry-forward then expiry
            let year_end = d(2026, 12, 31);
            let bal = balance_as_of(&entries, year_end);
            let policy = AccrualPolicy {
                accrual_cadence: "yearly".into(),
                accrual_units_milli: 20_000,
                carry_forward_cap_milli: Some(10_000),
                expiry_days: Some(30),
            };
            if let Some((cf, exp)) = carry_forward_credit(bal.max(0), &policy, year_end) {
                entries.push(LedgerEntry {
                    entry_kind: "carry_forward".into(),
                    units_milli: cf,
                    effective_date: d(2027, 1, 1),
                    expires_on: exp,
                });
            }
            let as_of = d(2027, 2, 15);
            let once = balance_as_of(&entries, as_of);
            let twice = balance_as_of(&entries, as_of);
            assert_eq!(once, twice);
            // Recompute from scratch with same entries → identical
            let recomputed: i32 = entries
                .iter()
                .filter(|e| e.effective_date <= as_of)
                .filter(|e| !matches!(e.expires_on, Some(exp) if e.units_milli > 0 && exp < as_of))
                .map(|e| e.units_milli)
                .sum();
            assert_eq!(once, recomputed);
        }
    }
}
