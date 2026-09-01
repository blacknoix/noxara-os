//! Purpose-built scripting sandbox — no host-language `eval`.
//!
//! Scripts are a tiny JSON AST executed with hard CPU (op), memory (bytes),
//! and wall-clock limits. Network, disk, and env access are denied; only
//! documented host functions (`get`/`set` on the current record) are available.
//! Fail closed on OOM / timeout / op exhaustion.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Resource caps for a single script invocation.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_ops: u64,
    pub max_bytes: usize,
    pub max_wall: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_ops: 10_000,
            max_bytes: 256 * 1024,
            max_wall: Duration::from_millis(50),
        }
    }
}

/// Fail-closed sandbox errors.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SandboxError {
    #[error("script exceeded CPU operation limit ({0})")]
    CpuLimit(u64),
    #[error("script exceeded memory limit ({0} bytes)")]
    MemoryLimit(usize),
    #[error("script exceeded wall-clock time limit")]
    TimeLimit,
    #[error("denied host function: {0}")]
    DeniedHost(String),
    #[error("script error: {0}")]
    Runtime(String),
}

/// Host surface available to scripts — current record only by default.
pub trait ScriptHost {
    fn get_field(&self, name: &str) -> Result<Value, SandboxError>;
    fn set_field(&mut self, name: &str, value: Value) -> Result<(), SandboxError>;
}

/// In-memory record host used by lifecycle hooks.
#[derive(Debug, Clone)]
pub struct RecordHost {
    pub values: serde_json::Map<String, Value>,
    pub bytes_used: usize,
    pub max_bytes: usize,
}

impl RecordHost {
    pub fn new(values: serde_json::Map<String, Value>, max_bytes: usize) -> Self {
        let bytes_used = approx_bytes(&Value::Object(values.clone()));
        Self {
            values,
            bytes_used,
            max_bytes,
        }
    }
}

impl ScriptHost for RecordHost {
    fn get_field(&self, name: &str) -> Result<Value, SandboxError> {
        Ok(self.values.get(name).cloned().unwrap_or(Value::Null))
    }

