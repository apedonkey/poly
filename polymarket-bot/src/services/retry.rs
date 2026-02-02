//! Retry Logic with Exponential Backoff
//!
//! Provides configurable retry behavior for CLOB API calls.
//! Only retries errors that are classified as retryable by ClobError.

use super::clob_errors::ClobError;
use std::future::Future;
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay between retries in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum delay between retries in milliseconds
    pub max_delay_ms: u64,
    /// Backoff multiplier
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_factor: 2.0,
        }
    }
}

/// Execute an async closure with retry logic.
///
/// The closure should return `Result<T, ClobError>`.
/// Only retries if `ClobError::is_retryable()` returns true.
pub async fn with_retry<T, F, Fut>(
    config: &RetryConfig,
    operation_name: &str,
    mut f: F,
) -> Result<T, ClobError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ClobError>>,
{
    let mut attempt = 0;
    let mut delay_ms = config.initial_delay_ms;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                attempt += 1;

                if !err.is_retryable() || attempt > config.max_retries {
                    if attempt > config.max_retries {
                        warn!(
                            "[Retry] {} failed after {} attempts: {}",
                            operation_name, attempt, err
                        );
                    }
                    return Err(err);
                }

                debug!(
                    "[Retry] {} attempt {}/{} failed ({}), retrying in {}ms",
                    operation_name, attempt, config.max_retries, err, delay_ms
                );

                sleep(Duration::from_millis(delay_ms)).await;

                // Exponential backoff with cap
                delay_ms = ((delay_ms as f64 * config.backoff_factor) as u64).min(config.max_delay_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let config = RetryConfig::default();
        let result = with_retry(&config, "test", || async { Ok::<_, ClobError>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_retries() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = with_retry(&config, "test", || {
            let count = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if count < 2 {
                    Err(ClobError::RateLimited) // Retryable
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_fails_immediately() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            ..Default::default()
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = with_retry(&config, "test", || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            async { Err::<i32, _>(ClobError::InsufficientBalance) }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Only tried once
    }
}
