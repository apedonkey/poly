//! CLOB API Error Differentiation
//!
//! Parses Polymarket CLOB API error responses into structured types
//! for better error handling, retries, and user-facing messages.

use serde::Deserialize;
use std::fmt;

/// Structured CLOB API error types
#[derive(Debug, Clone)]
pub enum ClobError {
    /// Wallet has insufficient USDC balance
    InsufficientBalance,
    /// Price moved beyond acceptable range
    PriceMoved,
    /// Order size is below minimum
    OrderSizeTooSmall,
    /// Price not on valid tick boundary
    InvalidTickSize,
    /// Rate limited by CLOB API
    RateLimited,
    /// Market is closed or not accepting orders
    MarketClosed,
    /// API key/signature authentication failed
    AuthenticationFailed,
    /// Network/connection error (timeout, DNS, etc.)
    NetworkError(String),
    /// Unknown error with status code and body
    Unknown { status: u16, body: String },
}

/// CLOB API error response format
#[derive(Debug, Deserialize)]
struct ClobErrorResponse {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl ClobError {
    /// Parse a CLOB API response into a structured error
    pub fn from_response(status: u16, body: &str) -> Self {
        // Try to parse JSON error response
        let error_msg = if let Ok(parsed) = serde_json::from_str::<ClobErrorResponse>(body) {
            parsed.error.or(parsed.message).unwrap_or_default()
        } else {
            body.to_string()
        };

        let msg_lower = error_msg.to_lowercase();

        // Rate limiting
        if status == 429 || msg_lower.contains("rate limit") || msg_lower.contains("too many requests") {
            return ClobError::RateLimited;
        }

        // Authentication
        if status == 401 || status == 403 || msg_lower.contains("unauthorized") || msg_lower.contains("forbidden") || msg_lower.contains("invalid api key") || msg_lower.contains("invalid signature") {
            return ClobError::AuthenticationFailed;
        }

        // Insufficient balance
        if msg_lower.contains("insufficient") || msg_lower.contains("not enough") || msg_lower.contains("balance") {
            return ClobError::InsufficientBalance;
        }

        // Price moved
        if msg_lower.contains("price") && (msg_lower.contains("moved") || msg_lower.contains("changed") || msg_lower.contains("stale")) {
            return ClobError::PriceMoved;
        }

        // Order size too small
        if msg_lower.contains("size") && (msg_lower.contains("small") || msg_lower.contains("minimum") || msg_lower.contains("below")) {
            return ClobError::OrderSizeTooSmall;
        }

        // Invalid tick size
        if msg_lower.contains("tick") || (msg_lower.contains("price") && msg_lower.contains("invalid")) {
            return ClobError::InvalidTickSize;
        }

        // Market closed
        if msg_lower.contains("closed") || msg_lower.contains("not accepting") || msg_lower.contains("market") && msg_lower.contains("inactive") {
            return ClobError::MarketClosed;
        }

        ClobError::Unknown {
            status,
            body: error_msg,
        }
    }

    /// Parse a network/reqwest error
    pub fn from_network_error(err: &reqwest::Error) -> Self {
        if err.is_timeout() {
            ClobError::NetworkError("Request timed out".to_string())
        } else if err.is_connect() {
            ClobError::NetworkError("Connection failed".to_string())
        } else {
            ClobError::NetworkError(err.to_string())
        }
    }

    /// Whether this error is retryable with exponential backoff
    pub fn is_retryable(&self) -> bool {
        matches!(self, ClobError::RateLimited | ClobError::NetworkError(_) | ClobError::PriceMoved)
    }

    /// Human-readable error message for the frontend
    pub fn user_message(&self) -> String {
        match self {
            ClobError::InsufficientBalance => "Insufficient USDC balance for this trade.".to_string(),
            ClobError::PriceMoved => "Price has moved since the order was created. Try again.".to_string(),
            ClobError::OrderSizeTooSmall => "Order size is below the minimum allowed.".to_string(),
            ClobError::InvalidTickSize => "Price is not on a valid tick boundary.".to_string(),
            ClobError::RateLimited => "Too many requests. Please wait a moment and try again.".to_string(),
            ClobError::MarketClosed => "This market is no longer accepting orders.".to_string(),
            ClobError::AuthenticationFailed => "API authentication failed. Please re-derive your API key.".to_string(),
            ClobError::NetworkError(msg) => format!("Network error: {}. Please check your connection.", msg),
            ClobError::Unknown { status, body } => format!("CLOB API error {}: {}", status, body),
        }
    }
}

impl fmt::Display for ClobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for ClobError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limited() {
        let err = ClobError::from_response(429, "");
        assert!(err.is_retryable());
        assert!(matches!(err, ClobError::RateLimited));
    }

    #[test]
    fn test_insufficient_balance() {
        let err = ClobError::from_response(400, r#"{"error":"Insufficient balance"}"#);
        assert!(!err.is_retryable());
        assert!(matches!(err, ClobError::InsufficientBalance));
    }

    #[test]
    fn test_auth_failed() {
        let err = ClobError::from_response(401, r#"{"message":"Unauthorized"}"#);
        assert!(!err.is_retryable());
        assert!(matches!(err, ClobError::AuthenticationFailed));
    }

    #[test]
    fn test_unknown() {
        let err = ClobError::from_response(500, "Internal server error");
        assert!(!err.is_retryable());
        assert!(matches!(err, ClobError::Unknown { .. }));
    }
}
