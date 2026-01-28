//! Discord webhook notifications for sniper opportunities

use crate::Opportunity;
use reqwest::Client;
use serde_json::json;
use tracing::{error, info};

/// Discord webhook client for sending sniper opportunity alerts
#[derive(Clone)]
pub struct DiscordWebhook {
    client: Client,
    webhook_url: String,
}

impl DiscordWebhook {
    /// Create a new Discord webhook client
    pub fn new(webhook_url: String) -> Self {
        Self {
            client: Client::new(),
            webhook_url,
        }
    }

    /// Send a sniper opportunity alert to Discord
    pub async fn send_sniper_alert(&self, opportunity: &Opportunity) {
        let no_bias_bonus = if matches!(opportunity.side, crate::Side::No) {
            " **[NO BIAS+]**"
        } else {
            ""
        };

        let embed = json!({
            "embeds": [{
                "title": format!("🎯 Sniper Opportunity{}", no_bias_bonus),
                "description": opportunity.short_question(200),
                "color": 0x00FF00,  // Green
                "fields": [
                    {
                        "name": "Side",
                        "value": format!("**{}** at **{}¢**", opportunity.side, opportunity.price_cents()),
                        "inline": true
                    },
                    {
                        "name": "Expected Value",
                        "value": format!("**{:.1}%**", opportunity.ev_percent()),
                        "inline": true
                    },
                    {
                        "name": "Return",
                        "value": format!("{:.1}%", opportunity.return_percent()),
                        "inline": true
                    },
                    {
                        "name": "Time to Close",
                        "value": opportunity.time_display(),
                        "inline": true
                    },
                    {
                        "name": "Liquidity",
                        "value": format!("${:.0}K", opportunity.liquidity.to_string().parse::<f64>().unwrap_or(0.0) / 1000.0),
                        "inline": true
                    },
                    {
                        "name": "Confidence",
                        "value": format!("{:.1}%", opportunity.confidence * 100.0),
                        "inline": true
                    }
                ],
                "url": opportunity.url(),
                "footer": {
                    "text": "Polymarket Sniper Bot"
                },
                "timestamp": chrono::Utc::now().to_rfc3339()
            }]
        });

        match self.client.post(&self.webhook_url)
            .json(&embed)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Discord alert sent for: {}", opportunity.short_question(50));
                } else {
                    error!("Discord webhook failed: {}", response.status());
                }
            }
            Err(e) => {
                error!("Failed to send Discord webhook: {}", e);
            }
        }
    }

    /// Send multiple sniper alerts (with rate limiting)
    pub async fn send_sniper_alerts(&self, opportunities: &[Opportunity]) {
        for opp in opportunities {
            self.send_sniper_alert(opp).await;
            // Small delay to avoid rate limiting
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}
