//! Wallet generation using alloy

use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A newly generated wallet with address and private key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedWallet {
    /// Ethereum address (0x prefixed)
    pub address: String,
    /// Private key in hex format (0x prefixed) - DISPLAY ONCE ONLY
    pub private_key: String,
}

/// Generate a new random wallet
pub fn generate_wallet() -> GeneratedWallet {
    let signer = PrivateKeySigner::random();

    let address = format!("{:?}", signer.address());
    let private_key = format!("0x{}", hex::encode(signer.to_bytes()));

    GeneratedWallet {
        address,
        private_key,
    }
}

/// Get wallet address from a private key
pub fn wallet_from_private_key(private_key: &str) -> Result<String> {
    let signer: PrivateKeySigner = private_key.parse()
        .context("Failed to parse private key")?;

    Ok(format!("{:?}", signer.address()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_wallet() {
        let wallet = generate_wallet();

        // Address should start with 0x and be 42 chars
        assert!(wallet.address.starts_with("0x"));
        assert_eq!(wallet.address.len(), 42);

        // Private key should start with 0x and be 66 chars (0x + 64 hex chars)
        assert!(wallet.private_key.starts_with("0x"));
        assert_eq!(wallet.private_key.len(), 66);
    }

    #[test]
    fn test_wallet_from_private_key() {
        let wallet = generate_wallet();
        let recovered_address = wallet_from_private_key(&wallet.private_key).unwrap();

        assert_eq!(wallet.address, recovered_address);
    }
}
