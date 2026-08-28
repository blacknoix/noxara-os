//! ABAC condition library evaluated by the sole PDP.
//!
//! Conditions are fail-closed: if a statement carries conditions and the
//! evaluation context cannot satisfy them, the statement does not grant access.

use chrono::{Datelike, NaiveTime, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

/// Attributes available when evaluating ABAC conditions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationContext {
    /// Wall-clock instant used for time-window checks (UTC).
    #[serde(default)]
    pub now: Option<chrono::DateTime<Utc>>,
    /// Caller IP / geo hint (CIDR or region tag).
    #[serde(default)]
    pub location: Option<String>,
    /// Active delegation id when acting on behalf of another principal.
    #[serde(default)]
    pub delegation_id: Option<String>,
    /// Domain record state (`draft`, `posted`, `closed`, `locked`, …).
    #[serde(default)]
    pub record_state: Option<String>,
}

impl EvaluationContext {
    pub fn at(now: chrono::DateTime<Utc>) -> Self {
        Self {
            now: Some(now),
            ..Self::default()
        }
    }

    pub fn with_record_state(mut self, state: impl Into<String>) -> Self {
        self.record_state = Some(state.into());
        self
    }

    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    pub fn with_delegation(mut self, delegation_id: impl Into<String>) -> Self {
        self.delegation_id = Some(delegation_id.into());
        self
    }
}

/// One ABAC predicate attached to a policy statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AbacCondition {
    /// Allow only within a daily UTC time window on selected weekdays.
    TimeWindow {
        /// Inclusive start (`HH:MM:SS`).
        start: String,
        /// Exclusive end (`HH:MM:SS`). Cross-midnight windows are supported.
        end: String,
        /// ISO weekdays 1=Mon … 7=Sun. Empty = every day.
        #[serde(default)]
        weekdays: Vec<u8>,
    },
    /// Allow when caller location matches any listed tag/CIDR prefix.
    Location {
        #[serde(default)]
        allow: Vec<String>,
    },
    /// Allow when an active delegation id is present (and optionally matches).
    Delegation {
        #[serde(default)]
        required: bool,
        #[serde(default)]
        allowed_ids: Vec<String>,
    },
    /// Allow when the resource record state is in `allow` and not in `deny`.
    RecordState {
        #[serde(default)]
        allow: Vec<String>,
        #[serde(default)]
        deny: Vec<String>,
    },
}

impl AbacCondition {
    /// Returns true when this condition is satisfied by `ctx`.
    pub fn matches(&self, ctx: &EvaluationContext) -> bool {
        match self {
            Self::TimeWindow {
                start,
                end,
                weekdays,
            } => match_time_window(ctx.now, start, end, weekdays),
            Self::Location { allow } => match_location(ctx.location.as_deref(), allow),
            Self::Delegation {
                required,
                allowed_ids,
            } => match_delegation(ctx.delegation_id.as_deref(), *required, allowed_ids),
            Self::RecordState { allow, deny } => {
                match_record_state(ctx.record_state.as_deref(), allow, deny)
            }
        }
    }
}

fn parse_hms(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M"))
        .ok()
}

fn weekday_num(w: Weekday) -> u8 {
    match w {
        Weekday::Mon => 1,
        Weekday::Tue => 2,
        Weekday::Wed => 3,
        Weekday::Thu => 4,
        Weekday::Fri => 5,
        Weekday::Sat => 6,
        Weekday::Sun => 7,
    }
}

fn match_time_window(
    now: Option<chrono::DateTime<Utc>>,
    start: &str,
    end: &str,
    weekdays: &[u8],
) -> bool {
    let Some(now) = now else {
        return false;
    };
    let Some(start_t) = parse_hms(start) else {
        return false;
    };
    let Some(end_t) = parse_hms(end) else {
        return false;
    };
    if !weekdays.is_empty() && !weekdays.contains(&weekday_num(now.weekday())) {
        return false;
    }
    let t = NaiveTime::from_hms_opt(now.hour(), now.minute(), now.second()).unwrap_or(now.time());
    if start_t <= end_t {
        t >= start_t && t < end_t
    } else {
        // Crosses midnight: e.g. 22:00–06:00
        t >= start_t || t < end_t
    }
}

