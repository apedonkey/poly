//! Polymarket Safe Proxy wallet derivation
//!
//! Derives the Gnosis Safe proxy wallet address from an EOA using CREATE2.

use alloy::primitives::{keccak256, Address, B256};

/// Safe Proxy Factory address on Polygon
const SAFE_FACTORY: &str = "0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b";
/// Init code hash for Safe proxy
const SAFE_INIT_CODE_HASH: &str = "0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf";

/// Derive the Polymarket Safe proxy wallet address from an EOA address.
/// Uses CREATE2: address = keccak256(0xff ++ factory ++ salt ++ init_code_hash)[12:]
pub fn derive_safe_wallet(eoa_address: &str) -> Result<String, String> {
    let eoa: Address = eoa_address
        .parse()
        .map_err(|e| format!("Invalid EOA address: {}", e))?;
    let factory: Address = SAFE_FACTORY
        .parse()
        .map_err(|e| format!("Invalid factory address: {}", e))?;
    let init_code_hash: B256 = SAFE_INIT_CODE_HASH
        .parse()
        .map_err(|e| format!("Invalid init code hash: {}", e))?;

    // Salt = keccak256(abi.encode(address)) - address padded to 32 bytes (left-padded with zeros)
    let mut padded = [0u8; 32];
    padded[12..32].copy_from_slice(eoa.as_slice());
    let salt = keccak256(&padded);

    // CREATE2: keccak256(0xff ++ factory ++ salt ++ init_code_hash)
    let mut data = Vec::with_capacity(1 + 20 + 32 + 32);
    data.push(0xff);
    data.extend_from_slice(factory.as_slice());
    data.extend_from_slice(salt.as_slice());
    data.extend_from_slice(init_code_hash.as_slice());

    let hash = keccak256(&data);
    // Take last 20 bytes as address
    let proxy_address = Address::from_slice(&hash[12..]);

    Ok(format!("{:?}", proxy_address))
}
