//! Deterministic formula fields — same-record only, no network.
//!
//! Supported: field refs, number literals, + - *, money minor-units as integers.
//! Recalculated on every write before persist.

use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FormulaError {
    #[error("formula parse error: {0}")]
    Parse(String),
    #[error("formula runtime error: {0}")]
    Runtime(String),
    #[error("formula attempted network/host escape")]
    Escape,
}

/// Evaluate a simple infix expression over record field values.
/// Grammar: expr := term (("+"|"-") term)* ; term := factor (("*") factor)* ;
/// factor := NUMBER | FIELD | "(" expr ")"
pub fn eval_formula(expr: &str, values: &Map<String, Value>) -> Result<Value, FormulaError> {
    let lower = expr.to_ascii_lowercase();
    for banned in ["http", "fetch", "fs::", "env::", "sql", "require", "import"] {
        if lower.contains(banned) {
            return Err(FormulaError::Escape);
        }
    }
    let mut p = Parser {
        chars: expr.chars().peekable(),
        values,
    };
    let v = p.parse_expr()?;
    p.skip_ws();
    if p.chars.peek().is_some() {
        return Err(FormulaError::Parse("trailing input".into()));
    }
    Ok(v)
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    values: &'a Map<String, Value>,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.chars.peek(), Some(c) if c.is_whitespace()) {
            self.chars.next();
        }
    }

    fn parse_expr(&mut self) -> Result<Value, FormulaError> {
        let mut left = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.chars.peek().copied() {
                Some('+') => {
                    self.chars.next();
                    let right = self.parse_term()?;
                    left = num_op(left, right, |a, b| a + b)?;
                }
                Some('-') => {
                    self.chars.next();
                    let right = self.parse_term()?;
                    left = num_op(left, right, |a, b| a - b)?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Value, FormulaError> {
        let mut left = self.parse_factor()?;
        loop {
            self.skip_ws();
            if self.chars.peek() == Some(&'*') {
                self.chars.next();
                let right = self.parse_factor()?;
                left = num_op(left, right, |a, b| a * b)?;
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Value, FormulaError> {
        self.skip_ws();
        match self.chars.peek().copied() {
            Some('(') => {
                self.chars.next();
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.chars.next() != Some(')') {
                    return Err(FormulaError::Parse("expected ')'".into()));
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '-' => self.parse_number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => self.parse_field(),
            other => Err(FormulaError::Parse(format!("unexpected {other:?}"))),
        }
    }

    fn parse_number(&mut self) -> Result<Value, FormulaError> {
        let mut s = String::new();
        if self.chars.peek() == Some(&'-') {
            s.push(self.chars.next().unwrap());
        }
        while matches!(self.chars.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(self.chars.next().unwrap());
        }
        // Integers only — money minor units; reject floats to avoid money-as-float.
        if self.chars.peek() == Some(&'.') {
            return Err(FormulaError::Parse(
                "floats forbidden — use integer minor units for money".into(),
            ));
        }
        let n: i64 = s
            .parse()
            .map_err(|e| FormulaError::Parse(format!("bad number: {e}")))?;
        Ok(Value::Number(n.into()))
    }

    fn parse_field(&mut self) -> Result<Value, FormulaError> {
        let mut name = String::new();
        while matches!(self.chars.peek(), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
            name.push(self.chars.next().unwrap());
        }
        match self.values.get(&name) {
            Some(Value::Object(m)) if m.contains_key("amount_minor") => m
                .get("amount_minor")
                .cloned()
                .ok_or_else(|| FormulaError::Runtime("money missing amount_minor".into())),
            Some(v) => Ok(v.clone()),
            None => Ok(Value::Null),
        }
    }
}

fn num_op(a: Value, b: Value, f: impl Fn(i64, i64) -> i64) -> Result<Value, FormulaError> {
    let ai = as_i64(&a)?;
    let bi = as_i64(&b)?;
    Ok(Value::Number(f(ai, bi).into()))
}

fn as_i64(v: &Value) -> Result<i64, FormulaError> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| FormulaError::Runtime("non-integer number".into())),
        Value::Null => Ok(0),
        _ => Err(FormulaError::Runtime("expected integer".into())),
    }
}

/// Apply formula field definitions onto a values map (in-place).
pub fn apply_formulas(
    fields: &[super::types::FieldDef],
    values: &mut Map<String, Value>,
) -> Result<(), FormulaError> {
    for f in fields {
        if f.field_type != super::types::FieldType::Formula {
            continue;
        }
        let Some(expr) = f.formula.as_deref() else {
            continue;
        };
        let v = eval_formula(expr, values)?;
        values.insert(f.name.clone(), v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_arithmetic() {
        let mut m = Map::new();
        m.insert("qty".into(), Value::from(3));
        m.insert("unit".into(), Value::from(250));
        let v = eval_formula("qty * unit", &m).unwrap();
        assert_eq!(v, Value::from(750));
    }

    #[test]
    fn rejects_network_escape() {
        let m = Map::new();
        assert_eq!(
            eval_formula("http://evil", &m).unwrap_err(),
            FormulaError::Escape
        );
    }

    #[test]
    fn rejects_floats() {
        let m = Map::new();
        assert!(eval_formula("1.5 + 2", &m).is_err());
    }

    #[test]
    fn money_uses_minor_units() {
        let mut m = Map::new();
        m.insert(
            "price".into(),
            serde_json::json!({ "amount_minor": 1999, "currency": "USD" }),
        );
        m.insert("qty".into(), Value::from(2));
        let v = eval_formula("price * qty", &m).unwrap();
        assert_eq!(v, Value::from(3998));
    }
}
