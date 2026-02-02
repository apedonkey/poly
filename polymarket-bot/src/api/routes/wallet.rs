//! Wallet API endpoints

use crate::api::server::AppState;
use crate::services::safe_activation::{self, BuilderCredentials};
use crate::wallet::{
    decrypt_private_key, encrypt_private_key,
    generate_wallet as create_wallet_keypair,
    wallet_from_private_key,
};
use alloy::primitives::keccak256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use serde::{Deserialize, Serialize};
use tracing::info;

// USDC.e (bridged) contract address on Polygon - used by Polymarket
const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

/// Generate wallet request
#[derive(Debug, Deserialize)]
pub struct GenerateWalletRequest {
    /// Optional password for server-side encrypted storage
    pub password: Option<String>,
}

/// Generate wallet response
#[derive(Debug, Serialize)]
pub struct GenerateWalletResponse {
    pub address: String,
    /// Private key - SHOWN ONCE ONLY
    pub private_key: String,
    pub session_token: String,
}

/// Import wallet request
#[derive(Debug, Deserialize)]
pub struct ImportWalletRequest {
    pub private_key: String,
    /// Optional password for server-side encrypted storage
    pub password: Option<String>,
}

/// Import wallet response
#[derive(Debug, Serialize)]
pub struct ImportWalletResponse {
    pub address: String,
    pub session_token: String,
}

/// Unlock wallet request (for existing encrypted wallets)
#[derive(Debug, Deserialize)]
pub struct UnlockWalletRequest {
    pub address: String,
    pub password: String,
}

/// Unlock wallet response
#[derive(Debug, Serialize)]
pub struct UnlockWalletResponse {
    pub session_token: String,
}

/// Wallet balance response
#[derive(Debug, Serialize)]
pub struct WalletBalanceResponse {
    pub address: String,
    pub usdc_balance: String,
    pub matic_balance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_usdc_balance: Option<String>,
}

/// API error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Generate a new wallet
pub async fn generate_wallet(
    State(state): State<AppState>,
    Json(req): Json<GenerateWalletRequest>,
) -> Result<Json<GenerateWalletResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Generate new wallet keypair
    let wallet = create_wallet_keypair();

    // Optionally encrypt and store
    let encrypted_key = match &req.password {
        Some(password) if !password.is_empty() => {
            Some(encrypt_private_key(&wallet.private_key, password).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Encryption failed: {}", e),
                    }),
                )
            })?)
        }
        _ => None,
    };

    // Store wallet in database
    state
        .db
        .create_wallet(&wallet.address, encrypted_key.as_ref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    // Create session
    let session = state.db.create_session(&wallet.address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Session error: {}", e),
            }),
        )
    })?;

    Ok(Json(GenerateWalletResponse {
        address: wallet.address,
        private_key: wallet.private_key, // SHOWN ONCE ONLY
        session_token: session.id,
    }))
}

/// Import an existing wallet
pub async fn import_wallet(
    State(state): State<AppState>,
    Json(req): Json<ImportWalletRequest>,
) -> Result<Json<ImportWalletResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate private key and get address
    let address = wallet_from_private_key(&req.private_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid private key: {}", e),
            }),
        )
    })?;

    // Check if wallet already exists
    let existing = state.db.get_wallet(&address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    if existing.is_none() {
        // Optionally encrypt and store
        let encrypted_key = match &req.password {
            Some(password) if !password.is_empty() => {
                Some(encrypt_private_key(&req.private_key, password).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("Encryption failed: {}", e),
                        }),
                    )
                })?)
            }
            _ => None,
        };

        // Store wallet
        state
            .db
            .create_wallet(&address, encrypted_key.as_ref())
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;
    }

    // Create session
    let session = state.db.create_session(&address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Session error: {}", e),
            }),
        )
    })?;

    Ok(Json(ImportWalletResponse {
        address,
        session_token: session.id,
    }))
}

