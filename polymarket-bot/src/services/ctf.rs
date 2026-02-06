//! Conditional Token Framework (CTF) Split/Merge/Redeem Service
//!
//! Supports splitting USDC collateral into YES + NO token pairs,
//! merging YES + NO tokens back into USDC, and redeeming after resolution.
//!
//! Uses the Polymarket relay service for gasless execution via Safe transactions.
//! The relay does NOT have dedicated /split, /merge, /redeem endpoints.
//! Instead, we ABI-encode the CTF contract call, wrap it in an EIP-712
//! signed Safe transaction, and submit via the relay's /submit endpoint.

use alloy::primitives::{keccak256, Address, B256, U256};
use alloy::signers::{local::PrivateKeySigner, Signer};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::str::FromStr;
use tracing::{debug, info, warn};

type HmacSha256 = Hmac<Sha256>;

const RELAY_URL: &str = "https://relayer-v2.polymarket.com";
/// CTF contract on Polygon — MUST target this directly (not NegRisk Adapter)
const CTF_ADDRESS: &str = "0x4d97dcd97ec945f40cf65f87097ace5ea0476045";
/// USDC on Polygon (6 decimals)
const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const CHAIN_ID: u64 = 137;
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
const POLYGON_RPC: &str = "https://polygon-rpc.com";
/// NegRisk Adapter for merging NegRisk market positions
const NEG_RISK_ADAPTER: &str = "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296";

// CTF contract function signatures for ABI encoding
sol! {
    function mergePositions(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] partition,
        uint256 amount
    );

    function splitPosition(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] partition,
        uint256 amount
    );

    function redeemPositions(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] indexSets
    );

    function balanceOf(address account, uint256 id) external view returns (uint256);
}

// NegRisk Adapter has a simplified mergePositions(bytes32, uint256) signature.
// Put in separate module to avoid name conflict with CTF's mergePositions.
mod neg_risk_abi {
    alloy::sol! {
        function mergePositions(bytes32 conditionId, uint256 amount);
        function redeemPositions(bytes32 conditionId, uint256[] amounts);
    }
}

/// Response from CTF operations
#[derive(Debug, Deserialize)]
pub struct CtfResponse {
    pub success: bool,
    #[serde(default)]
    pub transaction_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// CTF service for split/merge/redeem operations via Safe relay
pub struct CtfService {
    client: reqwest::Client,
}

impl CtfService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Split USDC collateral into YES + NO token pairs
    pub async fn split(
        &self,
        condition_id: &str,
        amount: rust_decimal::Decimal,
        private_key: &str,
        builder_api_key: &str,
        builder_secret: &str,
        builder_passphrase: &str,
    ) -> Result<CtfResponse> {
        let amount_raw = to_raw_amount(amount);
        info!(
            "CTF Split: {} USDC ({} raw) for condition {}",
            amount,
            amount_raw,
            &condition_id[..12.min(condition_id.len())]
        );

        let usdc: Address = USDC_ADDRESS.parse()?;
        let cond_bytes = parse_condition_id(condition_id)?;

        let call = splitPositionCall {
            collateralToken: usdc,
            parentCollectionId: B256::ZERO,
            conditionId: cond_bytes,
            partition: vec![U256::from(1), U256::from(2)],
            amount: U256::from(amount_raw),
        };

        let ctf: Address = CTF_ADDRESS.parse()?;
        self.submit_safe_transaction(
            ctf,
            &call.abi_encode(),
            private_key,
            builder_api_key,
            builder_secret,
            builder_passphrase,
            "CTF Split",
        )
        .await
    }

