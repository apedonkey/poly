//! Rate Limiter - token bucket implementation for CLOB API rate limits
//!
//! Polymarket CLOB enforces per-IP rate limits:
//! - General endpoints: 9000 requests per 10 seconds
//! - POST /order: 3500 requests per 10 seconds
//! - DELETE /order: 3000 requests per 10 seconds
//!
//! This module implements a token bucket at 80% of actual limits as a safety margin.

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use tracing::debug;

/// Rate limit endpoint classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointClass {
    /// General API endpoints (GET, etc.)
    General,
    /// POST /order endpoints
    PostOrder,
    /// DELETE /order endpoints
    DeleteOrder,
}

impl EndpointClass {
    /// Maximum tokens (requests) per window, at 80% of actual limit
    fn max_tokens(&self) -> u32 {
        match self {
            EndpointClass::General => 7200,     // 80% of 9000
            EndpointClass::PostOrder => 2800,   // 80% of 3500
            EndpointClass::DeleteOrder => 2400, // 80% of 3000
        }
    }

    /// Refill window duration
    fn window(&self) -> Duration {
        Duration::from_secs(10)
    }
}

/// A single token bucket
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(class: EndpointClass) -> Self {
        let max = class.max_tokens() as f64;
        let window_secs = class.window().as_secs_f64();
        Self {
            tokens: max,
            max_tokens: max,
            refill_rate: max / window_secs,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to consume one token. Returns true if successful.
    fn try_acquire(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Time until one token is available
    fn time_until_available(&mut self) -> Duration {
        self.refill();
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            let deficit = 1.0 - self.tokens;
            Duration::from_secs_f64(deficit / self.refill_rate)
        }
    }
}

/// Rate limiter with three token buckets
pub struct RateLimiter {
    general: Arc<Mutex<TokenBucket>>,
    post_order: Arc<Mutex<TokenBucket>>,
    delete_order: Arc<Mutex<TokenBucket>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            general: Arc::new(Mutex::new(TokenBucket::new(EndpointClass::General))),
            post_order: Arc::new(Mutex::new(TokenBucket::new(EndpointClass::PostOrder))),
            delete_order: Arc::new(Mutex::new(TokenBucket::new(EndpointClass::DeleteOrder))),
        }
    }

    /// Acquire a token for the given endpoint class, waiting if necessary.
    /// This blocks until a token is available.
    /// Returns true if we had to wait (i.e., were rate limited).
    pub async fn acquire(&self, class: EndpointClass) -> bool {
        let bucket = self.get_bucket(class);
        let mut waited = false;
        loop {
            let wait_time = {
                let mut b = bucket.lock().await;
                if b.try_acquire() {
                    return waited;
                }
                b.time_until_available()
            };

            waited = true;
            debug!("Rate limiter: waiting {:?} for {:?}", wait_time, class);
            tokio::time::sleep(wait_time).await;
        }
    }

    /// Try to acquire a token without waiting. Returns true if successful.
    pub async fn try_acquire(&self, class: EndpointClass) -> bool {
        let bucket = self.get_bucket(class);
        let mut b = bucket.lock().await;
        b.try_acquire()
    }

    /// Get utilization (0.0 = empty, 1.0 = full) for each bucket
    pub async fn utilization(&self) -> (f64, f64, f64) {
        let g = self.general.lock().await;
        let p = self.post_order.lock().await;
        let d = self.delete_order.lock().await;
        (
            1.0 - (g.tokens / g.max_tokens),
            1.0 - (p.tokens / p.max_tokens),
            1.0 - (d.tokens / d.max_tokens),
        )
    }

    fn get_bucket(&self, class: EndpointClass) -> &Arc<Mutex<TokenBucket>> {
        match class {
            EndpointClass::General => &self.general,
            EndpointClass::PostOrder => &self.post_order,
            EndpointClass::DeleteOrder => &self.delete_order,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_succeeds() {
        let limiter = RateLimiter::new();
        // Should succeed immediately since we start with full buckets
        limiter.acquire(EndpointClass::General).await;
        limiter.acquire(EndpointClass::PostOrder).await;
        limiter.acquire(EndpointClass::DeleteOrder).await;
    }

    #[tokio::test]
    async fn test_try_acquire() {
        let limiter = RateLimiter::new();
        // Should succeed since bucket starts full
        assert!(limiter.try_acquire(EndpointClass::General).await);
    }
}