/// Unlock an encrypted wallet
pub async fn unlock_wallet(
    State(state): State<AppState>,
    Json(req): Json<UnlockWalletRequest>,
) -> Result<Json<UnlockWalletResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get encrypted key
    let encrypted_key = state
        .db
        .get_encrypted_key(&req.address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Wallet not found or not encrypted".to_string(),
                }),
            )
        })?;

    // Try to decrypt (validates password)
    decrypt_private_key(&encrypted_key, &req.password).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid password".to_string(),
            }),
        )
    })?;

    // Create session
    let session = state.db.create_session(&req.address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Session error: {}", e),
            }),
        )
    })?;

    // Update activity
    let _ = state.db.update_wallet_activity(&req.address).await;

    Ok(Json(UnlockWalletResponse {
        session_token: session.id,
    }))
}

/// Query params for balance endpoint
#[derive(Debug, Deserialize)]
pub struct BalanceQuery {
    /// Optional address override (for external wallets like MetaMask)
    pub address: Option<String>,
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: &'static str,
    params: serde_json::Value,
    id: u32,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
}

/// Get wallet balance from Polygon
pub async fn get_balance(
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
    auth: Option<TypedHeader<Authorization<Bearer>>>,
) -> Result<Json<WalletBalanceResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get address from query param or session
    let address = if let Some(addr) = query.address {
        addr.to_lowercase()
    } else if let Some(TypedHeader(auth_header)) = auth {
        let session = state
            .db
            .get_session(auth_header.token())
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "Invalid or expired session".to_string(),
                    }),
                )
            })?;
        session.wallet_address
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Address or session token required".to_string(),
            }),
        ));
    };

    // Validate address format
    if !address.starts_with("0x") || address.len() != 42 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid address format".to_string(),
            }),
        ));
    }

    let client = reqwest::Client::new();
    let rpc_url = &state.config.polygon_rpc_url;

    // Fetch MATIC balance using eth_getBalance
    let matic_balance = fetch_native_balance(&client, rpc_url, &address)
        .await
        .unwrap_or_else(|_| "0.00".to_string());

    // Fetch USDC balance using eth_call (balanceOf)
    let usdc_balance = fetch_erc20_balance(&client, rpc_url, USDC_ADDRESS, &address, 6)
        .await
        .unwrap_or_else(|_| "0.00".to_string());

    // For generated wallets (those with an encrypted key), also return Safe balance
    let (safe_address, safe_usdc_balance) = match state.db.get_encrypted_key(&address).await {
        Ok(Some(_)) => {
            // Generated wallet — derive Safe address and fetch its balance
            match crate::services::safe_proxy::derive_safe_wallet(&address) {
                Ok(safe_addr) => {
                    let safe_bal = fetch_erc20_balance(&client, rpc_url, USDC_ADDRESS, &safe_addr, 6)
                        .await
                        .unwrap_or_else(|_| "0.00".to_string());
                    (Some(safe_addr), Some(safe_bal))
                }
                Err(_) => (None, None),
            }
        }
        _ => (None, None),
    };

    Ok(Json(WalletBalanceResponse {
        address,
        usdc_balance,
        matic_balance,
        safe_address,
        safe_usdc_balance,
    }))
}

/// Fetch native token balance (MATIC/POL)
async fn fetch_native_balance(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<String, anyhow::Error> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        method: "eth_getBalance",
        params: serde_json::json!([address, "latest"]),
        id: 1,
    };

    let response: JsonRpcResponse = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    if let Some(hex_balance) = response.result {
        let balance = u128::from_str_radix(hex_balance.trim_start_matches("0x"), 16).unwrap_or(0);
        Ok(format_balance_u128(balance, 18))
    } else {
        Ok("0.00".to_string())
    }
}

