//! Metrics collection for monitoring bot performance

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Collected metrics for the trading bot
#[derive(Debug, Serialize)]
pub struct MetricsSnapshot {
    /// Total orders submitted
    pub orders_submitted: u64,
    /// Orders by type
    pub orders_fok: u64,
    pub orders_gtc: u64,
    pub orders_gtd: u64,
    pub orders_fak: u64,
    /// Order outcomes
    pub orders_filled: u64,
    pub orders_cancelled: u64,
    pub orders_failed: u64,
    /// WebSocket status
    pub price_ws_reconnects: u64,
    pub user_ws_reconnects: u64,
    /// CLOB API stats
    pub api_calls_total: u64,
    pub api_errors_total: u64,
    pub api_rate_limited: u64,
    /// Rate limiter utilization (0.0 - 1.0)
    pub rate_limiter_general_util: f64,
    pub rate_limiter_post_util: f64,
    pub rate_limiter_delete_util: f64,
}

/// Thread-safe metrics collector
#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug)]
struct MetricsInner {
    orders_submitted: AtomicU64,
    orders_fok: AtomicU64,
    orders_gtc: AtomicU64,
    orders_gtd: AtomicU64,
    orders_fak: AtomicU64,
    orders_filled: AtomicU64,
    orders_cancelled: AtomicU64,
    orders_failed: AtomicU64,
    price_ws_reconnects: AtomicU64,
    user_ws_reconnects: AtomicU64,
    api_calls_total: AtomicU64,
    api_errors_total: AtomicU64,
    api_rate_limited: AtomicU64,
    rate_limiter_util: RwLock<(f64, f64, f64)>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsInner {
                orders_submitted: AtomicU64::new(0),
                orders_fok: AtomicU64::new(0),
                orders_gtc: AtomicU64::new(0),
                orders_gtd: AtomicU64::new(0),
                orders_fak: AtomicU64::new(0),
                orders_filled: AtomicU64::new(0),
                orders_cancelled: AtomicU64::new(0),
                orders_failed: AtomicU64::new(0),
                price_ws_reconnects: AtomicU64::new(0),
                user_ws_reconnects: AtomicU64::new(0),
                api_calls_total: AtomicU64::new(0),
                api_errors_total: AtomicU64::new(0),
                api_rate_limited: AtomicU64::new(0),
                rate_limiter_util: RwLock::new((0.0, 0.0, 0.0)),
            }),
        }
    }

    pub fn inc_orders_submitted(&self) {
        self.inner.orders_submitted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_order_type(&self, order_type: &str) {
        match order_type.to_uppercase().as_str() {
            "FOK" => self.inner.orders_fok.fetch_add(1, Ordering::Relaxed),
            "GTC" | "LIMIT" => self.inner.orders_gtc.fetch_add(1, Ordering::Relaxed),
            "GTD" => self.inner.orders_gtd.fetch_add(1, Ordering::Relaxed),
            "FAK" => self.inner.orders_fak.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }

    pub fn inc_orders_filled(&self) {
        self.inner.orders_filled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_orders_cancelled(&self) {
        self.inner.orders_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_orders_failed(&self) {
        self.inner.orders_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_price_ws_reconnects(&self) {
        self.inner.price_ws_reconnects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_user_ws_reconnects(&self) {
        self.inner.user_ws_reconnects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_api_calls(&self) {
        self.inner.api_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_api_errors(&self) {
        self.inner.api_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_api_rate_limited(&self) {
        self.inner.api_rate_limited.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn set_rate_limiter_util(&self, general: f64, post: f64, delete: f64) {
        *self.inner.rate_limiter_util.write().await = (general, post, delete);
    }

    pub async fn snapshot(&self) -> MetricsSnapshot {
        let (general, post, delete) = *self.inner.rate_limiter_util.read().await;
        MetricsSnapshot {
            orders_submitted: self.inner.orders_submitted.load(Ordering::Relaxed),
            orders_fok: self.inner.orders_fok.load(Ordering::Relaxed),
            orders_gtc: self.inner.orders_gtc.load(Ordering::Relaxed),
            orders_gtd: self.inner.orders_gtd.load(Ordering::Relaxed),
            orders_fak: self.inner.orders_fak.load(Ordering::Relaxed),
            orders_filled: self.inner.orders_filled.load(Ordering::Relaxed),
            orders_cancelled: self.inner.orders_cancelled.load(Ordering::Relaxed),
            orders_failed: self.inner.orders_failed.load(Ordering::Relaxed),
            price_ws_reconnects: self.inner.price_ws_reconnects.load(Ordering::Relaxed),
            user_ws_reconnects: self.inner.user_ws_reconnects.load(Ordering::Relaxed),
            api_calls_total: self.inner.api_calls_total.load(Ordering::Relaxed),
            api_errors_total: self.inner.api_errors_total.load(Ordering::Relaxed),
            api_rate_limited: self.inner.api_rate_limited.load(Ordering::Relaxed),
            rate_limiter_general_util: general,
            rate_limiter_post_util: post,
            rate_limiter_delete_util: delete,
        }
    }
}