fn match_location(location: Option<&str>, allow: &[String]) -> bool {
    if allow.is_empty() {
        return true;
    }
    let Some(loc) = location else {
        return false;
    };
    allow.iter().any(|a| loc == a || loc.starts_with(a))
}

fn match_delegation(delegation_id: Option<&str>, required: bool, allowed_ids: &[String]) -> bool {
    match delegation_id {
        None => !required && allowed_ids.is_empty(),
        Some(id) => {
            if allowed_ids.is_empty() {
                true
            } else {
                allowed_ids.iter().any(|a| a == id)
            }
        }
    }
}

fn match_record_state(state: Option<&str>, allow: &[String], deny: &[String]) -> bool {
    let Some(state) = state else {
        // Fail closed when a record-state condition is present but context omits state.
        return false;
    };
    if deny.iter().any(|d| d == state) {
        return false;
    }
    if allow.is_empty() {
        return true;
    }
    allow.iter().any(|a| a == state)
}

/// Returns true when every condition matches (AND semantics).
pub fn conditions_match(conditions: &[AbacCondition], ctx: &EvaluationContext) -> bool {
    conditions.iter().all(|c| c.matches(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn time_window_allows_inside_hours() {
        let cond = AbacCondition::TimeWindow {
            start: "09:00:00".into(),
            end: "17:00:00".into(),
            weekdays: vec![1, 2, 3, 4, 5],
        };
        // 2026-03-02 is a Monday
        let now = Utc.with_ymd_and_hms(2026, 3, 2, 12, 0, 0).unwrap();
        assert!(cond.matches(&EvaluationContext::at(now)));
    }

    #[test]
    fn time_window_denies_outside_hours() {
        let cond = AbacCondition::TimeWindow {
            start: "09:00:00".into(),
            end: "17:00:00".into(),
            weekdays: vec![1, 2, 3, 4, 5],
        };
        let now = Utc.with_ymd_and_hms(2026, 3, 2, 20, 0, 0).unwrap();
        assert!(!cond.matches(&EvaluationContext::at(now)));
    }

    #[test]
    fn record_state_denies_closed() {
        let cond = AbacCondition::RecordState {
            allow: vec!["draft".into(), "open".into()],
            deny: vec!["closed".into(), "locked".into()],
        };
        assert!(cond.matches(&EvaluationContext::default().with_record_state("draft")));
        assert!(!cond.matches(&EvaluationContext::default().with_record_state("closed")));
        assert!(!cond.matches(&EvaluationContext::default().with_record_state("locked")));
        assert!(!cond.matches(&EvaluationContext::default()));
    }

    #[test]
    fn location_and_delegation_basics() {
        let loc = AbacCondition::Location {
            allow: vec!["office".into(), "10.0.".into()],
        };
        assert!(loc.matches(&EvaluationContext::default().with_location("office")));
        assert!(loc.matches(&EvaluationContext::default().with_location("10.0.1.5")));
        assert!(!loc.matches(&EvaluationContext::default().with_location("home")));

        let del = AbacCondition::Delegation {
            required: true,
            allowed_ids: vec!["del_1".into()],
        };
        assert!(!del.matches(&EvaluationContext::default()));
        assert!(del.matches(&EvaluationContext::default().with_delegation("del_1")));
        assert!(!del.matches(&EvaluationContext::default().with_delegation("del_other")));
    }

    #[test]
    fn conditions_and_together() {
        let conds = vec![
            AbacCondition::TimeWindow {
                start: "00:00:00".into(),
                end: "23:59:59".into(),
                weekdays: vec![],
            },
            AbacCondition::RecordState {
                allow: vec!["open".into()],
                deny: vec![],
            },
        ];
        let now = Utc.with_ymd_and_hms(2026, 3, 2, 12, 0, 0).unwrap();
        assert!(conditions_match(
            &conds,
            &EvaluationContext::at(now).with_record_state("open")
        ));
        assert!(!conditions_match(
            &conds,
            &EvaluationContext::at(now).with_record_state("closed")
        ));
    }
}