    /// Merge YES + NO token pairs back into USDC collateral.
    ///
    /// For NegRisk markets, calls the NegRisk Adapter's simplified mergePositions(bytes32, uint256).
    /// For standard markets, calls CTF's mergePositions(address, bytes32, bytes32, uint256[], uint256).
    ///
    /// Token IDs (from CLOB) are used for on-chain balance verification.
    pub async fn merge(
        &self,
        condition_id: &str,
        amount: rust_decimal::Decimal,
        private_key: &str,
        builder_api_key: &str,
        builder_secret: &str,
        builder_passphrase: &str,
        yes_token_id: Option<&str>,
        no_token_id: Option<&str>,
        neg_risk: bool,
    ) -> Result<CtfResponse> {
        let ctf: Address = CTF_ADDRESS.parse()?;
        let cond_bytes = parse_condition_id(condition_id)?;

        // Derive Safe address to check on-chain balances
        let signer: PrivateKeySigner = private_key
            .parse()
            .context("Failed to parse private key")?;
        let eoa_address = signer.address();
        let safe_address_str =
            crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_address))
                .map_err(|e| anyhow::anyhow!("Failed to derive safe: {}", e))?;
        let safe_address: Address = safe_address_str
            .parse()
            .context("Failed to parse safe address")?;

        // Use CLOB token IDs for balance check (works for both NegRisk and standard)
        let (token_id_yes, token_id_no) = match (yes_token_id, no_token_id) {
            (Some(yes_str), Some(no_str)) => {
                let yes = U256::from_str_radix(yes_str, 10).unwrap_or(U256::ZERO);
                let no = U256::from_str_radix(no_str, 10).unwrap_or(U256::ZERO);
                (yes, no)
            }
            _ => {
                // Fallback: compute from condition_id (only correct for non-NegRisk)
                let usdc: Address = USDC_ADDRESS.parse()?;
                let yes = compute_position_id(&cond_bytes, 1, &usdc);
                let no = compute_position_id(&cond_bytes, 2, &usdc);
                warn!("CTF Merge: No CLOB token IDs, using computed IDs (may be wrong for NegRisk)");
                (yes, no)
            }
        };

        // Check on-chain balances before merging
        let bal_yes = self.check_ctf_balance(ctf, safe_address, token_id_yes).await
            .unwrap_or(U256::ZERO);
        let bal_no = self.check_ctf_balance(ctf, safe_address, token_id_no).await
            .unwrap_or(U256::ZERO);

        let amount_raw = to_raw_amount(amount);
        let min_balance = std::cmp::min(bal_yes, bal_no);

        info!(
            "CTF Merge: requested={} raw={} | on-chain YES={} NO={} min={} | neg_risk={} condition={} safe={}",
            amount, amount_raw, bal_yes, bal_no, min_balance, neg_risk,
            &condition_id[..12.min(condition_id.len())],
            &safe_address_str[..10]
        );

        if min_balance.is_zero() {
            warn!(
                "CTF Merge: Safe holds 0 tokens (YES={} NO={}). Cannot merge.",
                bal_yes, bal_no
            );
            return Ok(CtfResponse {
                success: false,
                transaction_id: None,
                error: Some(format!(
                    "Safe holds 0 tokens (YES={}, NO={}). Tokens may not have been delivered or were already redeemed.",
                    bal_yes, bal_no
                )),
            });
        }

        // Use the minimum of requested amount and actual on-chain balance
        let actual_raw = std::cmp::min(U256::from(amount_raw), min_balance);
        if actual_raw < U256::from(amount_raw) {
            warn!(
                "CTF Merge: Capping merge amount from {} to {} (on-chain balance limit)",
                amount_raw, actual_raw
            );
        }

        if neg_risk {
            // NegRisk: call NegRiskAdapter.mergePositions(conditionId, amount)
            let adapter: Address = NEG_RISK_ADAPTER.parse()?;
            let call = neg_risk_abi::mergePositionsCall {
                conditionId: cond_bytes,
                amount: actual_raw,
            };
            info!("CTF Merge: Using NegRisk Adapter at {:?}", adapter);
            self.submit_safe_transaction(
                adapter,
                &call.abi_encode(),
                private_key,
                builder_api_key,
                builder_secret,
                builder_passphrase,
                "CTF Merge (NegRisk)",
            )
            .await
        } else {
            // Standard: call CTF.mergePositions(USDC, 0, conditionId, [1,2], amount)
            let usdc: Address = USDC_ADDRESS.parse()?;
            let call = mergePositionsCall {
                collateralToken: usdc,
                parentCollectionId: B256::ZERO,
                conditionId: cond_bytes,
                partition: vec![U256::from(1), U256::from(2)],
                amount: actual_raw,
            };
            self.submit_safe_transaction(
                ctf,
                &call.abi_encode(),
                private_key,
                builder_api_key,
                builder_secret,
                builder_passphrase,
                "CTF Merge",
            )
            .await
        }
    }

    /// Redeem winning positions after market resolution.
    ///
    /// For NegRisk markets, calls NegRisk Adapter's redeemPositions(bytes32, uint256[]).
    /// For standard markets, calls CTF's redeemPositions(address, bytes32, bytes32, uint256[]).
    pub async fn redeem(
        &self,
        condition_id: &str,
        index_sets: &[u32],
        private_key: &str,
        builder_api_key: &str,
        builder_secret: &str,
        builder_passphrase: &str,
        neg_risk: bool,
    ) -> Result<CtfResponse> {
        info!(
            "CTF Redeem: condition {} index_sets {:?} neg_risk={}",
            &condition_id[..12.min(condition_id.len())],
            index_sets,
            neg_risk
        );

        let cond_bytes = parse_condition_id(condition_id)?;

        if neg_risk {
            let adapter: Address = NEG_RISK_ADAPTER.parse()?;
            let call = neg_risk_abi::redeemPositionsCall {
                conditionId: cond_bytes,
                amounts: index_sets.iter().map(|&i| U256::from(i)).collect(),
            };
            info!("CTF Redeem: Using NegRisk Adapter at {:?}", adapter);
            self.submit_safe_transaction(
                adapter,
                &call.abi_encode(),
                private_key,
                builder_api_key,
                builder_secret,
                builder_passphrase,
                "CTF Redeem (NegRisk)",
            )
            .await
        } else {
            let usdc: Address = USDC_ADDRESS.parse()?;
            let call = redeemPositionsCall {
                collateralToken: usdc,
                parentCollectionId: B256::ZERO,
                conditionId: cond_bytes,
                indexSets: index_sets.iter().map(|&i| U256::from(i)).collect(),
            };
            let ctf: Address = CTF_ADDRESS.parse()?;
            self.submit_safe_transaction(
                ctf,
                &call.abi_encode(),
                private_key,
                builder_api_key,
                builder_secret,
                builder_passphrase,
                "CTF Redeem",
            )
            .await
        }
    }

    /// Submit an ABI-encoded transaction as a Safe transaction via the relay
    async fn submit_safe_transaction(
        &self,
        to: Address,
        calldata: &[u8],
        private_key: &str,
        api_key: &str,
        secret: &str,
        passphrase: &str,
        metadata: &str,
    ) -> Result<CtfResponse> {
        let signer: PrivateKeySigner = private_key
            .parse()
            .context("Failed to parse private key")?;
        let eoa_address = signer.address();

        // Derive Safe address from EOA
        let safe_address_str =
            crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_address))
                .map_err(|e| anyhow::anyhow!("Failed to derive safe: {}", e))?;
        let safe_address: Address = safe_address_str
            .parse()
            .context("Failed to parse safe address")?;

        info!(
            "CTF relay: EOA={} Safe={} target={} calldata_len={}",
            format!("{:?}", eoa_address),
            safe_address_str,
            format!("{:?}", to),
            calldata.len()
        );

        // 1. Get nonce from relay
        let nonce = self
            .get_nonce(
                &format!("{:?}", eoa_address),
                api_key,
                secret,
                passphrase,
            )
            .await?;
        debug!("CTF relay nonce: {}", nonce);

        // 2. Compute EIP-712 Safe transaction hash
        let data_hex = format!("0x{}", hex::encode(calldata));
        let data_hash = compute_safe_tx_hash(safe_address, to, calldata, &nonce);

        // 3. Sign with personal_sign (Safe uses v > 30 to indicate eth_sign style)
        let signature = signer
            .sign_message(data_hash.as_slice())
            .await
            .context("Failed to sign Safe transaction")?;

        // 4. Pack signature with Safe v adjustment (v + 4)
        let r = signature.r();
        let s = signature.s();
        let v: u8 = if signature.v() { 32 } else { 31 }; // Safe: v=27+4=31 or v=28+4=32

        let mut packed_sig = Vec::with_capacity(65);
        packed_sig.extend_from_slice(&r.to_be_bytes::<32>());
        packed_sig.extend_from_slice(&s.to_be_bytes::<32>());
        packed_sig.push(v);
        let sig_hex = format!("0x{}", hex::encode(&packed_sig));

        // 5. Build transaction request
        let tx_request = serde_json::json!({
            "type": "SAFE",
            "from": format!("{:?}", eoa_address),
            "to": format!("{:?}", to),
            "proxyWallet": safe_address_str,
            "data": data_hex,
            "signature": sig_hex,
            "value": "0",
            "nonce": nonce,
            "signatureParams": {
                "gasPrice": "0",
                "operation": "0",
                "safeTxnGas": "0",
                "baseGas": "0",
                "gasToken": ZERO_ADDRESS,
                "refundReceiver": ZERO_ADDRESS
            },
            "metadata": metadata
        });

        let body = serde_json::to_string(&tx_request)?;
        debug!(
            "CTF relay submit body (first 200): {}",
            &body[..200.min(body.len())]
        );

        // 6. Submit with builder auth
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig_payload = format!("{}POST/submit{}", timestamp, body);
        let hmac_sig = compute_hmac(secret, &sig_payload)?;

        let response = self
            .client
            .post(format!("{}/submit", RELAY_URL))
            .header("Content-Type", "application/json")
            .header("POLY_BUILDER_TIMESTAMP", &timestamp)
            .header("POLY_BUILDER_SIGNATURE", &hmac_sig)
            .header("POLY_BUILDER_API_KEY", api_key)
            .header("POLY_BUILDER_PASSPHRASE", passphrase)
            .body(body)
            .send()
            .await
            .context("Failed to send relay request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            warn!("CTF relay submit failed: {} - {}", status, error_body);
            return Ok(CtfResponse {
                success: false,
                transaction_id: None,
                error: Some(format!("Relay error {}: {}", status, error_body)),
            });
        }

        #[derive(Debug, Deserialize)]
        struct SubmitResponse {
            #[serde(default, rename = "transactionID")]
            transaction_id: Option<String>,
            #[serde(default, rename = "transactionHash")]
            _transaction_hash: Option<String>,
        }

        let submit_resp: SubmitResponse = response
            .json()
            .await
            .context("Failed to parse submit response")?;

        let tx_id = submit_resp.transaction_id.unwrap_or_default();
        if tx_id.is_empty() {
            return Ok(CtfResponse {
                success: false,
                transaction_id: None,
                error: Some("No transaction ID returned".to_string()),
            });
        }
        info!("CTF relay submitted: tx_id={}, nonce={}", tx_id, nonce);

        // 7. Poll for completion
        self.poll_transaction(&tx_id, api_key, secret, passphrase)
            .await
    }

    /// Get nonce from relay
    async fn get_nonce(
        &self,
        eoa_address: &str,
        api_key: &str,
        secret: &str,
        passphrase: &str,
    ) -> Result<String> {
        let path = format!("/nonce?address={}&type=SAFE", eoa_address);
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig_payload = format!("{}GET{}", timestamp, path);
        let hmac_sig = compute_hmac(secret, &sig_payload)?;

        let url = format!("{}{}", RELAY_URL, path);
        let response = self
            .client
            .get(&url)
            .header("POLY_BUILDER_TIMESTAMP", &timestamp)
            .header("POLY_BUILDER_SIGNATURE", &hmac_sig)
            .header("POLY_BUILDER_API_KEY", api_key)
            .header("POLY_BUILDER_PASSPHRASE", passphrase)
            .send()
            .await
            .context("Failed to get nonce from relay")?;

        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            anyhow::bail!("Nonce request failed ({}): {}", status, err);
        }

        #[derive(Deserialize)]
        struct NonceResponse {
            nonce: serde_json::Value,
        }

        let resp: NonceResponse = response
            .json()
            .await
            .context("Failed to parse nonce response")?;

        match &resp.nonce {
            serde_json::Value::Number(n) => Ok(n.to_string()),
            serde_json::Value::String(s) => Ok(s.clone()),
            _ => anyhow::bail!("Unexpected nonce format: {:?}", resp.nonce),
        }
    }

    /// Poll for transaction completion
    async fn poll_transaction(
        &self,
        tx_id: &str,
        api_key: &str,
        secret: &str,
        passphrase: &str,
    ) -> Result<CtfResponse> {
        info!("CTF relay: polling for tx_id={}", tx_id);

        for i in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let path = format!("/transaction?id={}", tx_id);
            let timestamp = chrono::Utc::now().timestamp().to_string();
            let sig_payload = format!("{}GET{}", timestamp, path);
            let hmac_sig = match compute_hmac(secret, &sig_payload) {
                Ok(h) => h,
                Err(_) => continue,
            };

            let url = format!("{}{}", RELAY_URL, path);
            let response = self
                .client
                .get(&url)
                .header("POLY_BUILDER_TIMESTAMP", &timestamp)
                .header("POLY_BUILDER_SIGNATURE", &hmac_sig)
                .header("POLY_BUILDER_API_KEY", api_key)
                .header("POLY_BUILDER_PASSPHRASE", passphrase)
                .send()
                .await;

            if let Ok(resp) = response {
                if let Ok(txns) = resp.json::<Vec<serde_json::Value>>().await {
                    if let Some(txn) = txns.first() {
                        let state = txn
                            .get("state")
                            .and_then(|s| s.as_str())
                            .unwrap_or("");

                        match state {
                            "STATE_MINED" | "STATE_CONFIRMED" => {
                                let tx_hash = txn
                                    .get("transactionHash")
                                    .and_then(|h| h.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                info!("CTF operation confirmed: tx={}", tx_hash);
                                return Ok(CtfResponse {
                                    success: true,
                                    transaction_id: Some(tx_hash),
                                    error: None,
                                });
                            }
                            "STATE_FAILED" | "STATE_INVALID" => {
                                let tx_hash = txn
                                    .get("transactionHash")
                                    .and_then(|h| h.as_str())
                                    .map(|s| s.to_string());
                                warn!("CTF operation failed: full response: {}", serde_json::to_string_pretty(txn).unwrap_or_default());
                                let msg = format!(
                                    "Transaction {}: hash={:?}",
                                    state,
                                    tx_hash
                                );
                                warn!("CTF operation failed: {}", msg);
                                return Ok(CtfResponse {
                                    success: false,
                                    transaction_id: tx_hash,
                                    error: Some(msg),
                                });
                            }
                            _ => {
                                if i % 5 == 0 {
                                    debug!(
                                        "CTF relay polling: state={}, attempt {}/30",
                                        state,
                                        i + 1
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        warn!("CTF relay polling timed out for tx_id={}", tx_id);
        Ok(CtfResponse {
            success: false,
            transaction_id: Some(tx_id.to_string()),
            error: Some("Transaction polling timed out after 60s".to_string()),
        })
    }

    /// Public method to get token balance for a wallet's Safe address
    /// Returns the balance as a Decimal (scaled from raw 1e6)
    pub async fn get_token_balance(
        &self,
        private_key: &str,
        token_id: &str,
    ) -> Result<Decimal> {
        let ctf: Address = CTF_ADDRESS.parse()?;

        // Derive Safe address from private key
        let safe_address_str = crate::services::mint_maker::order_manager::derive_safe_address(private_key)
            .context("Failed to derive safe address")?;
        let safe_address: Address = safe_address_str
            .parse()
            .context("Failed to parse safe address")?;

        // Parse token ID
        let token_id_u256 = U256::from_str_radix(token_id, 10).unwrap_or(U256::ZERO);

        // Get raw balance
        let raw_balance = self.check_ctf_balance(ctf, safe_address, token_id_u256).await?;

        // Convert from raw (1e6 scaled) to Decimal
        let raw_str = raw_balance.to_string();
        let raw_decimal = Decimal::from_str(&raw_str).unwrap_or(Decimal::ZERO);
        let scale = Decimal::from(1_000_000);

        Ok(raw_decimal / scale)
    }

    /// Check ERC-1155 token balance on the CTF contract via Polygon RPC
    async fn check_ctf_balance(
        &self,
        ctf_address: Address,
        account: Address,
        token_id: U256,
    ) -> Result<U256> {
        let call = balanceOfCall {
            account,
            id: token_id,
        };
        let calldata = format!("0x{}", hex::encode(call.abi_encode()));

        let rpc_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": format!("{:?}", ctf_address),
                "data": calldata,
            }, "latest"],
            "id": 1
        });

        let resp = self
            .client
            .post(POLYGON_RPC)
            .json(&rpc_payload)
            .send()
            .await
            .context("Polygon RPC call failed")?;

        let json: serde_json::Value = resp.json().await
            .context("Failed to parse RPC response")?;

        if let Some(error) = json.get("error") {
            anyhow::bail!("RPC error: {}", error);
        }

        let result = json["result"]
            .as_str()
            .unwrap_or("0x0");
        let hex_str = result.strip_prefix("0x").unwrap_or(result);
        Ok(U256::from_str_radix(hex_str, 16).unwrap_or(U256::ZERO))
    }
}

/// Compute EIP-712 Safe transaction hash
fn compute_safe_tx_hash(safe_address: Address, to: Address, data: &[u8], nonce: &str) -> B256 {
    // Domain separator: keccak256(abi.encode(typehash, chainId, verifyingContract))
    let domain_typehash =
        keccak256(b"EIP712Domain(uint256 chainId,address verifyingContract)");
    let mut domain_data = Vec::with_capacity(96);
    domain_data.extend_from_slice(domain_typehash.as_slice());
    domain_data.extend_from_slice(&U256::from(CHAIN_ID).to_be_bytes::<32>());
    let mut addr_padded = [0u8; 32];
    addr_padded[12..].copy_from_slice(safe_address.as_slice());
    domain_data.extend_from_slice(&addr_padded);
    let domain_separator = keccak256(&domain_data);

    // Safe TX struct hash
    let safe_tx_typehash = keccak256(
        b"SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)"
    );

    let data_hash = keccak256(data);
    let nonce_u256 = U256::from_str_radix(nonce, 10).unwrap_or(U256::ZERO);

    // abi.encode all fields: typehash + 10 fields = 352 bytes
    let mut struct_data = Vec::with_capacity(352);
    struct_data.extend_from_slice(safe_tx_typehash.as_slice());

    // to (address, left-padded to 32 bytes)
    let mut to_padded = [0u8; 32];
    to_padded[12..].copy_from_slice(to.as_slice());
    struct_data.extend_from_slice(&to_padded);

    // value = 0
    struct_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

    // keccak256(data) — EIP-712 encodes bytes as their hash
    struct_data.extend_from_slice(data_hash.as_slice());

    // operation = 0 (Call)
    struct_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

    // safeTxGas = 0
    struct_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

    // baseGas = 0
    struct_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

    // gasPrice = 0
    struct_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

    // gasToken = address(0)
    struct_data.extend_from_slice(&[0u8; 32]);

    // refundReceiver = address(0)
    struct_data.extend_from_slice(&[0u8; 32]);

    // nonce
    struct_data.extend_from_slice(&nonce_u256.to_be_bytes::<32>());

    let struct_hash = keccak256(&struct_data);

    // Final EIP-712 hash: 0x19 0x01 + domainSeparator + structHash
    let mut final_data = Vec::with_capacity(66);
    final_data.push(0x19);
    final_data.push(0x01);
    final_data.extend_from_slice(domain_separator.as_slice());
    final_data.extend_from_slice(struct_hash.as_slice());

    keccak256(&final_data)
}

/// Parse condition_id hex string to B256
fn parse_condition_id(condition_id: &str) -> Result<B256> {
    let hex_str = condition_id.strip_prefix("0x").unwrap_or(condition_id);
    let bytes = hex::decode(hex_str).context("Invalid condition ID hex")?;
    if bytes.len() != 32 {
        anyhow::bail!(
            "Condition ID must be 32 bytes, got {}",
            bytes.len()
        );
    }
    Ok(B256::from_slice(&bytes))
}

/// Convert Decimal amount (whole tokens) to raw 6-decimal units
fn to_raw_amount(amount: rust_decimal::Decimal) -> u64 {
    use rust_decimal::prelude::*;
    let raw = amount * rust_decimal::Decimal::from(1_000_000);
    raw.to_u64().unwrap_or(0)
}

/// Compute CTF position ID (ERC-1155 token ID) from condition_id and index set.
///
/// For standard (non-NegRisk) markets with parentCollectionId = 0:
///   collectionId = keccak256(abi.encodePacked(conditionId, uint256(indexSet)))
///   positionId = uint256(keccak256(abi.encodePacked(collateralToken, collectionId)))
fn compute_position_id(condition_id: &B256, index_set: u32, collateral_token: &Address) -> U256 {
    // collectionId = keccak256(abi.encodePacked(conditionId, uint256(indexSet)))
    let mut packed = Vec::with_capacity(64);
    packed.extend_from_slice(condition_id.as_slice());
    packed.extend_from_slice(&U256::from(index_set).to_be_bytes::<32>());
    let collection_id = keccak256(&packed);

    // positionId = uint256(keccak256(abi.encodePacked(collateralToken, collectionId)))
    let mut packed2 = Vec::with_capacity(52);
    packed2.extend_from_slice(collateral_token.as_slice());
    packed2.extend_from_slice(collection_id.as_slice());
    U256::from_be_bytes(keccak256(&packed2).into())
}

/// Compute HMAC-SHA256 signature for builder auth
fn compute_hmac(secret: &str, payload: &str) -> Result<String> {
    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(secret))
        .context("Failed to decode builder secret")?;

    let mut mac =
        HmacSha256::new_from_slice(&secret_bytes).context("Invalid HMAC key")?;
    mac.update(payload.as_bytes());
    Ok(base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes()))
}
