//! Safe proxy activation for generated wallets
//! Handles deploying the Gnosis Safe proxy and setting on-chain ERC-20/ERC-1155 approvals
//! via Polymarket's relayer service.

use alloy::primitives::{keccak256, Address, U256};
use alloy::signers::{local::PrivateKeySigner, Signer};
use anyhow::{Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::info;

type HmacSha256 = Hmac<Sha256>;

const RELAY_URL: &str = "https://relayer-v2.polymarket.com";
const POLYGON_CHAIN_ID: u64 = 137;

// Contract addresses on Polygon
const SAFE_FACTORY: &str = "0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b";
const SAFE_MULTISEND: &str = "0xA238CBeb142c10Ef7Ad8442C6D1f9E89e07e7761";
const USDC_E: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const CTF: &str = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045";
const CTF_EXCHANGE: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";
const NEG_RISK_CTF_EXCHANGE: &str = "0xC5d563A36AE78145C45a50134d48A1215220f80a";
const NEG_RISK_ADAPTER: &str = "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296";

// EIP-712 domain name for SafeFactory
const SAFE_FACTORY_NAME: &str = "Polymarket Contract Proxy Factory";

// Max uint256 for unlimited approval
const MAX_APPROVAL: &str = "115792089237316195423570985008687907853269984665640564039457584007913129639935";

pub struct BuilderCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

/// Create builder auth headers for relay requests
fn create_builder_headers(
    creds: &BuilderCredentials,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(String, String)> {
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    let secret_bytes = base64::engine::general_purpose::STANDARD
        .decode(&creds.secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&creds.secret))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&creds.secret))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)?;
    mac.update(sig_payload.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    Ok((timestamp, signature))
}

/// Make an authenticated request to the relay
async fn relay_request(
    client: &reqwest::Client,
    creds: &BuilderCredentials,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let body_str = body.map(|b| serde_json::to_string(b).unwrap_or_default()).unwrap_or_default();
    let (timestamp, signature) = create_builder_headers(creds, method, path, &body_str)?;

    let url = format!("{}{}", RELAY_URL, path);

    let mut req = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        _ => anyhow::bail!("Unsupported method: {}", method),
    };

    req = req
        .header("POLY_BUILDER_TIMESTAMP", &timestamp)
        .header("POLY_BUILDER_SIGNATURE", &signature)
        .header("POLY_BUILDER_API_KEY", &creds.api_key)
        .header("POLY_BUILDER_PASSPHRASE", &creds.passphrase)
        .header("Content-Type", "application/json");

    if let Some(b) = body {
        req = req.json(b);
    }

    let response = req.send().await?;
    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Relay error ({}): {}", status, text);
    }

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    Ok(json)
}

/// Check if the Safe proxy is deployed on-chain
pub async fn check_safe_deployed(
    client: &reqwest::Client,
    creds: &BuilderCredentials,
    safe_address: &str,
) -> Result<bool> {
    let path = format!("/deployed?address={}", safe_address);
    let result = relay_request(client, creds, "GET", &path, None).await?;
    Ok(result.get("deployed").and_then(|v| v.as_bool()).unwrap_or(false))
}

