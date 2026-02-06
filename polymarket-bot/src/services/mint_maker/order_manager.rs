//! Order management for Mint Maker - places/cancels GTC limit orders via CLOB API

use anyhow::Result;
use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use chrono::Utc;
use polymarket_client_sdk::clob::{
    Client as ClobClient, Config as ClobConfig,
};
use polymarket_client_sdk::clob::types::{
    AssetType, OrderType as ClobOrderType, Side as ClobSide, SignatureType,
    request::UpdateBalanceAllowanceRequest,
};
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{info, warn};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Place a GTC BUY order for a specific number of shares (maker-only at below-market prices)
pub async fn place_gtc_bid(
    private_key: &str,
    token_id: &str,
    price: Decimal,
    shares: Decimal,
) -> Result<String> {
    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    // Log the EOA and derived Safe address for debugging
    let eoa_addr = signer.address();
    let safe_addr = crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_addr))
        .unwrap_or_else(|_| "unknown".to_string());
    info!("place_gtc_bid: EOA={:?}, Safe={}, using GnosisSafe signature type", eoa_addr, safe_addr);

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    // Use GnosisSafe signature type so the SDK auto-derives the Safe proxy
    // address as the funder/maker. Generated wallets hold USDC in the Safe,
    // not the EOA — without this, the CLOB checks the empty EOA for balance.
    let client = ClobClient::new(CLOB_ENDPOINT, clob_config)?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await?;

    let token_id_u256 = U256::from_str_radix(token_id, 10)?;

    // Round price down to tick size (0.01 = 2 decimals for most markets).
    // The CLOB rejects prices with more decimal places than the tick size allows.
    // We round down (truncate) so the bid stays at or below the intended price.
    let price = price.trunc_with_scale(2);

    info!("Placing GTC limit BUY: token={}, price={}, shares={}", token_id, price, shares);

    // Use limit_order() builder for GTC orders (not market_order() which is for FAK/FOK).
    // limit_order takes .size() (shares) and .price() directly.
    // For Buy: taker_amount = size (shares), maker_amount = size * price (USDC).
    // GTC limit order at scanner price. If the bid crosses the spread it fills
    // immediately. On 15-min crypto markets, resting orders get maker (0% fee)
    // while crossing orders pay taker fees (up to 3%). Placing at scanner price
    // gives fast fills while potentially qualifying as maker on thin fresh books.
    let order = client
        .limit_order()
        .token_id(token_id_u256)
        .size(shares)
        .side(ClobSide::Buy)
        .price(price)
        .order_type(ClobOrderType::GTC)
        .build()
        .await?;

    let signed_order = client.sign(&signer, order).await?;
    let response = client.post_order(signed_order).await?;

    info!("GTC order placed: id={}", response.order_id);
    Ok(response.order_id)
}

/// Place a GTC SELL order for a specific number of shares.
/// Used by stop loss to exit a half-filled position.
pub async fn place_gtc_sell(
    private_key: &str,
    token_id: &str,
    price: Decimal,
    shares: Decimal,
) -> Result<String> {
    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    let eoa_addr = signer.address();
    let safe_addr = crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_addr))
        .unwrap_or_else(|_| "unknown".to_string());
    info!("place_gtc_sell: EOA={:?}, Safe={}, using GnosisSafe signature type", eoa_addr, safe_addr);

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let client = ClobClient::new(CLOB_ENDPOINT, clob_config)?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await?;

    let token_id_u256 = U256::from_str_radix(token_id, 10)?;
    let price = price.trunc_with_scale(2);

    info!("Placing GTC limit SELL: token={}, price={}, shares={}", token_id, price, shares);

    let order = client
        .limit_order()
        .token_id(token_id_u256)
        .size(shares)
        .side(ClobSide::Sell)
        .price(price)
        .order_type(ClobOrderType::GTC)
        .build()
        .await?;

    let signed_order = client.sign(&signer, order).await?;
    let response = client.post_order(signed_order).await?;

    info!("GTC sell order placed: id={}", response.order_id);
    Ok(response.order_id)
}