/// Fetch ERC20 token balance
async fn fetch_erc20_balance(
    client: &reqwest::Client,
    rpc_url: &str,
    contract: &str,
    address: &str,
    decimals: u8,
) -> Result<String, anyhow::Error> {
    // balanceOf(address) function selector: 0x70a08231
    // Pad address to 32 bytes
    let padded_address = format!("000000000000000000000000{}", address.trim_start_matches("0x"));
    let data = format!("0x70a08231{}", padded_address);

    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        method: "eth_call",
        params: serde_json::json!([
            {
                "to": contract,
                "data": data
            },
            "latest"
        ]),
        id: 1,
    };

    let response: JsonRpcResponse = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    if let Some(hex_balance) = response.result {
        let balance = u128::from_str_radix(hex_balance.trim_start_matches("0x"), 16).unwrap_or(0);
        Ok(format_balance_u128(balance, decimals))
    } else {
        Ok("0.00".to_string())
    }
}

/// Format a balance with given decimals
fn format_balance_u128(balance: u128, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = balance / divisor;
    let fraction = balance % divisor;

    // Format with 2 decimal places
    let fraction_scaled = (fraction * 100) / divisor;
    format!("{}.{:02}", whole, fraction_scaled)
}

/// Connect external wallet (MetaMask, etc.) - just creates a session
#[derive(Debug, Deserialize)]
pub struct ConnectExternalWalletRequest {
    pub address: String,
}

#[derive(Debug, Serialize)]
pub struct ConnectExternalWalletResponse {
    pub address: String,
    pub session_token: String,
}

pub async fn connect_external_wallet(
    State(state): State<AppState>,
    Json(req): Json<ConnectExternalWalletRequest>,
) -> Result<Json<ConnectExternalWalletResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate address format
    let address = req.address.to_lowercase();
    if !address.starts_with("0x") || address.len() != 42 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid Ethereum address".to_string(),
            }),
        ));
    }

    // Check if wallet exists, create if not
    let existing = state.db.get_wallet(&address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    if existing.is_none() {
        state
            .db
            .create_wallet(&address, None) // No encrypted key for external wallets
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;
    }

    // Create session
    let session = state.db.create_session(&address).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Session error: {}", e),
            }),
        )
    })?;

    Ok(Json(ConnectExternalWalletResponse {
        address,
        session_token: session.id,
    }))
}

/// Export private key request
#[derive(Debug, Deserialize)]
pub struct ExportPrivateKeyRequest {
    pub password: String,
}

/// Export private key response
#[derive(Debug, Serialize)]
pub struct ExportPrivateKeyResponse {
    pub private_key: String,
}

/// Export private key (requires password verification)
pub async fn export_private_key(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<ExportPrivateKeyRequest>,
) -> Result<Json<ExportPrivateKeyResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    // Get encrypted key
    let encrypted_key = state
        .db
        .get_encrypted_key(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No private key stored for this wallet. This may be an external wallet.".to_string(),
                }),
            )
        })?;

    // Decrypt with password
    let private_key = decrypt_private_key(&encrypted_key, &req.password).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid password".to_string(),
            }),
        )
    })?;

    Ok(Json(ExportPrivateKeyResponse { private_key }))
}

/// Disconnect wallet - stops the User WebSocket and clears session resources
pub async fn disconnect_wallet(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    let wallet_address = &session.wallet_address;
    tracing::info!("Disconnecting wallet {}", wallet_address);

    // Stop the User WebSocket for this wallet (frees resources)
    state.stop_user_ws(wallet_address).await;

    // Clear the key from in-memory store
    state.key_store.remove_key(wallet_address).await;

    Ok(Json(serde_json::json!({ "status": "disconnected", "wallet": wallet_address })))
}

// ── Deposit / Withdraw endpoints ────────────────────────────────────────

const POLYGON_CHAIN_ID: u64 = 137;

/// Deposit request (EOA → Safe)
#[derive(Debug, Deserialize)]
pub struct DepositRequest {
    pub password: String,
    pub amount: String,
}