/// Deploy the Safe proxy via the relay
pub async fn deploy_safe(
    signer: &PrivateKeySigner,
    client: &reqwest::Client,
    creds: &BuilderCredentials,
    safe_address: &str,
) -> Result<String> {
    let eoa = signer.address();
    let factory: Address = SAFE_FACTORY.parse()?;

    // EIP-712 typed data for CreateProxy
    // Domain: {name: "Polymarket Contract Proxy Factory", chainId: 137, verifyingContract: SafeFactory}
    // Type: CreateProxy(address paymentToken, uint256 payment, address paymentReceiver)
    // Values: {paymentToken: 0x0, payment: 0, paymentReceiver: 0x0}

    // Domain type hash: keccak256("EIP712Domain(string name,uint256 chainId,address verifyingContract)")
    let domain_type_hash = keccak256(b"EIP712Domain(string name,uint256 chainId,address verifyingContract)");

    // Domain separator
    let name_hash = keccak256(SAFE_FACTORY_NAME.as_bytes());
    let mut domain_data = Vec::with_capacity(128);
    domain_data.extend_from_slice(domain_type_hash.as_slice());
    domain_data.extend_from_slice(name_hash.as_slice());
    domain_data.extend_from_slice(&U256::from(POLYGON_CHAIN_ID).to_be_bytes::<32>());
    let mut factory_padded = [0u8; 32];
    factory_padded[12..].copy_from_slice(factory.as_slice());
    domain_data.extend_from_slice(&factory_padded);
    let domain_separator = keccak256(&domain_data);

    // Struct type hash: keccak256("CreateProxy(address paymentToken,uint256 payment,address paymentReceiver)")
    let struct_type_hash = keccak256(b"CreateProxy(address paymentToken,uint256 payment,address paymentReceiver)");

    // Struct hash: keccak256(abi.encode(typeHash, paymentToken, payment, paymentReceiver))
    // All values are zero
    let mut struct_data = Vec::with_capacity(128);
    struct_data.extend_from_slice(struct_type_hash.as_slice());
    struct_data.extend_from_slice(&[0u8; 32]); // paymentToken = address(0)
    struct_data.extend_from_slice(&[0u8; 32]); // payment = 0
    struct_data.extend_from_slice(&[0u8; 32]); // paymentReceiver = address(0)
    let struct_hash = keccak256(&struct_data);

    // EIP-712 signing hash: keccak256("\x19\x01" ++ domainSeparator ++ structHash)
    let mut signing_input = Vec::with_capacity(66);
    signing_input.push(0x19);
    signing_input.push(0x01);
    signing_input.extend_from_slice(domain_separator.as_slice());
    signing_input.extend_from_slice(struct_hash.as_slice());
    let signing_hash = keccak256(&signing_input);

    // Sign with EIP-712 (sign the hash directly, no prefix)
    let sig = signer.sign_hash(&signing_hash).await?;
    let sig_hex = format!("0x{}", hex::encode(sig.as_bytes()));

    // Submit to relay
    let body = serde_json::json!({
        "from": format!("{:?}", eoa),
        "to": SAFE_FACTORY,
        "proxyWallet": safe_address,
        "data": "0x",
        "signature": sig_hex,
        "signatureParams": {
            "paymentToken": "0x0000000000000000000000000000000000000000",
            "payment": "0",
            "paymentReceiver": "0x0000000000000000000000000000000000000000"
        },
        "type": "SAFE-CREATE"
    });

    info!("Deploying Safe for {} -> {}", eoa, safe_address);
    let result = relay_request(client, creds, "POST", "/submit", Some(&body)).await?;

    let tx_id = result.get("transactionID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    info!("Safe deploy submitted: tx_id={}", tx_id);
    Ok(tx_id)
}

/// Build ERC-20 approve calldata: approve(address spender, uint256 amount)
fn build_approve_data(spender: &Address, amount: &U256) -> Vec<u8> {
    // Function selector: keccak256("approve(address,uint256)")[:4]
    let selector = &keccak256(b"approve(address,uint256)")[..4];
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(selector);
    // address spender (left-padded to 32 bytes)
    let mut spender_padded = [0u8; 32];
    spender_padded[12..].copy_from_slice(spender.as_slice());
    data.extend_from_slice(&spender_padded);
    // uint256 amount
    data.extend_from_slice(&amount.to_be_bytes::<32>());
    data
}

/// Build ERC-1155 setApprovalForAll calldata: setApprovalForAll(address operator, bool approved)
fn build_set_approval_for_all_data(operator: &Address, approved: bool) -> Vec<u8> {
    // Function selector: keccak256("setApprovalForAll(address,bool)")[:4]
    let selector = &keccak256(b"setApprovalForAll(address,bool)")[..4];
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(selector);
    // address operator
    let mut operator_padded = [0u8; 32];
    operator_padded[12..].copy_from_slice(operator.as_slice());
    data.extend_from_slice(&operator_padded);
    // bool approved
    let mut bool_padded = [0u8; 32];
    bool_padded[31] = if approved { 1 } else { 0 };
    data.extend_from_slice(&bool_padded);
    data
}

/// Encode multiple Safe transactions into a multisend call
fn encode_multisend(txns: &[(Address, Vec<u8>)]) -> (Address, Vec<u8>, u8) {
    let multisend_addr: Address = SAFE_MULTISEND.parse().unwrap();

    if txns.len() == 1 {
        // Single transaction - no multisend needed
        return (txns[0].0, txns[0].1.clone(), 0); // operation = Call
    }

    // Encode packed: uint8 operation ++ address to ++ uint256 value ++ uint256 dataLength ++ bytes data
    let mut packed = Vec::new();
    for (to, data) in txns {
        packed.push(0u8); // operation = Call (0)
        // address to (20 bytes, NOT padded)
        packed.extend_from_slice(to.as_slice());
        // uint256 value = 0 (32 bytes)
        packed.extend_from_slice(&[0u8; 32]);
        // uint256 data length (32 bytes)
        packed.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
        // bytes data
        packed.extend_from_slice(data);
    }

    // Encode multiSend(bytes transactions) call
    // Function selector: keccak256("multiSend(bytes)")[:4] = 0x8d80ff0a
    let selector = &keccak256(b"multiSend(bytes)")[..4];
    let mut calldata = Vec::new();
    calldata.extend_from_slice(selector);
    // ABI encode bytes: offset (32) + length (32) + data (padded to 32)
    calldata.extend_from_slice(&U256::from(32u64).to_be_bytes::<32>()); // offset
    calldata.extend_from_slice(&U256::from(packed.len()).to_be_bytes::<32>()); // length
    calldata.extend_from_slice(&packed);
    // Pad to 32 bytes
    let remainder = packed.len() % 32;
    if remainder != 0 {
        calldata.extend_from_slice(&vec![0u8; 32 - remainder]);
    }

    (multisend_addr, calldata, 1) // operation = DelegateCall (1) for multisend
}

/// Set all on-chain approvals via Safe transactions through the relay
pub async fn set_safe_approvals(
    signer: &PrivateKeySigner,
    client: &reqwest::Client,
    creds: &BuilderCredentials,
    safe_address: &str,
) -> Result<String> {
    let eoa = signer.address();
    let safe_addr: Address = safe_address.parse()?;

    // Get nonce from relay
    let nonce_path = format!("/nonce?address={:?}&type=SAFE", eoa);
    let nonce_result = relay_request(client, creds, "GET", &nonce_path, None).await?;
    let nonce_str = if let Some(n) = nonce_result.get("nonce").and_then(|v| v.as_u64()) {
        n.to_string()
    } else if let Some(s) = nonce_result.get("nonce").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        "0".to_string()
    };
    let nonce_u256 = U256::from_str_radix(&nonce_str, 10).unwrap_or(U256::ZERO);

    info!("Safe nonce for {:?}: {}", eoa, nonce_str);

    // Build approval transactions
    let ctf_exchange: Address = CTF_EXCHANGE.parse()?;
    let neg_risk_exchange: Address = NEG_RISK_CTF_EXCHANGE.parse()?;
    let neg_risk_adapter: Address = NEG_RISK_ADAPTER.parse()?;
    let usdc_addr: Address = USDC_E.parse()?;
    let ctf_addr: Address = CTF.parse()?;
    let max_amount = U256::from_str_radix(MAX_APPROVAL, 10)?;

    let txns: Vec<(Address, Vec<u8>)> = vec![
        // USDC.e approvals for exchanges
        (usdc_addr, build_approve_data(&ctf_exchange, &max_amount)),
        (usdc_addr, build_approve_data(&neg_risk_exchange, &max_amount)),
        // CTF (ERC-1155) approvals
        (ctf_addr, build_set_approval_for_all_data(&ctf_exchange, true)),
        (ctf_addr, build_set_approval_for_all_data(&neg_risk_exchange, true)),
        (ctf_addr, build_set_approval_for_all_data(&neg_risk_adapter, true)),
    ];

    let (to, data, operation) = encode_multisend(&txns);

    // Compute SafeTx EIP-712 struct hash
    // Domain: {chainId: 137, verifyingContract: safeAddress} (no name!)
    let domain_type_hash = keccak256(b"EIP712Domain(uint256 chainId,address verifyingContract)");
    let mut domain_data = Vec::with_capacity(96);
    domain_data.extend_from_slice(domain_type_hash.as_slice());
    domain_data.extend_from_slice(&U256::from(POLYGON_CHAIN_ID).to_be_bytes::<32>());
    let mut safe_padded = [0u8; 32];
    safe_padded[12..].copy_from_slice(safe_addr.as_slice());
    domain_data.extend_from_slice(&safe_padded);
    let domain_separator = keccak256(&domain_data);

    // SafeTx struct type hash
    let safe_tx_type_hash = keccak256(
        b"SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)"
    );

    // Encode struct: typeHash, to, value, keccak256(data), operation, safeTxGas, baseGas, gasPrice, gasToken, refundReceiver, nonce
    let data_hash = keccak256(&data);
    let mut struct_data = Vec::with_capacity(352);
    struct_data.extend_from_slice(safe_tx_type_hash.as_slice());
    // to
    let mut to_padded = [0u8; 32];
    to_padded[12..].copy_from_slice(to.as_slice());
    struct_data.extend_from_slice(&to_padded);
    // value = 0
    struct_data.extend_from_slice(&[0u8; 32]);
    // keccak256(data)
    struct_data.extend_from_slice(data_hash.as_slice());
    // operation
    struct_data.extend_from_slice(&U256::from(operation).to_be_bytes::<32>());
    // safeTxGas = 0
    struct_data.extend_from_slice(&[0u8; 32]);
    // baseGas = 0
    struct_data.extend_from_slice(&[0u8; 32]);
    // gasPrice = 0
    struct_data.extend_from_slice(&[0u8; 32]);
    // gasToken = address(0)
    struct_data.extend_from_slice(&[0u8; 32]);
    // refundReceiver = address(0)
    struct_data.extend_from_slice(&[0u8; 32]);
    // nonce
    struct_data.extend_from_slice(&nonce_u256.to_be_bytes::<32>());
    let struct_hash = keccak256(&struct_data);

    // EIP-712 signing hash
    let mut signing_input = Vec::with_capacity(66);
    signing_input.push(0x19);
    signing_input.push(0x01);
    signing_input.extend_from_slice(domain_separator.as_slice());
    signing_input.extend_from_slice(struct_hash.as_slice());
    let signing_hash = keccak256(&signing_input);

    // Sign with signMessage (personal_sign - adds Ethereum prefix)
    // Safe contract expects eth_sign signature type (v=31 or v=32)
    let sig = signer.sign_message(signing_hash.as_slice()).await?;
    let sig_bytes = sig.as_bytes();

    // Pack signature with adjusted v: Safe's eth_sign type
    // The signer returns v=27 or v=28, we need v=31 or v=32
    let mut packed_sig = Vec::with_capacity(65);
    packed_sig.extend_from_slice(&sig_bytes[..64]); // r + s (64 bytes)
    let v = sig_bytes[64];
    let adjusted_v = match v {
        0 | 1 => v + 31,
        27 | 28 => v + 4,
        _ => v, // Already adjusted
    };
    packed_sig.push(adjusted_v);

    // Encode packed signature as hex with abi encoding (uint256 r, uint256 s, uint8 v)
    let r = U256::from_be_slice(&sig_bytes[..32]);
    let s = U256::from_be_slice(&sig_bytes[32..64]);
    let mut encoded_sig = Vec::with_capacity(65);
    encoded_sig.extend_from_slice(&r.to_be_bytes::<32>());
    encoded_sig.extend_from_slice(&s.to_be_bytes::<32>());
    encoded_sig.push(adjusted_v);
    let packed_sig_hex = format!("0x{}", hex::encode(&encoded_sig));

    // Submit to relay
    let data_hex = format!("0x{}", hex::encode(&data));
    let body = serde_json::json!({
        "from": format!("{:?}", eoa),
        "to": format!("{:?}", to),
        "proxyWallet": safe_address,
        "data": data_hex,
        "nonce": nonce_str,
        "signature": packed_sig_hex,
        "signatureParams": {
            "gasPrice": "0",
            "operation": format!("{}", operation),
            "safeTxnGas": "0",
            "baseGas": "0",
            "gasToken": "0x0000000000000000000000000000000000000000",
            "refundReceiver": "0x0000000000000000000000000000000000000000"
        },
        "type": "SAFE",
        "metadata": "Set all token approvals for trading"
    });

    info!("Setting Safe approvals for {} (safe={})", eoa, safe_address);
    let result = relay_request(client, creds, "POST", "/submit", Some(&body)).await?;

    let tx_id = result.get("transactionID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    info!("Safe approval tx submitted: tx_id={}", tx_id);
    Ok(tx_id)
}

/// Execute a single Safe transaction (Call) via the relay.
/// Used for withdrawals (USDC transfer from Safe to EOA).
pub async fn execute_safe_transaction(
    signer: &PrivateKeySigner,
    client: &reqwest::Client,
    creds: &BuilderCredentials,
    safe_address: &str,
    to: Address,
    data: &[u8],
) -> Result<String> {
    let eoa = signer.address();
    let safe_addr: Address = safe_address.parse()?;
    let operation: u8 = 0; // Call

    // Get nonce from relay
    let nonce_path = format!("/nonce?address={:?}&type=SAFE", eoa);
    let nonce_result = relay_request(client, creds, "GET", &nonce_path, None).await?;
    let nonce_str = if let Some(n) = nonce_result.get("nonce").and_then(|v| v.as_u64()) {
        n.to_string()
    } else if let Some(s) = nonce_result.get("nonce").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        "0".to_string()
    };
    let nonce_u256 = U256::from_str_radix(&nonce_str, 10).unwrap_or(U256::ZERO);

    // Compute SafeTx EIP-712 hash (same pattern as set_safe_approvals)
    let domain_type_hash = keccak256(b"EIP712Domain(uint256 chainId,address verifyingContract)");
    let mut domain_data = Vec::with_capacity(96);
    domain_data.extend_from_slice(domain_type_hash.as_slice());
    domain_data.extend_from_slice(&U256::from(POLYGON_CHAIN_ID).to_be_bytes::<32>());
    let mut safe_padded = [0u8; 32];
    safe_padded[12..].copy_from_slice(safe_addr.as_slice());
    domain_data.extend_from_slice(&safe_padded);
    let domain_separator = keccak256(&domain_data);

    let safe_tx_type_hash = keccak256(
        b"SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)"
    );

    let data_hash = keccak256(data);
    let mut struct_data = Vec::with_capacity(352);
    struct_data.extend_from_slice(safe_tx_type_hash.as_slice());
    let mut to_padded = [0u8; 32];
    to_padded[12..].copy_from_slice(to.as_slice());
    struct_data.extend_from_slice(&to_padded);
    struct_data.extend_from_slice(&[0u8; 32]); // value = 0
    struct_data.extend_from_slice(data_hash.as_slice());
    struct_data.extend_from_slice(&U256::from(operation).to_be_bytes::<32>());
    struct_data.extend_from_slice(&[0u8; 32]); // safeTxGas
    struct_data.extend_from_slice(&[0u8; 32]); // baseGas
    struct_data.extend_from_slice(&[0u8; 32]); // gasPrice
    struct_data.extend_from_slice(&[0u8; 32]); // gasToken
    struct_data.extend_from_slice(&[0u8; 32]); // refundReceiver
    struct_data.extend_from_slice(&nonce_u256.to_be_bytes::<32>());
    let struct_hash = keccak256(&struct_data);

    // EIP-712 signing hash
    let mut signing_input = Vec::with_capacity(66);
    signing_input.push(0x19);
    signing_input.push(0x01);
    signing_input.extend_from_slice(domain_separator.as_slice());
    signing_input.extend_from_slice(struct_hash.as_slice());
    let signing_hash = keccak256(&signing_input);

    // Sign with personal_sign, adjust v for Safe's eth_sign type
    let sig = signer.sign_message(signing_hash.as_slice()).await?;
    let sig_bytes = sig.as_bytes();

    let v = sig_bytes[64];
    let adjusted_v = match v {
        0 | 1 => v + 31,
        27 | 28 => v + 4,
        _ => v,
    };

    let r = U256::from_be_slice(&sig_bytes[..32]);
    let s = U256::from_be_slice(&sig_bytes[32..64]);
    let mut encoded_sig = Vec::with_capacity(65);
    encoded_sig.extend_from_slice(&r.to_be_bytes::<32>());
    encoded_sig.extend_from_slice(&s.to_be_bytes::<32>());
    encoded_sig.push(adjusted_v);
    let packed_sig_hex = format!("0x{}", hex::encode(&encoded_sig));

    let data_hex = format!("0x{}", hex::encode(data));
    let body = serde_json::json!({
        "from": format!("{:?}", eoa),
        "to": format!("{:?}", to),
        "proxyWallet": safe_address,
        "data": data_hex,
        "nonce": nonce_str,
        "signature": packed_sig_hex,
        "signatureParams": {
            "gasPrice": "0",
            "operation": "0",
            "safeTxnGas": "0",
            "baseGas": "0",
            "gasToken": "0x0000000000000000000000000000000000000000",
            "refundReceiver": "0x0000000000000000000000000000000000000000"
        },
        "type": "SAFE",
        "metadata": "Withdraw USDC from Safe"
    });

    info!("Executing Safe tx for {} (safe={})", eoa, safe_address);
    let result = relay_request(client, creds, "POST", "/submit", Some(&body)).await?;

    let tx_id = result
        .get("transactionID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    info!("Safe tx submitted: tx_id={}", tx_id);
    Ok(tx_id)
}

/// Ensure the Safe wallet is fully activated (deployed + approvals set)
/// Returns the Safe proxy address
pub async fn ensure_safe_activated(
    signer: &PrivateKeySigner,
    builder_creds: &BuilderCredentials,
) -> Result<String> {
    let eoa = signer.address();

    // Derive Safe address from EOA
    let safe_address = crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa))
        .map_err(|e| anyhow::anyhow!("Failed to derive Safe address: {}", e))?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    // Step 1: Check if Safe is deployed
    let is_deployed = check_safe_deployed(&client, builder_creds, &safe_address).await
        .unwrap_or(false);

    if !is_deployed {
        info!("Safe not deployed for {}, deploying...", safe_address);
        deploy_safe(signer, &client, builder_creds, &safe_address).await
            .context("Failed to deploy Safe")?;

        // Poll for deployment
        for i in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let deployed = check_safe_deployed(&client, builder_creds, &safe_address).await
                .unwrap_or(false);
            if deployed {
                info!("Safe deployed after {} checks", i + 1);
                break;
            }
        }

        // Verify
        let deployed = check_safe_deployed(&client, builder_creds, &safe_address).await
            .unwrap_or(false);
        if !deployed {
            anyhow::bail!("Safe deployment may still be pending. Try again in a moment.");
        }

        // Wait before setting approvals
        info!("Safe deployed, waiting 10s before setting approvals...");
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    } else {
        info!("Safe already deployed: {}", safe_address);
    }

    // Step 2: Set approvals
    // Check if USDC allowance is already set (via RPC)
    let has_allowance = check_usdc_allowance(&client, &safe_address).await.unwrap_or(false);

    if !has_allowance {
        info!("Setting on-chain approvals for Safe {}...", safe_address);
        set_safe_approvals(signer, &client, builder_creds, &safe_address).await
            .context("Failed to set Safe approvals")?;

        // Poll for allowance confirmation
        for i in 0..40 {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let has_it = check_usdc_allowance(&client, &safe_address).await.unwrap_or(false);
            if has_it {
                info!("Allowances confirmed after {} checks", i + 1);
                break;
            }
        }
    } else {
        info!("Safe already has USDC allowance: {}", safe_address);
    }

    Ok(safe_address)
}