/// Place a FOK (Fill or Kill) BUY order — fills entirely and immediately, or is cancelled.
/// Used for mint maker pairs to guarantee both sides fill atomically.
/// Takes USD amount (not shares) — the CLOB determines how many shares you get.
pub async fn place_fok_buy(
    private_key: &str,
    token_id: &str,
    price: Decimal,
    usd_amount: Decimal,
) -> Result<String> {
    use polymarket_client_sdk::clob::types::Amount;

    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    let eoa_addr = signer.address();
    let safe_addr = crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_addr))
        .unwrap_or_else(|_| "unknown".to_string());
    info!("place_fok_buy: EOA={:?}, Safe={}, using GnosisSafe signature type", eoa_addr, safe_addr);

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let client = ClobClient::new(CLOB_ENDPOINT, clob_config)?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await?;

    let token_id_u256 = U256::from_str_radix(token_id, 10)?;

    // Round price to tick size
    let price = price.trunc_with_scale(2);

    info!("Placing FOK BUY: token={}, price={}, amount=${}", token_id, price, usd_amount);

    let order = client
        .market_order()
        .token_id(token_id_u256)
        .amount(Amount::usdc(usd_amount)?)
        .side(ClobSide::Buy)
        .price(price)
        .order_type(ClobOrderType::FOK)
        .build()
        .await?;

    let signed_order = client.sign(&signer, order).await?;
    let response = client.post_order(signed_order).await?;

    info!("FOK order placed: id={}", response.order_id);
    Ok(response.order_id)
}

/// Cancel an order via CLOB SDK.
/// Uses DELETE /order with body {"orderId": "..."} (NOT /order/{id}).
pub async fn cancel_order(
    _wallet_address: &str,
    order_id: &str,
    _api_key: &str,
    _api_secret: &str,
    _api_passphrase: &str,
) -> Result<()> {
    // We need a private key to authenticate with the SDK.
    // Fall back to raw HTTP cancel using the correct endpoint format.
    cancel_order_raw(order_id, _wallet_address, _api_key, _api_secret, _api_passphrase).await
}

/// Cancel an order using a private key (SDK-based, for stop loss and internal use).
pub async fn cancel_order_with_key(
    private_key: &str,
    order_id: &str,
) -> Result<()> {
    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let client = ClobClient::new(CLOB_ENDPOINT, clob_config)?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await?;

    let resp = client.cancel_order(order_id).await?;
    info!("Order {} cancelled via SDK: {:?}", &order_id[..16.min(order_id.len())], resp);
    Ok(())
}

/// Cancel an order via raw HTTP with correct endpoint: DELETE /order with body {"orderId": "..."}
async fn cancel_order_raw(
    order_id: &str,
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<()> {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let path = "/order";
    let method = "DELETE";
    let body = serde_json::json!({ "orderID": order_id }).to_string();
    let timestamp = Utc::now().timestamp_millis().to_string();

    // Signature includes body for DELETE with payload
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)?;
    mac.update(sig_payload.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("{}{}", CLOB_ENDPOINT, path);
    let response = client
        .delete(&url)
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .body(body)
        .send()
        .await?;

    if response.status().is_success() {
        info!("Order {} cancelled", &order_id[..16.min(order_id.len())]);
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        warn!("Failed to cancel order {}: {} - {}", &order_id[..16.min(order_id.len())], status, body);
    }

    Ok(())
}

/// Check order status via CLOB API with retry for transient errors
pub async fn check_order_status(
    wallet_address: &str,
    order_id: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<OrderCheckResult> {
    // Retry up to 2 times on transient failures
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
        }
        match check_order_status_once(wallet_address, order_id, api_key, api_secret, api_passphrase).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!("check_order_status attempt {} failed for {}: {}", attempt + 1, order_id, e);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("check_order_status failed")))
}