/// Deposit response
#[derive(Debug, Serialize)]
pub struct DepositResponse {
    pub tx_hash: String,
    pub safe_address: String,
    pub amount: String,
}

/// Withdraw request (Safe → EOA)
#[derive(Debug, Deserialize)]
pub struct WithdrawRequest {
    pub password: String,
    pub amount: String,
}

/// Withdraw response
#[derive(Debug, Serialize)]
pub struct WithdrawResponse {
    pub transaction_id: String,
    pub safe_address: String,
    pub amount: String,
}

/// Helper: validate session + decrypt private key → (wallet_address, PrivateKeySigner)
async fn decrypt_signer(
    state: &AppState,
    token: &str,
    password: &str,
) -> Result<(String, PrivateKeySigner), (StatusCode, Json<ErrorResponse>)> {
    let session = state
        .db
        .get_session(token)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: format!("Database error: {}", e) }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse { error: "Invalid or expired session".to_string() }),
            )
        })?;

    let encrypted_key = state
        .db
        .get_encrypted_key(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: format!("Database error: {}", e) }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No private key stored — this may be an external wallet".to_string(),
                }),
            )
        })?;

    let private_key = decrypt_private_key(&encrypted_key, password).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse { error: "Invalid password".to_string() }),
        )
    })?;

    let signer: PrivateKeySigner = private_key
        .parse()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: format!("Signer error: {}", e) }),
            )
        })?;

    Ok((session.wallet_address, signer))
}

/// Helper: build BuilderCredentials from config
fn get_builder_creds(state: &AppState) -> Option<BuilderCredentials> {
    match (
        state.config.builder_api_key.as_ref(),
        state.config.builder_secret.as_ref(),
        state.config.builder_passphrase.as_ref(),
    ) {
        (Some(key), Some(secret), Some(pass)) => Some(BuilderCredentials {
            api_key: key.clone(),
            secret: secret.clone(),
            passphrase: pass.clone(),
        }),
        _ => None,
    }
}

/// Build ERC-20 transfer calldata: transfer(address to, uint256 amount)
fn build_transfer_data(to: &[u8; 20], amount_raw: u128) -> Vec<u8> {
    let selector = &keccak256(b"transfer(address,uint256)")[..4];
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(selector);
    let mut to_padded = [0u8; 32];
    to_padded[12..].copy_from_slice(to);
    data.extend_from_slice(&to_padded);
    let mut amount_bytes = [0u8; 32];
    amount_bytes[16..].copy_from_slice(&amount_raw.to_be_bytes());
    data.extend_from_slice(&amount_bytes);
    data
}

// ── RLP encoding helpers (legacy EIP-155 transactions) ─────────────────

