//! Fixed-window attempt limiter. Replaces `limiter.allowRequests(10).every('1 minute')` on login.
//! In memory and clock-free: callers pass the time, so it is trivial to test.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

/// Keeps the table from growing without bound if many distinct keys are tried.
const PRUNE_AT: usize = 1024;

#[derive(Debug)]
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    entries: HashMap<String, (DateTime<Utc>, u32)>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            entries: HashMap::new(),
        }
    }

    /// Counts one attempt for `key`. `Err` carries how long until the window reopens.
    pub fn check(&mut self, key: &str, now: DateTime<Utc>) -> Result<(), Duration> {
        if self.entries.len() >= PRUNE_AT {
            let window = self.window;
            self.entries.retain(|_, (start, _)| now < *start + window);
        }
        let entry = self.entries.entry(key.to_owned()).or_insert((now, 0));
        if now >= entry.0 + self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit {
            return Err(entry.0 + self.window - now);
        }
        entry.1 += 1;
        Ok(())
    }

    /// Forgets `key`, e.g. after a successful login.
    pub fn reset(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_750_000_000 + seconds, 0).unwrap()
    }

    #[test]
    fn allows_the_limit_then_blocks_until_the_window_ends() {
        let mut limiter = RateLimiter::new(10, Duration::minutes(1));
        for second in 0..10 {
            assert!(limiter.check("a", at(second)).is_ok(), "attempt {second}");
        }
        assert_eq!(limiter.check("a", at(10)), Err(Duration::seconds(50)));
        assert_eq!(limiter.check("a", at(59)), Err(Duration::seconds(1)));
        assert!(limiter.check("a", at(60)).is_ok());
    }

    #[test]
    fn keys_are_independent_and_reset_clears_one() {
        let mut limiter = RateLimiter::new(2, Duration::minutes(1));
        limiter.check("a", at(0)).unwrap();
        limiter.check("a", at(1)).unwrap();
        assert!(limiter.check("a", at(2)).is_err());
        assert!(limiter.check("b", at(2)).is_ok());
        limiter.reset("a");
        assert!(limiter.check("a", at(3)).is_ok());
    }

    #[test]
    fn expired_entries_are_pruned() {
        let mut limiter = RateLimiter::new(1, Duration::seconds(10));
        for n in 0..PRUNE_AT {
            limiter.check(&format!("k{n}"), at(0)).unwrap();
        }
        limiter.check("fresh", at(100)).unwrap();
        assert!(limiter.entries.len() < 10);
    }
}