/// Single attempt to check order status via CLOB API
async fn check_order_status_once(
    wallet_address: &str,
    order_id: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<OrderCheckResult> {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let path = format!("/data/order/{}", order_id);
    let method = "GET";
    let timestamp = Utc::now().timestamp_millis().to_string();
    let sig_payload = format!("{}{}{}", timestamp, method, path);

    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)?;
    mac.update(sig_payload.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("{}{}", CLOB_ENDPOINT, path);
    let response = client
        .get(&url)
        .header("POLY_ADDRESS", wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .send()
        .await?;

    let status_code = response.status();

    if status_code.is_success() {
        let data: serde_json::Value = response.json().await?;
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        let size_matched = data.get("size_matched").and_then(|v| v.as_str()).unwrap_or("0");
        let original_size = data.get("original_size").and_then(|v| v.as_str()).unwrap_or("0");
        let price = data.get("price").and_then(|v| v.as_str()).unwrap_or("0");

        let matched: f64 = size_matched.parse().unwrap_or(0.0);
        let original: f64 = original_size.parse().unwrap_or(0.0);

        info!(
            "CLOB order status: id={} status={} matched={}/{} price={}",
            &order_id[..16.min(order_id.len())], status, size_matched, original_size, price
        );

        // Check terminal statuses FIRST — INVALID/CANCELED are final even with partial fills.
        // Note: CLOB uses American spelling "CANCELED" (one L), not "CANCELLED" (two L's).
        let is_terminal = status == "CANCELLED" || status == "CANCELED" || status == "INVALID" || status == "CANCELED_MARKET_RESOLVED";
        let fill_status = if is_terminal {
            if matched > 0.0 {
                // Partial fill on a terminal order — some shares matched but the rest never will.
                warn!("CLOB order {} status={} with partial fill {}/{} — treating as Cancelled",
                    &order_id[..16.min(order_id.len())], status, size_matched, original_size);
            } else if status != "CANCELLED" && status != "CANCELED" {
                info!("CLOB order {} terminal status={} — treating as Cancelled", &order_id[..16.min(order_id.len())], status);
            }
            FillStatus::Cancelled
        } else if status == "UNKNOWN" && original == 0.0 && matched == 0.0 {
            // CLOB returned no data for this order — it was never created or already purged.
            warn!("CLOB order {} status=UNKNOWN with no size/price — treating as Cancelled", &order_id[..16.min(order_id.len())]);
            FillStatus::Cancelled
        } else if matched > 0.0 && (status == "MATCHED" || matched >= original * 0.99) {
            FillStatus::Filled
        } else if status == "MATCHED" && matched == 0.0 {
            // CLOB says MATCHED but no size — likely API lag or phantom match.
            // Treat as still open so we re-check next cycle.
            warn!("CLOB order {} status=MATCHED but size_matched=0 — treating as Open", &order_id[..16.min(order_id.len())]);
            FillStatus::Open
        } else if matched > 0.0 {
            FillStatus::PartiallyFilled
        } else {
            FillStatus::Open
        };

        Ok(OrderCheckResult {
            order_id: order_id.to_string(),
            fill_status,
            fill_price: if matched > 0.0 { Some(price.to_string()) } else { None },
            size_matched: size_matched.to_string(),
        })
    } else if status_code == reqwest::StatusCode::NOT_FOUND {
        // 404 — order genuinely doesn't exist, treat as cancelled
        warn!("CLOB order 404: id={} — treating as cancelled", &order_id[..16.min(order_id.len())]);
        Ok(OrderCheckResult {
            order_id: order_id.to_string(),
            fill_status: FillStatus::Cancelled,
            fill_price: None,
            size_matched: "0".to_string(),
        })
    } else {
        // Rate limit, server error, etc. — return error so caller can retry
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("CLOB API error {} for order {}: {}", status_code, order_id, body)
    }
}

#[derive(Debug, Clone)]
pub struct OrderCheckResult {
    pub order_id: String,
    pub fill_status: FillStatus,
    pub fill_price: Option<String>,
    pub size_matched: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    /// API call failed — don't assume anything about order state
    Unknown,
}

/// Refresh the CLOB server's cached view of on-chain balance & allowances for a wallet.
///
/// The CLOB caches each wallet's USDC allowance and balance. After Safe activation
/// sets on-chain approvals, the CLOB won't see them until told to refresh. Without
/// this call, every order is rejected with "not enough balance / allowance".
///
/// This must be called with GnosisSafe signature type so the CLOB checks the Safe
/// proxy address (where USDC lives), not the bare EOA.
pub async fn refresh_clob_allowance_cache(private_key: &str) -> Result<()> {
    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    let eoa_addr = signer.address();
    let safe_addr = crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_addr))
        .unwrap_or_else(|_| "unknown".to_string());

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let client = ClobClient::new(CLOB_ENDPOINT, clob_config)?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await?;

    // Refresh COLLATERAL (USDC) allowance cache
    let collateral_req = UpdateBalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    match client.update_balance_allowance(collateral_req).await {
        Ok(()) => info!("CLOB cache: COLLATERAL refreshed for Safe {}", safe_addr),
        Err(e) => warn!("CLOB cache: Failed to refresh COLLATERAL for {}: {}", safe_addr, e),
    }

    // Refresh CONDITIONAL (ERC-1155 token) allowance cache
    let conditional_req = UpdateBalanceAllowanceRequest::builder()
        .asset_type(AssetType::Conditional)
        .build();
    match client.update_balance_allowance(conditional_req).await {
        Ok(()) => info!("CLOB cache: CONDITIONAL refreshed for Safe {}", safe_addr),
        Err(e) => warn!("CLOB cache: Failed to refresh CONDITIONAL for {}: {}", safe_addr, e),
    }

    Ok(())
}