fn rlp_encode_uint(val: u64) -> Vec<u8> {
    if val == 0 {
        return vec![0x80]; // empty string
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(8);
    let trimmed = &bytes[start..];
    if trimmed.len() == 1 && trimmed[0] < 0x80 {
        trimmed.to_vec()
    } else {
        let mut out = vec![0x80 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0x80];
    }
    if data.len() == 1 && data[0] < 0x80 {
        return data.to_vec();
    }
    if data.len() < 56 {
        let mut out = vec![0x80 + data.len() as u8];
        out.extend_from_slice(data);
        out
    } else {
        let len_bytes = {
            let b = data.len().to_be_bytes();
            let start = b.iter().position(|&x| x != 0).unwrap_or(b.len());
            b[start..].to_vec()
        };
        let mut out = vec![0xb7 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(data);
        out
    }
}

fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = items.iter().flat_map(|i| i.iter().copied()).collect();
    if payload.len() < 56 {
        let mut out = vec![0xc0 + payload.len() as u8];
        out.extend_from_slice(&payload);
        out
    } else {
        let len_bytes = {
            let b = payload.len().to_be_bytes();
            let start = b.iter().position(|&x| x != 0).unwrap_or(b.len());
            b[start..].to_vec()
        };
        let mut out = vec![0xf7 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(&payload);
        out
    }
}

/// POST /api/wallet/deposit — Transfer USDC from EOA to Safe
pub async fn deposit_to_safe(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<DepositRequest>,
) -> Result<Json<DepositResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (wallet_address, signer) = decrypt_signer(&state, auth.token(), &req.password).await?;

    // Parse amount (USDC has 6 decimals)
    let amount_f: f64 = req.amount.parse().map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid amount".to_string() }))
    })?;
    if amount_f <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Amount must be positive".to_string() })));
    }
    let amount_raw = (amount_f * 1_000_000.0) as u128;

    // Derive Safe address
    let safe_address = crate::services::safe_proxy::derive_safe_wallet(&wallet_address)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Failed to derive Safe: {}", e) }))
        })?;

    // Auto-activate Safe if builder creds available
    if let Some(bcreds) = get_builder_creds(&state) {
        match safe_activation::ensure_safe_activated(&signer, &bcreds).await {
            Ok(addr) => info!("Deposit: Safe activated at {}", addr),
            Err(e) => tracing::warn!("Deposit: Safe activation warning: {}", e),
        }
    }

    // Build USDC transfer(safe_address, amount) calldata
    let safe_addr_bytes: [u8; 20] = hex::decode(safe_address.trim_start_matches("0x"))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Hex error: {}", e) })))?
        .try_into()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Address length error".to_string() })))?;
    let calldata = build_transfer_data(&safe_addr_bytes, amount_raw);

    // Fetch nonce and gas price via RPC
    let client = reqwest::Client::new();
    let rpc_url = &state.config.polygon_rpc_url;

    let nonce_resp: JsonRpcResponse = client
        .post(rpc_url)
        .json(&JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_getTransactionCount",
            params: serde_json::json!([&wallet_address, "latest"]),
            id: 1,
        })
        .send()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC error: {}", e) })))?
        .json()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC parse error: {}", e) })))?;
    let nonce = u64::from_str_radix(
        nonce_resp.result.as_deref().unwrap_or("0x0").trim_start_matches("0x"),
        16,
    )
    .unwrap_or(0);

    let gas_resp: JsonRpcResponse = client
        .post(rpc_url)
        .json(&JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_gasPrice",
            params: serde_json::json!([]),
            id: 2,
        })
        .send()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC error: {}", e) })))?
        .json()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC parse error: {}", e) })))?;
    let gas_price = u64::from_str_radix(
        gas_resp.result.as_deref().unwrap_or("0x0").trim_start_matches("0x"),
        16,
    )
    .unwrap_or(30_000_000_000); // 30 gwei fallback

    let gas_limit: u64 = 80_000; // ERC-20 transfer
    let usdc_contract: [u8; 20] = hex::decode(USDC_ADDRESS.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();

    // RLP-encode unsigned legacy tx for EIP-155 signing:
    // [nonce, gasPrice, gasLimit, to, value=0, data, chainId, 0, 0]
    let unsigned_items = vec![
        rlp_encode_uint(nonce),
        rlp_encode_uint(gas_price),
        rlp_encode_uint(gas_limit),
        rlp_encode_bytes(&usdc_contract),
        rlp_encode_uint(0), // value = 0 (we're calling transfer, not sending MATIC)
        rlp_encode_bytes(&calldata),
        rlp_encode_uint(POLYGON_CHAIN_ID),
        rlp_encode_uint(0),
        rlp_encode_uint(0),
    ];
    let unsigned_rlp = rlp_encode_list(&unsigned_items);
    let tx_hash_unsigned = keccak256(&unsigned_rlp);

    // Sign the hash
    let sig = signer
        .sign_hash(&tx_hash_unsigned)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Signing error: {}", e) })))?;
    let sig_bytes = sig.as_bytes(); // 65 bytes: r(32) + s(32) + v(1)

    // Recover v for EIP-155: v = recovery_id + chainId*2 + 35
    // sig.as_bytes() returns v as 27/28 (legacy) or 0/1 (recovery id)
    let recovery_id = match sig_bytes[64] {
        0 | 1 => sig_bytes[64] as u64,
        27 => 0u64,
        28 => 1u64,
        other => (other - 27) as u64,
    };
    let v_eip155 = recovery_id + POLYGON_CHAIN_ID * 2 + 35;

    // RLP-encode signed tx: [nonce, gasPrice, gasLimit, to, value, data, v, r, s]
    let signed_items = vec![
        rlp_encode_uint(nonce),
        rlp_encode_uint(gas_price),
        rlp_encode_uint(gas_limit),
        rlp_encode_bytes(&usdc_contract),
        rlp_encode_uint(0),
        rlp_encode_bytes(&calldata),
        rlp_encode_uint(v_eip155),
        rlp_encode_bytes(&sig_bytes[..32]),  // r
        rlp_encode_bytes(&sig_bytes[32..64]), // s
    ];
    let signed_rlp = rlp_encode_list(&signed_items);
    let raw_tx_hex = format!("0x{}", hex::encode(&signed_rlp));

    // Send via eth_sendRawTransaction
    let send_resp: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": [raw_tx_hex],
            "id": 3
        }))
        .send()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC error: {}", e) })))?
        .json()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("RPC parse error: {}", e) })))?;

    if let Some(err) = send_resp.get("error") {
        let msg = err.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown RPC error");
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: format!("Transaction failed: {}", msg) })));
    }

    let tx_hash = send_resp
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    info!("Deposit tx sent: {} ({} USDC -> Safe {})", tx_hash, req.amount, safe_address);

    Ok(Json(DepositResponse {
        tx_hash,
        safe_address,
        amount: req.amount,
    }))
}

