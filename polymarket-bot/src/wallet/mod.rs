//! Wallet management module for multi-user support
//!
//! Provides wallet generation, import, and encrypted storage.

mod generator;
mod encryption;

pub use generator::{generate_wallet, wallet_from_private_key, GeneratedWallet};
pub use encryption::{encrypt_private_key, decrypt_private_key, EncryptedKey};