/// Derive and store CLOB API credentials for a wallet if they don't exist yet.
///
/// The `enable` endpoint normally does this, but if the runner starts auto-placing
/// before `enable` was called (or after a restart where key_store still has the key
/// but API creds were lost), this ensures creds are available.
pub async fn ensure_clob_api_credentials(
    private_key: &str,
    db: &crate::db::Database,
    wallet_address: &str,
) -> Result<(String, String, String)> {
    use polymarket_client_sdk::auth::ExposeSecret;

    // Check if we already have credentials stored
    if let Ok(Some(creds)) = db.get_api_credentials(wallet_address).await {
        return Ok(creds);
    }

    info!("MintMaker: Deriving CLOB API credentials for {}", wallet_address);

    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let unauth_client = ClobClient::new(CLOB_ENDPOINT, clob_config)?;
    let creds = unauth_client.create_or_derive_api_key(&signer, None).await?;

    let api_key = creds.key().to_string();
    let api_secret = creds.secret().expose_secret().to_string();
    let api_passphrase = creds.passphrase().expose_secret().to_string();

    // Store for future use
    db.store_api_credentials(wallet_address, &api_key, &api_secret, &api_passphrase).await?;
    info!("MintMaker: Stored CLOB API credentials for {}", wallet_address);

    Ok((api_key, api_secret, api_passphrase))
}

const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

/// Fetch on-chain USDC balance for a Safe wallet address via Polygon RPC
pub async fn fetch_safe_usdc_balance(client: &reqwest::Client, safe_address: &str) -> Result<Decimal> {
    let rpc_urls = [
        "https://polygon-rpc.com",
        "https://rpc-mainnet.matic.quiknode.pro",
        "https://polygon.llamarpc.com",
    ];

    // balanceOf(address) function selector: 0x70a08231
    let padded_address = format!(
        "000000000000000000000000{}",
        safe_address.trim_start_matches("0x")
    );
    let data = format!("0x70a08231{}", padded_address);

    let rpc_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{
            "to": USDC_ADDRESS,
            "data": data
        }, "latest"]
    });

    for rpc_url in &rpc_urls {
        match client.post(*rpc_url).json(&rpc_body).send().await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(result) = json.get("result").and_then(|v| v.as_str()) {
                        let hex = result.trim_start_matches("0x");
                        let balance_raw = u128::from_str_radix(hex, 16).unwrap_or(0);
                        // USDC has 6 decimals
                        let whole = balance_raw / 1_000_000;
                        let fraction = balance_raw % 1_000_000;
                        let balance_str = format!("{}.{:06}", whole, fraction);
                        return Decimal::from_str(&balance_str).map_err(|e| anyhow::anyhow!("Failed to parse balance: {}", e));
                    }
                }
            }
            Err(_) => continue,
        }
    }

    anyhow::bail!("Failed to fetch USDC balance from all RPC endpoints")
}

/// Derive the Safe proxy address for an EOA address
pub fn derive_safe_address(private_key: &str) -> Result<String> {
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::Signer;
    let signer: PrivateKeySigner = private_key.parse()?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));
    let eoa_addr = signer.address();
    crate::services::safe_proxy::derive_safe_wallet(&format!("{:?}", eoa_addr))
        .map_err(|e| anyhow::anyhow!("Failed to derive Safe address: {}", e))
}
