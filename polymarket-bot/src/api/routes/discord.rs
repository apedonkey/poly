//! Discord webhook API routes

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::webhook::DiscordWebhook;

#[derive(Debug, Deserialize)]
pub struct SniperAlert {
    pub market_id: String,
    pub question: String,
    pub side: String,
    pub entry_price: String,
    pub edge: f64,
    pub expected_return: f64,
    pub confidence: f64,
    pub time_to_close_hours: Option<f64>,
    pub liquidity: String,
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendAlertsRequest {
    pub opportunities: Vec<SniperAlert>,
}

#[derive(Debug, Serialize)]
pub struct SendAlertsResponse {
    pub sent: usize,
}

/// POST /api/discord/alerts - Send sniper alerts to Discord
pub async fn send_alerts(
    State(state): State<AppState>,
    Json(request): Json<SendAlertsRequest>,
) -> Result<Json<SendAlertsResponse>, StatusCode> {
    let webhook_url = match &state.config.discord_webhook_url {
        Some(url) => url,
        None => {
            tracing::warn!("Discord webhook URL not configured");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    let _webhook = DiscordWebhook::new(webhook_url.clone());
    let mut sent = 0;

    for opp in &request.opportunities {
        // Build the embed for Discord
        let no_bias_bonus = if opp.side.to_lowercase() == "no" {
            " **[NO BIAS+]**"
        } else {
            ""
        };

        let price_cents = (opp.entry_price.parse::<f64>().unwrap_or(0.0) * 100.0) as i32;
        let ev_percent = opp.edge * 100.0;
        let return_percent = opp.expected_return * 100.0;
        let confidence_percent = opp.confidence * 100.0;

        let time_display = match opp.time_to_close_hours {
            Some(h) if h < 1.0 => format!("{:.0}m", h * 60.0),
            Some(h) if h < 24.0 => format!("{:.1}h", h),
            Some(h) if h < 168.0 => format!("{:.1}d", h / 24.0),
            Some(h) => format!("{:.1}w", h / 168.0),
            None => "?".to_string(),
        };

        let liquidity_k = opp.liquidity.parse::<f64>().unwrap_or(0.0) / 1000.0;

        let url = opp.slug.as_ref()
            .map(|s| format!("https://polymarket.com/event/{}", s))
            .unwrap_or_default();

        let short_question: String = opp.question.chars().take(200).collect();

        let embed = serde_json::json!({
            "embeds": [{
                "title": format!("🎯 Sniper Opportunity{}", no_bias_bonus),
                "description": short_question,
                "color": 0x00FF00,
                "fields": [
                    {
                        "name": "Side",
                        "value": format!("**{}** at **{}¢**", opp.side, price_cents),
                        "inline": true
                    },
                    {
                        "name": "Expected Value",
                        "value": format!("**{:.1}%**", ev_percent),
                        "inline": true
                    },
                    {
                        "name": "Return",
                        "value": format!("{:.1}%", return_percent),
                        "inline": true
                    },
                    {
                        "name": "Time to Close",
                        "value": time_display,
                        "inline": true
                    },
                    {
                        "name": "Liquidity",
                        "value": format!("${:.0}K", liquidity_k),
                        "inline": true
                    },
                    {
                        "name": "Confidence",
                        "value": format!("{:.1}%", confidence_percent),
                        "inline": true
                    }
                ],
                "url": url,
                "footer": {
                    "text": "Polymarket Sniper Bot"
                },
                "timestamp": chrono::Utc::now().to_rfc3339()
            }]
        });

        match reqwest::Client::new()
            .post(webhook_url)
            .json(&embed)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    tracing::info!("Discord alert sent for: {}", short_question.chars().take(50).collect::<String>());
                    sent += 1;
                } else {
                    tracing::error!("Discord webhook failed: {}", response.status());
                }
            }
            Err(e) => {
                tracing::error!("Failed to send Discord webhook: {}", e);
            }
        }

        // Small delay to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    Ok(Json(SendAlertsResponse { sent }))
}