    fn set_field(&mut self, name: &str, value: Value) -> Result<(), SandboxError> {
        let new_bytes = approx_bytes(&value);
        let old = self.values.get(name).map(approx_bytes).unwrap_or(0);
        let next = self
            .bytes_used
            .saturating_sub(old)
            .saturating_add(new_bytes);
        if next > self.max_bytes {
            return Err(SandboxError::MemoryLimit(self.max_bytes));
        }
        self.bytes_used = next;
        self.values.insert(name.to_string(), value);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Stmt {
    /// Set a field from an expression.
    Set { field: String, expr: Expr },
    /// Conditional branch.
    If {
        cond: Expr,
        then: Vec<Stmt>,
        #[serde(default)]
        else_: Vec<Stmt>,
    },
    /// Bounded loop — `times` is clamped by remaining ops.
    Loop { times: u32, body: Vec<Stmt> },
    /// Explicit busy-wait (always denied / used only to test time kill).
    SleepMs { ms: u64 },
    /// Allocate a string of `size` bytes into `field` (memory-limit tests).
    Alloc { field: String, size: usize },
    /// Call a host function by name (allow-list only).
    Call {
        name: String,
        #[serde(default)]
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Expr {
    Lit { value: Value },
    Field { name: String },
    Add { left: Box<Expr>, right: Box<Expr> },
    Sub { left: Box<Expr>, right: Box<Expr> },
    Mul { left: Box<Expr>, right: Box<Expr> },
    Eq { left: Box<Expr>, right: Box<Expr> },
    Gt { left: Box<Expr>, right: Box<Expr> },
    Not { expr: Box<Expr> },
    True,
}

struct Meter {
    ops: u64,
    limits: Limits,
    started: Instant,
}

impl Meter {
    fn new(limits: Limits) -> Self {
        Self {
            ops: 0,
            limits,
            started: Instant::now(),
        }
    }

    fn tick(&mut self) -> Result<(), SandboxError> {
        self.ops = self.ops.saturating_add(1);
        if self.ops > self.limits.max_ops {
            return Err(SandboxError::CpuLimit(self.limits.max_ops));
        }
        if self.started.elapsed() > self.limits.max_wall {
            return Err(SandboxError::TimeLimit);
        }
        Ok(())
    }
}

/// Execute a script program against a host under resource limits.
pub fn execute(
    program: &[Stmt],
    host: &mut dyn ScriptHost,
    limits: Limits,
) -> Result<(), SandboxError> {
    let mut meter = Meter::new(limits);
    eval_block(program, host, &mut meter)
}

fn eval_block(
    stmts: &[Stmt],
    host: &mut dyn ScriptHost,
    meter: &mut Meter,
) -> Result<(), SandboxError> {
    for stmt in stmts {
        eval_stmt(stmt, host, meter)?;
    }
    Ok(())
}

fn eval_stmt(
    stmt: &Stmt,
    host: &mut dyn ScriptHost,
    meter: &mut Meter,
) -> Result<(), SandboxError> {
    meter.tick()?;
    match stmt {
        Stmt::Set { field, expr } => {
            let v = eval_expr(expr, host, meter)?;
            host.set_field(field, v)?;
        }
        Stmt::If { cond, then, else_ } => {
            let c = eval_expr(cond, host, meter)?;
            if truthy(&c) {
                eval_block(then, host, meter)?;
            } else {
                eval_block(else_, host, meter)?;
            }
        }
        Stmt::Loop { times, body } => {
            // Hard clamp: never more iterations than remaining ops budget.
            let n = (*times as u64).min(meter.limits.max_ops.saturating_add(1));
            for _ in 0..n {
                eval_block(body, host, meter)?;
            }
        }
        Stmt::SleepMs { ms } => {
            // Sleep is not a permitted host capability — busy-wait until wall clock kills us.
            let deadline = meter.started + meter.limits.max_wall;
            let target = Instant::now() + Duration::from_millis(*ms);
            while Instant::now() < target {
                meter.tick()?;
                if Instant::now() >= deadline {
                    return Err(SandboxError::TimeLimit);
                }
            }
        }
        Stmt::Alloc { field, size } => {
            meter.tick()?;
            let s = "x".repeat(*size);
            host.set_field(field, Value::String(s))?;
        }
        Stmt::Call { name, args } => {
            let _ = args;
            match name.as_str() {
                "get" | "set" => {
                    return Err(SandboxError::Runtime(
                        "use set/field AST nodes instead of call get/set".into(),
                    ));
                }
                "http" | "fetch" | "fs" | "env" | "sql" => {
                    return Err(SandboxError::DeniedHost(name.clone()));
                }
                other => return Err(SandboxError::DeniedHost(other.to_string())),
            }
        }
    }
    Ok(())
}

fn eval_expr(
    expr: &Expr,
    host: &mut dyn ScriptHost,
    meter: &mut Meter,
) -> Result<Value, SandboxError> {
    meter.tick()?;
    Ok(match expr {
        Expr::Lit { value } => value.clone(),
        Expr::Field { name } => host.get_field(name)?,
        Expr::True => Value::Bool(true),
        Expr::Not { expr } => Value::Bool(!truthy(&eval_expr(expr, host, meter)?)),
        Expr::Eq { left, right } => {
            Value::Bool(eval_expr(left, host, meter)? == eval_expr(right, host, meter)?)
        }
        Expr::Gt { left, right } => {
            let l = as_f64(&eval_expr(left, host, meter)?)?;
            let r = as_f64(&eval_expr(right, host, meter)?)?;
            Value::Bool(l > r)
        }
        Expr::Add { left, right } => {
            let l = as_f64(&eval_expr(left, host, meter)?)?;
            let r = as_f64(&eval_expr(right, host, meter)?)?;
            json_number(l + r)
        }
        Expr::Sub { left, right } => {
            let l = as_f64(&eval_expr(left, host, meter)?)?;
            let r = as_f64(&eval_expr(right, host, meter)?)?;
            json_number(l - r)
        }
        Expr::Mul { left, right } => {
            let l = as_f64(&eval_expr(left, host, meter)?)?;
            let r = as_f64(&eval_expr(right, host, meter)?)?;
            json_number(l * r)
        }
    })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn as_f64(v: &Value) -> Result<f64, SandboxError> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| SandboxError::Runtime("invalid number".into())),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(SandboxError::Runtime("expected number".into())),
    }
}

fn json_number(n: f64) -> Value {
    serde_json::Number::from_f64(n)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn approx_bytes(v: &Value) -> usize {
    match v {
        Value::Null => 4,
        Value::Bool(_) => 1,
        Value::Number(_) => 16,
        Value::String(s) => s.len(),
        Value::Array(a) => a.iter().map(approx_bytes).sum::<usize>().saturating_add(8),
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| k.len() + approx_bytes(v))
            .sum::<usize>()
            .saturating_add(8),
    }
}

/// Parse a script source string as a JSON array of [`Stmt`].
pub fn parse_program(source: &str) -> Result<Vec<Stmt>, SandboxError> {
    serde_json::from_str(source).map_err(|e| SandboxError::Runtime(format!("parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infinite_loop_is_killed() {
        let program = vec![Stmt::Loop {
            times: u32::MAX,
            body: vec![Stmt::Set {
                field: "n".into(),
                expr: Expr::Add {
                    left: Box::new(Expr::Field { name: "n".into() }),
                    right: Box::new(Expr::Lit {
                        value: Value::from(1),
                    }),
                },
            }],
        }];
        let mut host = RecordHost::new(
            serde_json::Map::from_iter([("n".into(), Value::from(0))]),
            64 * 1024,
        );
        let err = execute(
            &program,
            &mut host,
            Limits {
                max_ops: 100,
                max_bytes: 64 * 1024,
                max_wall: Duration::from_secs(1),
            },
        )
        .unwrap_err();
        assert!(matches!(err, SandboxError::CpuLimit(_)));
    }

    #[test]
    fn huge_alloc_is_killed() {
        let program = vec![Stmt::Alloc {
            field: "blob".into(),
            size: 1024 * 1024,
        }];
        let mut host = RecordHost::new(serde_json::Map::new(), 8 * 1024);
        let err = execute(&program, &mut host, Limits::default()).unwrap_err();
        assert!(matches!(err, SandboxError::MemoryLimit(_)));
    }

    #[test]
    fn sleep_is_killed_by_wall_clock() {
        let program = vec![Stmt::SleepMs { ms: 5_000 }];
        let mut host = RecordHost::new(serde_json::Map::new(), 8 * 1024);
        let err = execute(
            &program,
            &mut host,
            Limits {
                max_ops: 1_000_000,
                max_bytes: 8 * 1024,
                max_wall: Duration::from_millis(5),
            },
        )
        .unwrap_err();
        assert_eq!(err, SandboxError::TimeLimit);
    }

    #[test]
    fn network_host_denied() {
        let program = vec![Stmt::Call {
            name: "http".into(),
            args: vec![],
        }];
        let mut host = RecordHost::new(serde_json::Map::new(), 8 * 1024);
        let err = execute(&program, &mut host, Limits::default()).unwrap_err();
        assert!(matches!(err, SandboxError::DeniedHost(_)));
    }

    #[test]
    fn set_field_works() {
        let program = vec![Stmt::Set {
            field: "status".into(),
            expr: Expr::Lit {
                value: Value::String("ok".into()),
            },
        }];
        let mut host = RecordHost::new(serde_json::Map::new(), 8 * 1024);
        execute(&program, &mut host, Limits::default()).unwrap();
        assert_eq!(host.values.get("status").unwrap(), "ok");
    }
}
