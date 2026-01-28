//! UMA Dispute Tracker Service
//!
//! Monitors UMA Optimistic Oracle for active Polymarket disputes.
//! Queries the Goldsky subgraph for dispute events and tracks their status.

use crate::types::{DisputeAlert, DisputeStatus};
use crate::Database;
use anyhow::Result;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// UMA Optimistic Oracle subgraph endpoint (Polygon)
const UMA_SUBGRAPH_URL: &str = "https://api.goldsky.com/api/public/project_clus2fndawbcc01w31192938i/subgraphs/polygon-optimistic-oracle-v3/1/gn";

/// Polymarket adapter address on Polygon
const POLYMARKET_ADAPTER: &str = "0x6A9D222616C90FcA5754cd1333cFD9b7fb6a4F74";

/// GraphQL query for active assertions/disputes
#[derive(Serialize)]
struct GraphQLRequest {
    query: String,
    variables: Option<serde_json::Value>,
}

/// Response from the subgraph
#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLData {
    assertions: Option<Vec<UmaAssertion>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

/// UMA assertion from subgraph
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UmaAssertion {
    id: String,
    assertion_id: Option<String>,
    claim: Option<String>,
    /// Timestamp when assertion was made
    assertion_timestamp: Option<String>,
    /// Expiration timestamp (when challenge window ends)
    expiration_time: Option<String>,
    /// Disputer address (if disputed, this will be non-null)
    disputer: Option<String>,
    /// Settlement resolution (true/false if settled, null if pending)
    settlement_resolution: Option<bool>,
    /// Dispute timestamp (if disputed)
    dispute_timestamp: Option<String>,
    /// Settlement timestamp (if settled)
    settlement_timestamp: Option<String>,
    /// Market condition ID (in the claim data)
    identifier: Option<String>,
}

/// Dispute tracker service
pub struct DisputeTracker {
    db: Arc<Database>,
    client: Client,
    /// In-memory cache of assertion_id -> (status, alert)
    tracked_disputes: HashMap<String, (DisputeStatus, DisputeAlert)>,
}