/// POST /api/wallet/withdraw — Transfer USDC from Safe to EOA via relay
pub async fn withdraw_from_safe(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<WithdrawRequest>,
) -> Result<Json<WithdrawResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (wallet_address, signer) = decrypt_signer(&state, auth.token(), &req.password).await?;

    // Parse amount
    let amount_f: f64 = req.amount.parse().map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid amount".to_string() }))
    })?;
    if amount_f <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Amount must be positive".to_string() })));
    }
    let amount_raw = (amount_f * 1_000_000.0) as u128;

    // Derive Safe address
    let safe_address = crate::services::safe_proxy::derive_safe_wallet(&wallet_address)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Failed to derive Safe: {}", e) }))
        })?;

    // Build USDC transfer(eoa, amount) calldata
    let eoa_bytes: [u8; 20] = hex::decode(wallet_address.trim_start_matches("0x"))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Hex error: {}", e) })))?
        .try_into()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Address length error".to_string() })))?;
    let calldata = build_transfer_data(&eoa_bytes, amount_raw);

    // Builder credentials required for relay
    let builder_creds = get_builder_creds(&state).ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "Builder credentials not configured — cannot withdraw via relay".to_string(),
        }))
    })?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Client error: {}", e) })))?;

    let usdc_addr: alloy::primitives::Address = USDC_ADDRESS.parse().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Address parse error: {}", e) }))
    })?;

    let tx_id = safe_activation::execute_safe_transaction(
        &signer,
        &client,
        &builder_creds,
        &safe_address,
        usdc_addr,
        &calldata,
    )
    .await
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Relay error: {}", e) }))
    })?;

    info!("Withdraw tx submitted: {} ({} USDC from Safe {} -> EOA {})", tx_id, req.amount, safe_address, wallet_address);

    Ok(Json(WithdrawResponse {
        transaction_id: tx_id,
        safe_address,
        amount: req.amount,
    }))
}
