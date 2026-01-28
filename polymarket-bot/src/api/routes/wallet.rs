//! Wallet API endpoints

use crate::api::server::AppState;
use crate::wallet::{
    decrypt_private_key, encrypt_private_key,
    generate_wallet as create_wallet_keypair,
    wallet_from_private_key,
};
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

    Ok(Json(WalletBalanceResponse {
        address,
        usdc_balance,
        matic_balance,
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
