//! Private key encryption using AES-256-GCM with Argon2id key derivation

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Context, Result};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Encrypted private key with all data needed for decryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKey {
    /// AES-256-GCM ciphertext
    pub ciphertext: Vec<u8>,
    /// Salt for Argon2id key derivation (16 bytes)
    pub salt: Vec<u8>,
    /// Nonce for AES-GCM (12 bytes)
    pub nonce: Vec<u8>,
}

/// Encrypt a private key with a user-provided password
///
/// Uses Argon2id for key derivation and AES-256-GCM for encryption.
/// The private key is never stored in plaintext.
pub fn encrypt_private_key(private_key: &str, password: &str) -> Result<EncryptedKey> {
    // Generate random salt (16 bytes)
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);

    // Derive key from password using Argon2id
    let mut key_bytes = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key_bytes)
        .map_err(|e| anyhow!("Failed to derive key: {}", e))?;

    // Generate random nonce (12 bytes for AES-GCM)
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Encrypt with AES-256-GCM
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, private_key.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;

    Ok(EncryptedKey {
        ciphertext,
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
    })
}

/// Decrypt a private key using the user's password
pub fn decrypt_private_key(encrypted: &EncryptedKey, password: &str) -> Result<String> {
    // Derive key from password
    let mut key_bytes = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &encrypted.salt, &mut key_bytes)
        .map_err(|e| anyhow!("Failed to derive key: {}", e))?;

    // Decrypt
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&encrypted.nonce);

    let plaintext = cipher
        .decrypt(nonce, encrypted.ciphertext.as_ref())
        .map_err(|_| anyhow!("Decryption failed - incorrect password"))?;

    String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let private_key = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let password = "my_secure_password";

        let encrypted = encrypt_private_key(private_key, password).unwrap();
        let decrypted = decrypt_private_key(&encrypted, password).unwrap();

        assert_eq!(private_key, decrypted);
    }

    #[test]
    fn test_wrong_password() {
        let private_key = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let password = "correct_password";
        let wrong_password = "wrong_password";

        let encrypted = encrypt_private_key(private_key, password).unwrap();
        let result = decrypt_private_key(&encrypted, wrong_password);

        assert!(result.is_err());
    }

    #[test]
    fn test_encrypted_key_serialization() {
        let private_key = "0xtest";
        let password = "password";

        let encrypted = encrypt_private_key(private_key, password).unwrap();

        // Should serialize to JSON
        let json = serde_json::to_string(&encrypted).unwrap();
        let deserialized: EncryptedKey = serde_json::from_str(&json).unwrap();

        // Should still decrypt correctly
        let decrypted = decrypt_private_key(&deserialized, password).unwrap();
        assert_eq!(private_key, decrypted);
    }
}