impl DisputeTracker {
    pub fn new(db: Arc<Database>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            db,
            client,
            tracked_disputes: HashMap::new(),
        }
    }

    /// Run the dispute tracker loop
    pub async fn run(
        mut self,
        interval: Duration,
        tx: broadcast::Sender<Vec<DisputeAlert>>,
    ) {
        loop {
            match self.check_disputes().await {
                Ok(alerts) => {
                    if !alerts.is_empty() {
                        info!("Found {} active UMA disputes", alerts.len());
                    }
                    // Always send current state (even if empty) so UI stays updated
                    let _ = tx.send(alerts);
                }
                Err(e) => {
                    error!("Dispute tracker scan failed: {}", e);
                }
            }

            tokio::time::sleep(interval).await;
        }
    }

    /// Check for active disputes
    async fn check_disputes(&mut self) -> Result<Vec<DisputeAlert>> {
        let assertions = self.fetch_assertions().await?;
        let mut alerts = Vec::new();

        // Track which assertions we've seen this scan
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for assertion in assertions {
            let assertion_id = assertion.assertion_id
                .as_ref()
                .or(Some(&assertion.id))
                .cloned()
                .unwrap_or_default();

            if assertion_id.is_empty() {
                continue;
            }

            seen_ids.insert(assertion_id.clone());

            // Determine dispute status based on available fields
            // settlementResolution being Some (true or false) means it's settled
            let is_settled = assertion.settlement_resolution.is_some()
                || assertion.settlement_timestamp.is_some();
            // disputer being present means it was disputed
            let is_disputed = assertion.disputer.is_some()
                && assertion.disputer.as_ref().map(|d| !d.is_empty() && d != "null").unwrap_or(false);

            // Skip settled assertions
            if is_settled {
                self.tracked_disputes.remove(&assertion_id);
                continue;
            }

            // Determine status
            let status = if is_disputed {
                // Has a disputer - could be disputed or escalated to DVM
                // Check if dispute_timestamp exists to confirm active dispute
                if assertion.dispute_timestamp.is_some() {
                    DisputeStatus::Disputed
                } else {
                    DisputeStatus::Proposed
                }
            } else {
                DisputeStatus::Proposed
            };

            // Parse timestamps - use dispute_timestamp if available, otherwise assertion_timestamp
            let dispute_timestamp = assertion.dispute_timestamp
                .as_ref()
                .or(assertion.assertion_timestamp.as_ref())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            let estimated_resolution = assertion.expiration_time
                .as_ref()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            // Parse the claim to extract market info
            let (question, condition_id, proposed_outcome) = self.parse_claim(&assertion);

            // For now, we don't have real-time prices from the subgraph
            // The frontend can fetch these separately if needed
            let alert = DisputeAlert {
                market_id: assertion_id.clone(),
                condition_id: condition_id.clone(),
                question: question.clone(),
                slug: String::new(), // Would need to look up from market data
                dispute_status: status,
                proposed_outcome,
                dispute_timestamp,
                estimated_resolution,
                current_yes_price: Decimal::ZERO,
                current_no_price: Decimal::ZERO,
                liquidity: Decimal::ZERO,
            };

            // Check if status changed
            let status_changed = self.tracked_disputes
                .get(&assertion_id)
                .map(|(old_status, _)| *old_status != status)
                .unwrap_or(true);

            if status_changed {
                info!(
                    "Dispute {} status: {} for market: {}",
                    assertion_id, status, question
                );
            }

            self.tracked_disputes.insert(assertion_id.clone(), (status, alert.clone()));
            alerts.push(alert);
        }

        // Remove disputes that are no longer in the subgraph
        self.tracked_disputes.retain(|id, _| seen_ids.contains(id));

        debug!("Tracking {} active disputes", alerts.len());
        Ok(alerts)
    }

    /// Fetch active assertions from UMA subgraph
    async fn fetch_assertions(&self) -> Result<Vec<UmaAssertion>> {
        // Query for recent assertions (filter unsettled ones in code)
        let query = r#"
            {
                assertions(
                    first: 100
                    orderBy: assertionTimestamp
                    orderDirection: desc
                ) {
                    id
                    assertionId
                    claim
                    assertionTimestamp
                    expirationTime
                    disputer
                    settlementResolution
                    disputeTimestamp
                    settlementTimestamp
                    identifier
                }
            }
            "#.to_string();

        let request = GraphQLRequest {
            query,
            variables: None,
        };

        let response = self.client
            .post(UMA_SUBGRAPH_URL)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Subgraph error: {}", response.status());
        }

        let result: GraphQLResponse = response.json().await?;

        if let Some(errors) = result.errors {
            if !errors.is_empty() {
                warn!("GraphQL errors: {:?}", errors);
            }
        }

        Ok(result.data
            .and_then(|d| d.assertions)
            .unwrap_or_default())
    }

    /// Parse claim data to extract market info
    fn parse_claim(&self, assertion: &UmaAssertion) -> (String, String, String) {
        // The claim typically contains encoded data about the market
        // Try to decode or extract readable text
        let question = assertion.claim
            .as_ref()
            .map(|c| {
                // Try to extract readable text from claim
                // Claims are often hex-encoded or have a specific format
                if c.starts_with("0x") && c.len() > 2 {
                    // Try to decode hex as UTF-8
                    hex::decode(&c[2..])
                        .ok()
                        .and_then(|bytes| {
                            // Find printable ASCII portion
                            let printable: String = bytes.iter()
                                .filter(|&&b| b >= 32 && b < 127)
                                .map(|&b| b as char)
                                .collect();
                            if printable.len() > 10 {
                                Some(printable)
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| {
                            // Truncate hex for display
                            let display_len = 40.min(c.len());
                            format!("{}...", &c[..display_len])
                        })
                } else if !c.is_empty() {
                    c.clone()
                } else {
                    "Unknown market".to_string()
                }
            })
            .unwrap_or_else(|| "Unknown market".to_string());

        let condition_id = assertion.identifier
            .clone()
            .unwrap_or_default();

        // Proposed outcome is typically encoded - would need market-specific decoding
        let proposed_outcome = "Unknown".to_string();

        (question, condition_id, proposed_outcome)
    }
}
