//! In-memory auth rate limiter with progressive delays.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct Bucket {
    hits: Vec<Instant>,
    lockout_until: Option<Instant>,
    progressive_delay_ms: u64,
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
    /// Max attempts in window before 429.
    max_hits: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_hits: 20,
            window: Duration::from_secs(60),
        }
    }

    /// Stricter limiter for password endpoints (login / reset).
    pub fn auth_strict() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_hits: 10,
            window: Duration::from_secs(60),
        }
    }

    pub fn check_and_hit(&self, key: &str) -> Result<Duration, Duration> {
        let mut g = self.buckets.lock().expect("rate limiter");
        let now = Instant::now();
        let bucket = g.entry(key.to_string()).or_insert(Bucket {
            hits: vec![],
            lockout_until: None,
            progressive_delay_ms: 0,
        });
        if let Some(until) = bucket.lockout_until {
            if now < until {
                return Err(until.saturating_duration_since(now));
            }
            bucket.lockout_until = None;
        }
        bucket.hits.retain(|t| now.duration_since(*t) < self.window);
        if bucket.hits.len() >= self.max_hits {
            bucket.progressive_delay_ms = (bucket.progressive_delay_ms + 250).min(5_000);
            let delay = Duration::from_millis(bucket.progressive_delay_ms);
            bucket.lockout_until = Some(now + delay);
            return Err(delay);
        }
        bucket.hits.push(now);
        let delay = Duration::from_millis(bucket.progressive_delay_ms);
        Ok(delay)
    }

    pub fn register_failure(&self, key: &str) {
        let mut g = self.buckets.lock().expect("rate limiter");
        let bucket = g.entry(key.to_string()).or_insert(Bucket {
            hits: vec![],
            lockout_until: None,
            progressive_delay_ms: 0,
        });
        bucket.progressive_delay_ms = (bucket.progressive_delay_ms * 2).clamp(100, 8_000);
    }

    pub fn reset(&self, key: &str) {
        let mut g = self.buckets.lock().expect("rate limiter");
        g.remove(key);
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_after_max_hits() {
        let lim = RateLimiter {
            buckets: Mutex::new(HashMap::new()),
            max_hits: 3,
            window: Duration::from_secs(60),
        };
        assert!(lim.check_and_hit("a").is_ok());
        assert!(lim.check_and_hit("a").is_ok());
        assert!(lim.check_and_hit("a").is_ok());
        assert!(lim.check_and_hit("a").is_err());
    }
}
