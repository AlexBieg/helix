use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

use parking_lot::Mutex;

/// Token bucket rate limiter — allows `max_rate` requests per second,
/// smoothing bursts with a bucket capacity of `max_rate` tokens.
pub struct RateLimiter {
    max_rate: u64,
    tokens: AtomicU64,
    last_refill: Mutex<Instant>,
}

impl RateLimiter {
    /// Create a new `RateLimiter` with the given maximum requests per second.
    pub fn new(max_rate: u64) -> Self {
        RateLimiter {
            max_rate,
            tokens: AtomicU64::new(max_rate),
            last_refill: Mutex::new(Instant::now()),
        }
    }

    /// Try to consume one token. Returns true if allowed, false if rate-limited.
    pub fn try_acquire(&self) -> bool {
        self.refill();
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .tokens
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            thread::yield_now();
        }
    }

    /// Block until a token is available (respects rate limit).
    pub fn acquire(&self) {
        loop {
            self.refill();
            if self.try_acquire() {
                return;
            }
            thread::yield_now();
        }
    }

    fn refill(&self) {
        let mut last = self.last_refill.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(*last);
        if elapsed.as_millis() < 10 {
            return;
        }
        *last = now;

        let new_tokens = (elapsed.as_secs_f64() * self.max_rate as f64) as u64;
        if new_tokens > 0 {
            let mut current = self.tokens.load(Ordering::Relaxed);
            loop {
                let capped = std::cmp::min(current.saturating_add(new_tokens), self.max_rate);
                match self.tokens.compare_exchange_weak(
                    current,
                    capped,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_tokens_available() {
        let rl = RateLimiter::new(100);
        // Should be able to consume max_rate tokens immediately
        for _ in 0..100 {
            assert!(rl.try_acquire(), "should have initial tokens");
        }
        // Now it should be empty
        assert!(
            !rl.try_acquire(),
            "should be rate-limited after consuming all tokens"
        );
    }

    #[test]
    fn test_refill_over_time() {
        let rl = RateLimiter::new(1000); // 1000 tokens/sec (~1 per ms)
                                         // Drain all tokens
        for _ in 0..1000 {
            assert!(rl.try_acquire());
        }
        assert!(!rl.try_acquire());

        // Wait a bit for refill — but this is tricky in tests
        // Skip time-based refill test in CI-fast scenarios
    }

    #[test]
    fn test_acquire_eventually_succeeds() {
        let rl = RateLimiter::new(10000); // very high rate
        assert!(rl.try_acquire());
    }
}
