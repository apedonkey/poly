//! In-memory key store for auto-trading
//!
//! Holds decrypted private keys in memory while auto-trading is enabled.
//! Keys are cleared when auto-trading is disabled.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Thread-safe store for decrypted private keys
#[derive(Clone, Default)]
pub struct KeyStore {
    keys: Arc<RwLock<HashMap<String, String>>>,
}

impl KeyStore {
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store a decrypted private key for a wallet
    pub async fn store_key(&self, wallet_address: &str, private_key: String) {
        let mut keys = self.keys.write().await;
        keys.insert(wallet_address.to_lowercase(), private_key);
        info!(
            "[KeyStore] Stored key for wallet {} (auto-trading enabled)",
            wallet_address
        );
    }

    /// Get a stored private key
    pub async fn get_key(&self, wallet_address: &str) -> Option<String> {
        let keys = self.keys.read().await;
        keys.get(&wallet_address.to_lowercase()).cloned()
    }

    /// Remove a stored key when auto-trading is disabled
    pub async fn remove_key(&self, wallet_address: &str) {
        let mut keys = self.keys.write().await;
        if keys.remove(&wallet_address.to_lowercase()).is_some() {
            info!(
                "[KeyStore] Removed key for wallet {} (auto-trading disabled)",
                wallet_address
            );
        }
    }

    /// Check if a key is stored
    pub async fn has_key(&self, wallet_address: &str) -> bool {
        let keys = self.keys.read().await;
        keys.contains_key(&wallet_address.to_lowercase())
    }

    /// Clear all stored keys (for shutdown)
    pub async fn clear(&self) {
        let mut keys = self.keys.write().await;
        let count = keys.len();
        keys.clear();
        if count > 0 {
            info!("[KeyStore] Cleared {} stored keys", count);
        }
    }
}