/// Check if the Safe has USDC allowance for CTF_EXCHANGE via RPC
async fn check_usdc_allowance(client: &reqwest::Client, safe_address: &str) -> Result<bool> {
    // Use a public Polygon RPC
    let rpc_urls = [
        "https://polygon-rpc.com",
        "https://rpc-mainnet.matic.quiknode.pro",
        "https://polygon.llamarpc.com",
    ];

    let ctf_exchange: Address = CTF_EXCHANGE.parse()?;
    let safe_addr: Address = safe_address.parse()?;

    // Build allowance(address,address) calldata
    let selector = &keccak256(b"allowance(address,address)")[..4];
    let mut calldata = Vec::with_capacity(68);
    calldata.extend_from_slice(selector);
    let mut owner_padded = [0u8; 32];
    owner_padded[12..].copy_from_slice(safe_addr.as_slice());
    calldata.extend_from_slice(&owner_padded);
    let mut spender_padded = [0u8; 32];
    spender_padded[12..].copy_from_slice(ctf_exchange.as_slice());
    calldata.extend_from_slice(&spender_padded);

    let data_hex = format!("0x{}", hex::encode(&calldata));

    let rpc_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{
            "to": USDC_E,
            "data": data_hex
        }, "latest"]
    });

    for rpc_url in &rpc_urls {
        match client.post(*rpc_url)
            .json(&rpc_body)
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(result) = json.get("result").and_then(|v| v.as_str()) {
                        // Parse as uint256 - if > 0, allowance is set
                        if result != "0x" && result != "0x0000000000000000000000000000000000000000000000000000000000000000" {
                            return Ok(true);
                        }
                        return Ok(false);
                    }
                }
            }
            Err(_) => continue,
        }
    }

    Ok(false)
}
