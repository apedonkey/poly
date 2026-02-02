//! UMA Dispute Tracker Service
//!
//! Monitors UMA Optimistic Oracle for active Polymarket disputes.
//! Queries the Goldsky subgraph for dispute events and tracks their status.
//! Filters by Polymarket's callback recipient address to only track relevant assertions.

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
use urlencoding;

/// UMA Optimistic Oracle subgraph endpoints (primary + fallback)
const UMA_SUBGRAPH_URL: &str = "https://api.goldsky.com/api/public/project_clus2fndawbcc01w31192938i/subgraphs/polygon-optimistic-oracle-v3/1/gn";
const UMA_SUBGRAPH_FALLBACK_URL: &str = "https://api.thegraph.com/subgraphs/name/umaprotocol/polygon-optimistic-oracle-v3";

/// Polymarket UMA CTF Adapter addresses on Polygon (all versions, lowercase)
/// v3.0 - newest adapter
const ADAPTER_V3: &str = "0x157ce2d672854c848c9b79c49a8cc6cc89176a49";
/// v2.0 - most common adapter
const ADAPTER_V2: &str = "0x6a9d222616c90fca5754cd1333cfd9b7fb6a4f74";
/// v1.0 - legacy adapter
const ADAPTER_V1: &str = "0xc8b122858a4ef82c2d4ee2e6a276c719e692995130";

/// All adapter addresses with their version labels
const ADAPTERS: &[(&str, &str)] = &[
    (ADAPTER_V3, "v3"),
    (ADAPTER_V2, "v2"),
    (ADAPTER_V1, "v1"),
];

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
#[allow(dead_code)]
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
    /// Domain ID - for Polymarket this is the questionId used to create the condition
    domain_id: Option<String>,
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
    /// UMA identifier (typically "ASSERT_TRUTH", NOT a market condition_id)
    #[allow(dead_code)]
    identifier: Option<String>,
    /// Bond amount in wei (from subgraph)
    bond: Option<String>,
    /// Bond currency address
    #[allow(dead_code)]
    currency: Option<String>,
    /// Callback recipient (adapter address) - used to determine adapter version
    callback_recipient: Option<String>,
}

/// Market data from Gamma API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaMarket {
    id: String,
    #[serde(default)]
    condition_id: String,
    question: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    outcome_prices: Option<String>,
    #[serde(default)]
    liquidity: Option<String>,
    #[serde(default)]
    clob_token_ids: Option<String>,
    #[serde(default)]
    events: Option<Vec<GammaEvent>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    #[serde(default)]
    slug: Option<String>,
}

/// Dispute tracker service
pub struct DisputeTracker {
    #[allow(dead_code)]
    db: Arc<Database>,
    client: Client,
    /// In-memory cache of assertion_id -> (status, alert)
    tracked_disputes: HashMap<String, (DisputeStatus, DisputeAlert)>,
    /// Cache of lookup_key -> market data
    market_cache: HashMap<String, GammaMarket>,
    /// Track domain_id (questionId) -> count of assertions seen (for two-round detection)
    /// If count > 1, the latest assertion is a re-proposal (round 2)
    domain_assertion_count: HashMap<String, u8>,
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
            market_cache: HashMap::new(),
            domain_assertion_count: HashMap::new(),
        }
    }

    /// Look up market data from Gamma API
    /// Tries: 1) condition_id lookup, 2) question text search, 3) keyword search
    async fn get_market_data(&mut self, condition_id: &str, question: &str) -> Option<&GammaMarket> {
        let cache_key = if !condition_id.is_empty() {
            condition_id.to_string()
        } else {
            question.to_string()
        };

        // Check cache first
        if self.market_cache.contains_key(&cache_key) {
            return self.market_cache.get(&cache_key);
        }

        info!("Looking up market data - condition_id: '{}', question: '{}'",
            condition_id, &question[..question.len().min(100)]);

        // Strategy 1: condition_id lookup (most reliable if we have it)
        if !condition_id.is_empty() {
            let url = format!(
                "https://gamma-api.polymarket.com/markets?condition_id={}",
                condition_id
            );

            debug!("Trying condition_id lookup: {}", url);

            match self.client.get(&url).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json::<Vec<GammaMarket>>().await {
                            Ok(markets) => {
                                info!("Condition_id lookup returned {} markets", markets.len());
                                if let Some(market) = markets.into_iter().next() {
                                    info!("Found market by condition_id: {} (liq: {:?}, prices: {:?})",
                                        market.question, market.liquidity, market.outcome_prices);
                                    self.market_cache.insert(cache_key.clone(), market);
                                    return self.market_cache.get(&cache_key);
                                }
                            }
                            Err(e) => warn!("Failed to parse condition_id response: {}", e),
                        }
                    } else {
                        warn!("Condition_id lookup returned status: {}", status);
                    }
                }
                Err(e) => warn!("Condition_id lookup request failed: {}", e),
            }
        }

        // Clean the question text for search
        let clean_question = question
            .replace('"', "")
            .replace('\'', "")
            .trim()
            .to_string();

        if clean_question.len() < 10 || clean_question.contains("...") {
            warn!("Question too short or truncated for search: '{}'", clean_question);
            return None;
        }

        // Strategy 2: Full question text search (with closed=false for active markets)
        {
            let encoded_search = urlencoding::encode(&clean_question);
            let url = format!(
                "https://gamma-api.polymarket.com/markets?closed=false&limit=10&search={}",
                encoded_search
            );

            info!("Searching Gamma API with question: {}", &clean_question[..clean_question.len().min(80)]);

            match self.client.get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<Vec<GammaMarket>>().await {
                            Ok(markets) => {
                                info!("Question search returned {} markets", markets.len());
                                // Try to find a market that matches our question closely
                                if let Some(market) = Self::find_best_match(&markets, &clean_question) {
                                    info!("Found market via search: {} (slug: {}, liq: {:?})",
                                        market.question, market.slug, market.liquidity);
                                    self.market_cache.insert(cache_key.clone(), market);
                                    return self.market_cache.get(&cache_key);
                                } else if !markets.is_empty() {
                                    // Log what we got back but didn't match
                                    for (i, m) in markets.iter().take(3).enumerate() {
                                        debug!("  Search result {}: {}", i, m.question);
                                    }
                                }
                            }
                            Err(e) => warn!("Failed to parse search response: {}", e),
                        }
                    } else {
                        warn!("Search returned non-success status");
                    }
                }
                Err(e) => warn!("Search request failed: {}", e),
            }
        }

        // Strategy 3: Keyword search with fewer, more specific terms
        {
            let search_terms: String = clean_question
                .split_whitespace()
                .filter(|w| {
                    let w_lower = w.to_lowercase();
                    w.len() > 3
                        && !["will", "what", "does", "have", "been", "that", "this",
                             "with", "from", "than", "more", "less", "before", "after"]
                            .contains(&w_lower.as_str())
                })
                .take(5)
                .collect::<Vec<_>>()
                .join(" ");

            if !search_terms.is_empty() && search_terms.len() > 5 {
                let encoded_search = urlencoding::encode(&search_terms);
                let url = format!(
                    "https://gamma-api.polymarket.com/markets?closed=false&limit=10&search={}",
                    encoded_search
                );

                info!("Retrying search with keywords: {}", search_terms);

                if let Ok(response) = self.client.get(&url).send().await {
                    if response.status().is_success() {
                        if let Ok(markets) = response.json::<Vec<GammaMarket>>().await {
                            info!("Keyword search returned {} markets", markets.len());
                            if let Some(market) = Self::find_best_match(&markets, &clean_question) {
                                info!("Found market via keyword search: {}", market.question);
                                self.market_cache.insert(cache_key.clone(), market);
                                return self.market_cache.get(&cache_key);
                            }
                        }
                    }
                }
            }
        }

        // Strategy 4: Try without closed=false (market might already be in closing state)
        {
            let encoded_search = urlencoding::encode(&clean_question);
            let url = format!(
                "https://gamma-api.polymarket.com/markets?limit=10&search={}",
                encoded_search
            );

            info!("Trying search without closed filter");

            if let Ok(response) = self.client.get(&url).send().await {
                if response.status().is_success() {
                    if let Ok(markets) = response.json::<Vec<GammaMarket>>().await {
                        info!("Unfiltered search returned {} markets", markets.len());
                        if let Some(market) = Self::find_best_match(&markets, &clean_question) {
                            info!("Found market via unfiltered search: {}", market.question);
                            self.market_cache.insert(cache_key.clone(), market);
                            return self.market_cache.get(&cache_key);
                        }
                    }
                }
            }
        }

        warn!("Could not find market for question: '{}'", &clean_question[..clean_question.len().min(100)]);
        None
    }

    /// Find the best matching market from search results
    /// Returns the market with the highest word overlap with the target question
    fn find_best_match(markets: &[GammaMarket], target_question: &str) -> Option<GammaMarket> {
        if markets.is_empty() {
            return None;
        }

        let target_words: Vec<String> = target_question
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_string())
            .collect();

        if target_words.is_empty() {
            // Just return the first result if we can't compare
            return markets.first().map(|m| GammaMarket {
                id: m.id.clone(),
                condition_id: m.condition_id.clone(),
                question: m.question.clone(),
                slug: m.slug.clone(),
                outcome_prices: m.outcome_prices.clone(),
                liquidity: m.liquidity.clone(),
                clob_token_ids: m.clob_token_ids.clone(),
                events: None, // Can't easily clone nested events
            });
        }

        let mut best_score = 0usize;
        let mut best_idx = 0usize;

        for (i, market) in markets.iter().enumerate() {
            let market_words: Vec<String> = market.question
                .to_lowercase()
                .split_whitespace()
                .filter(|w| w.len() > 2)
                .map(|w| w.to_string())
                .collect();

            let overlap = target_words.iter()
                .filter(|tw| market_words.iter().any(|mw| mw == *tw))
                .count();

            if overlap > best_score {
                best_score = overlap;
                best_idx = i;
            }
        }

        // Require at least 30% word overlap to consider it a match
        let min_overlap = (target_words.len() as f64 * 0.3).ceil() as usize;
        if best_score >= min_overlap.max(2) {
            let m = &markets[best_idx];
            debug!("Best match score: {}/{} words overlap", best_score, target_words.len());
            Some(GammaMarket {
                id: m.id.clone(),
                condition_id: m.condition_id.clone(),
                question: m.question.clone(),
                slug: m.slug.clone(),
                outcome_prices: m.outcome_prices.clone(),
                liquidity: m.liquidity.clone(),
                clob_token_ids: m.clob_token_ids.clone(),
                events: None,
            })
        } else {
            debug!("No good match found. Best was {}/{} words overlap", best_score, target_words.len());
            None
        }
    }

    /// Parse token IDs from JSON string
    fn parse_token_ids(ids_str: &Option<String>) -> (Option<String>, Option<String>) {
        let Some(ids_str) = ids_str else {
            return (None, None);
        };

        if let Ok(ids) = serde_json::from_str::<Vec<String>>(ids_str) {
            let yes_id = ids.first().cloned();
            let no_id = ids.get(1).cloned();
            return (yes_id, no_id);
        }

        (None, None)
    }

    /// Parse outcome prices from JSON string (handles both string array and number array formats)
    fn parse_prices(prices_str: &Option<String>) -> (Decimal, Decimal) {
        let Some(prices_str) = prices_str else {
            return (Decimal::ZERO, Decimal::ZERO);
        };

        // Try parsing as JSON array of strings first: ["0.5", "0.5"]
        if let Ok(prices) = serde_json::from_str::<Vec<String>>(prices_str) {
            if prices.len() >= 2 {
                let yes = Decimal::from_str(&prices[0]).unwrap_or_default();
                let no = Decimal::from_str(&prices[1]).unwrap_or_default();
                return (yes, no);
            }
        }

        // Try parsing as JSON array of numbers: [0.5, 0.5]
        if let Ok(prices) = serde_json::from_str::<Vec<f64>>(prices_str) {
            if prices.len() >= 2 {
                let yes = Decimal::try_from(prices[0]).unwrap_or_default();
                let no = Decimal::try_from(prices[1]).unwrap_or_default();
                return (yes, no);
            }
        }

        (Decimal::ZERO, Decimal::ZERO)
    }

    /// Calculate edge based on proposed outcome and current price
    fn calculate_edge(proposed_outcome: &str, yes_price: Decimal, no_price: Decimal) -> Option<Decimal> {
        // If proposed outcome is YES, edge = 1.0 - yes_price
        // If proposed outcome is NO, edge = 1.0 - no_price
        let one = Decimal::from(1);
        match proposed_outcome.to_uppercase().as_str() {
            "YES" => Some(one - yes_price),
            "NO" => Some(one - no_price),
            _ => None,
        }
    }

    /// Calculate expected value considering possible dispute outcomes (Item 7)
    ///
    /// Per official docs, four outcomes are possible:
    /// 1. Proposer prevails → token pays $1 (probability: P_hold)
    /// 2. Challenger prevails → token pays $0 (probability: P_challenge)
    /// 3. Too Early → reset, uncertain (probability: P_early)
    /// 4. 50-50 Unknown → each token pays $0.50 (probability: P_5050)
    ///
    /// For round 1 proposals (no dispute yet), P_hold is high (~85%).
    /// For round 2 re-proposals, P_hold is higher (~90%) since frivolous disputes filtered out.
    fn calculate_expected_value(
        proposed_outcome: &str,
        yes_price: Decimal,
        _no_price: Decimal,
        dispute_round: u8,
    ) -> Option<Decimal> {
        let price = match proposed_outcome.to_uppercase().as_str() {
            "YES" => yes_price,
            "NO" => Decimal::from(1) - yes_price,  // NO price = complement
            _ => return None,
        };

        // Estimated probabilities based on dispute round
        let (p_hold, p_5050) = if dispute_round >= 2 {
            // Round 2: first dispute already filtered, higher conviction
            (Decimal::new(90, 2), Decimal::new(2, 2))  // 90% hold, 2% 50-50
        } else {
            // Round 1: initial proposal
            (Decimal::new(85, 2), Decimal::new(3, 2))  // 85% hold, 3% 50-50
        };
        let p_fail = Decimal::from(1) - p_hold - p_5050;

        // EV = P(hold) * (1 - price) + P(50-50) * (0.50 - price) + P(fail) * (0 - price)
        let one = Decimal::from(1);
        let half = Decimal::new(50, 2);

        let ev = p_hold * (one - price) + p_5050 * (half - price) - p_fail * price;

        Some(ev)
    }

    /// Determine adapter version from callback recipient address
    fn adapter_version_from_address(callback: &Option<String>) -> Option<String> {
        let cb = callback.as_ref()?.to_lowercase();
        for (addr, version) in ADAPTERS {
            if cb.contains(addr.trim_start_matches("0x")) {
                return Some(version.to_string());
            }
        }
        None
    }

    /// Parse bond amount from subgraph (typically in USDC with 6 decimals)
    fn parse_bond(bond_str: &Option<String>) -> Option<Decimal> {
        let s = bond_str.as_ref()?;
        let raw = u128::from_str_radix(s.trim_start_matches("0x"), 10).ok()
            .or_else(|| s.parse::<u128>().ok())?;
        // USDC has 6 decimals
        let whole = raw / 1_000_000;
        let frac = raw % 1_000_000;
        Decimal::from_str(&format!("{}.{:06}", whole, frac)).ok()
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
                        info!("Found {} active Polymarket UMA disputes", alerts.len());
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
        let assertions = self.fetch_all_adapters().await?;
        let mut alerts = Vec::new();

        // Track which assertions we've seen this scan
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        // First pass: count assertions per domain_id for two-round detection (Item 1)
        let mut domain_counts: HashMap<String, u8> = HashMap::new();
        let mut domain_disputed: HashMap<String, bool> = HashMap::new();
        for assertion in &assertions {
            if let Some(domain) = &assertion.domain_id {
                if !domain.is_empty() {
                    let count = domain_counts.entry(domain.clone()).or_insert(0);
                    *count += 1;

                    // Track if any assertion for this domain was disputed
                    let is_disputed = assertion.disputer.as_ref()
                        .map(|d| !d.is_empty() && d != "null")
                        .unwrap_or(false);
                    if is_disputed {
                        domain_disputed.insert(domain.clone(), true);
                    }
                }
            }
        }
        self.domain_assertion_count = domain_counts;

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
            let is_settled = assertion.settlement_resolution.is_some()
                || assertion.settlement_timestamp.is_some();
            let is_disputed = assertion.disputer.is_some()
                && assertion.disputer.as_ref().map(|d| !d.is_empty() && d != "null").unwrap_or(false);

            // Skip settled assertions
            if is_settled {
                self.tracked_disputes.remove(&assertion_id);
                continue;
            }

            // Determine dispute round (Item 1, 10)
            // If this domain has had a previous disputed assertion, this one is round 2
            let domain_id = assertion.domain_id.clone().unwrap_or_default();
            let assertion_count = self.domain_assertion_count.get(&domain_id).copied().unwrap_or(1);
            let domain_had_dispute = domain_disputed.get(&domain_id).copied().unwrap_or(false);

            // Round 2 = this is a non-disputed assertion AND there's already a disputed one for same domain
            // OR assertion_count > 1 and this one is the newer (non-disputed) one
            let dispute_round = if !is_disputed && domain_had_dispute && assertion_count > 1 {
                2u8
            } else {
                1u8
            };

            // Determine status
            let status = if is_disputed {
                if dispute_round >= 2 {
                    // Second dispute → DVM escalation
                    DisputeStatus::DvmVote
                } else if assertion.dispute_timestamp.is_some() {
                    DisputeStatus::Disputed
                } else {
                    DisputeStatus::Proposed
                }
            } else {
                DisputeStatus::Proposed
            };

            // Parse timestamps
            let dispute_timestamp = assertion.dispute_timestamp
                .as_ref()
                .or(assertion.assertion_timestamp.as_ref())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            // Use actual expirationTime from subgraph (Item 6)
            let estimated_resolution = assertion.expiration_time
                .as_ref()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            // Calculate liveness period from assertion timestamp and expiration
            let assertion_ts = assertion.assertion_timestamp
                .as_ref()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let liveness_seconds = if assertion_ts > 0 && estimated_resolution > assertion_ts {
                Some(estimated_resolution - assertion_ts)
            } else {
                None
            };

            // Parse claim
            let (mut question, proposed_outcome) = self.parse_claim(&assertion);
            let condition_id = domain_id;

            // Determine adapter version (Item 2)
            let adapter_version = Self::adapter_version_from_address(&assertion.callback_recipient);

            // Parse bond amount (Item 4)
            let proposer_bond = Self::parse_bond(&assertion.bond);

            debug!("Assertion {} - round: {}, adapter: {:?}, bond: {:?}, domainId: '{}', question: '{}'",
                &assertion_id[..assertion_id.len().min(16)],
                dispute_round,
                adapter_version,
                proposer_bond,
                &condition_id[..condition_id.len().min(20)],
                &question[..question.len().min(60)]);

            // Try to get market data from Gamma API
            let mut slug = String::new();
            let mut yes_price = Decimal::ZERO;
            let mut no_price = Decimal::ZERO;
            let mut liquidity = Decimal::ZERO;
            let mut yes_token_id = None;
            let mut no_token_id = None;

            {
                if let Some(market) = self.get_market_data(&condition_id, &question).await {
                    if question.contains("...") || question.len() < 20 {
                        question = market.question.clone();
                    }

                    slug = market.events
                        .as_ref()
                        .and_then(|events| events.first())
                        .and_then(|e| e.slug.clone())
                        .unwrap_or_else(|| market.slug.clone());

                    let prices = Self::parse_prices(&market.outcome_prices);
                    yes_price = prices.0;
                    no_price = prices.1;

                    liquidity = market.liquidity
                        .as_ref()
                        .and_then(|l| Decimal::from_str(l).ok())
                        .unwrap_or_default();

                    let tokens = Self::parse_token_ids(&market.clob_token_ids);
                    yes_token_id = tokens.0;
                    no_token_id = tokens.1;

                    info!("Market data loaded - yes: {}, no: {}, liq: {}, slug: {}",
                        yes_price, no_price, liquidity, slug);
                } else {
                    warn!("No market data found for assertion {}", &assertion_id[..assertion_id.len().min(16)]);
                }
            }

            // Calculate edge and expected value (Item 7)
            let edge = Self::calculate_edge(&proposed_outcome, yes_price, no_price);
            let expected_value = Self::calculate_expected_value(
                &proposed_outcome, yes_price, no_price, dispute_round
            );

            let alert = DisputeAlert {
                assertion_id: assertion_id.clone(),
                condition_id: condition_id.clone(),
                question: question.clone(),
                slug,
                dispute_status: status,
                proposed_outcome,
                dispute_timestamp,
                estimated_resolution,
                current_yes_price: yes_price,
                current_no_price: no_price,
                liquidity,
                yes_token_id,
                no_token_id,
                edge,
                dispute_round,
                proposer_bond,
                adapter_version,
                liveness_seconds,
                expected_value,
            };

            // Check if status changed
            let status_changed = self.tracked_disputes
                .get(&assertion_id)
                .map(|(old_status, _)| *old_status != status)
                .unwrap_or(true);

            if status_changed {
                info!(
                    "Dispute {} status: {} (round {}) for market: {}",
                    &assertion_id[..assertion_id.len().min(16)], status, dispute_round,
                    &question[..question.len().min(80)]
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

    /// Fetch assertions from ALL Polymarket adapter versions (Item 2)
    async fn fetch_all_adapters(&self) -> Result<Vec<UmaAssertion>> {
        let mut all_assertions = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for (adapter_addr, version) in ADAPTERS {
            match self.fetch_assertions_for_adapter(adapter_addr).await {
                Ok(assertions) => {
                    let count = assertions.len();
                    for assertion in assertions {
                        let id = assertion.assertion_id.clone()
                            .unwrap_or_else(|| assertion.id.clone());
                        if seen_ids.insert(id) {
                            all_assertions.push(assertion);
                        }
                    }
                    if count > 0 {
                        info!("Fetched {} assertions from adapter {} ({})", count, version, adapter_addr);
                    }
                }
                Err(e) => {
                    // Non-fatal: log and continue with other adapters
                    warn!("Failed to fetch assertions from adapter {} ({}): {}", version, adapter_addr, e);
                }
            }
        }

        info!("Total: {} unique Polymarket assertions across all adapters", all_assertions.len());
        Ok(all_assertions)
    }

    /// Fetch assertions for a specific adapter address with retry + fallback (Items 2, 9)
    async fn fetch_assertions_for_adapter(&self, adapter_address: &str) -> Result<Vec<UmaAssertion>> {
        let query = format!(r#"
            {{
                assertions(
                    first: 100
                    orderBy: assertionTimestamp
                    orderDirection: desc
                    where: {{ callbackRecipient: "{}" }}
                ) {{
                    id
                    assertionId
                    claim
                    domainId
                    assertionTimestamp
                    expirationTime
                    disputer
                    settlementResolution
                    disputeTimestamp
                    settlementTimestamp
                    identifier
                    bond
                    currency
                    callbackRecipient
                }}
            }}
            "#, adapter_address);

        let request = GraphQLRequest {
            query,
            variables: None,
        };

        // Try primary endpoint first, then fallback (Item 9)
        let endpoints = [UMA_SUBGRAPH_URL, UMA_SUBGRAPH_FALLBACK_URL];

        let mut last_error = None;
        for (ep_idx, endpoint) in endpoints.iter().enumerate() {
            let ep_label = if ep_idx == 0 { "primary" } else { "fallback" };

            // Retry up to 3 times per endpoint with exponential backoff
            for attempt in 0..3 {
                if attempt > 0 {
                    let delay = Duration::from_secs(2u64.pow(attempt));
                    debug!("Retrying UMA subgraph ({}) query in {:?}...", ep_label, delay);
                    tokio::time::sleep(delay).await;
                }

                match self.client
                    .post(*endpoint)
                    .json(&request)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if !response.status().is_success() {
                            last_error = Some(format!("Subgraph {} error: {}", ep_label, response.status()));
                            continue;
                        }

                        match response.json::<GraphQLResponse>().await {
                            Ok(result) => {
                                if let Some(errors) = &result.errors {
                                    if !errors.is_empty() {
                                        warn!("GraphQL errors from {}: {:?}", ep_label, errors);
                                    }
                                }

                                let assertions = result.data
                                    .and_then(|d| d.assertions)
                                    .unwrap_or_default();

                                return Ok(assertions);
                            }
                            Err(e) => {
                                last_error = Some(format!("Failed to parse {} response: {}", ep_label, e));
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        last_error = Some(format!("{} request failed: {}", ep_label, e));
                        continue;
                    }
                }
            }

            // If primary failed all retries, try fallback
            if ep_idx == 0 {
                warn!("Primary UMA subgraph failed for adapter {}, trying fallback...", adapter_address);
            }
        }

        anyhow::bail!("{}", last_error.unwrap_or_else(|| "Unknown error".to_string()))
    }

    /// Parse claim data to extract market question and proposed outcome
    fn parse_claim(&self, assertion: &UmaAssertion) -> (String, String) {
        let mut question = String::new();
        let mut proposed_outcome = "Unknown".to_string();

        if let Some(claim) = &assertion.claim {
            // Decode hex-encoded claim data
            let decoded_text = if claim.starts_with("0x") && claim.len() > 2 {
                hex::decode(&claim[2..])
                    .ok()
                    .map(|bytes| {
                        // Extract printable ASCII characters
                        bytes.iter()
                            .filter(|&&b| b >= 32 && b < 127)
                            .map(|&b| b as char)
                            .collect::<String>()
                    })
                    .unwrap_or_default()
            } else {
                claim.clone()
            };

            if decoded_text.len() > 10 {
                // Log decoded text for debugging
                info!("Decoded claim (first 200 chars): {}",
                    &decoded_text[..decoded_text.len().min(200)]);

                // === Extract proposed outcome ===
                // Look for "outcome is YES/NO" or similar patterns
                let text_lower = decoded_text.to_lowercase();
                if let Some(outcome_pos) = text_lower.find("outcome") {
                    let after_outcome = &text_lower[outcome_pos..];
                    // Check for YES after "outcome"
                    if let Some(yes_pos) = after_outcome.find("yes") {
                        let abs_pos = outcome_pos + yes_pos;
                        let end = abs_pos + 3;
                        if end >= decoded_text.len() || !decoded_text.as_bytes().get(end).map(|b| b.is_ascii_alphabetic()).unwrap_or(false) {
                            proposed_outcome = "Yes".to_string();
                        }
                    }
                    // Check for NO after "outcome" (only if we didn't find YES)
                    if proposed_outcome == "Unknown" {
                        // Look for " no" (with space before) to avoid matching "not", "none", etc.
                        if let Some(no_pos) = after_outcome.find(" no") {
                            let abs_pos = outcome_pos + no_pos + 1; // +1 to skip the space
                            let end = abs_pos + 2;
                            if end >= decoded_text.len() || !decoded_text.as_bytes().get(end).map(|b| b.is_ascii_alphabetic()).unwrap_or(false) {
                                proposed_outcome = "No".to_string();
                            }
                        }
                    }
                }

                // Also check for p3: "Yes" or p3: "No" pattern (Polymarket specific)
                if proposed_outcome == "Unknown" {
                    if let Some(p3_pos) = text_lower.find("p3:") {
                        let after_p3 = &text_lower[p3_pos + 3..];
                        let trimmed = after_p3.trim_start();
                        if trimmed.starts_with("\"yes\"") || trimmed.starts_with("yes") {
                            proposed_outcome = "Yes".to_string();
                        } else if trimmed.starts_with("\"no\"") || trimmed.starts_with("no") {
                            proposed_outcome = "No".to_string();
                        }
                    }
                }

                // === Extract question text ===
                // Get text before "Asserting" (the claim format is: <question_data> Asserting that...)
                let before_asserting = if let Some(idx) = text_lower.find("asserting") {
                    decoded_text[..idx].to_string()
                } else {
                    decoded_text.clone()
                };

                let before = before_asserting.trim();
                let before_lower = before.to_lowercase();

                // Try format: "q: title: <question>, description: ..."
                if let Some(title_idx) = before_lower.find("title:") {
                    let start = title_idx + 6;
                    if start < before.len() {
                        let after_title = before[start..].trim_start();
                        // Find end of title - stop at description, res_data, etc.
                        let end_markers = [", description:", ". res_data:", "res_data:", ",description:"];
                        let end = end_markers.iter()
                            .filter_map(|m| after_title.to_lowercase().find(m))
                            .min()
                            .unwrap_or(after_title.len());
                        question = after_title[..end]
                            .trim()
                            .trim_end_matches(',')
                            .trim_end_matches('.')
                            .to_string();
                    }
                }
                // Try format: "q: <question>"
                else if let Some(q_idx) = before_lower.find("q:") {
                    let start = q_idx + 2;
                    if start < before.len() {
                        let after_q = before[start..].trim_start();
                        let end_markers = [", description:", ". res_data:", "res_data:", ",description:"];
                        let end = end_markers.iter()
                            .filter_map(|m| after_q.to_lowercase().find(m))
                            .min()
                            .unwrap_or(after_q.len());
                        question = after_q[..end]
                            .trim()
                            .trim_end_matches(',')
                            .trim_end_matches('.')
                            .to_string();
                    }
                }
                // No structured format - use raw text before "Asserting"
                else {
                    question = before
                        .trim_start_matches('/')
                        .trim_start_matches('\\')
                        .trim()
                        .to_string();
                }

                // Clean up: remove leading non-alphanumeric garbage
                while !question.is_empty() {
                    if let Some(ch) = question.chars().next() {
                        if ch.is_alphanumeric() || ch == '"' {
                            break;
                        }
                        question = question[ch.len_utf8()..].to_string();
                    } else {
                        break;
                    }
                }
                // Remove surrounding quotes
                question = question.trim_matches('"').trim().to_string();

                // If still empty or too short, use full decoded text
                if question.len() < 10 {
                    question = decoded_text.chars().take(200).collect();
                }
            } else {
                // Very short decoded text - show hex
                let display_len = 40.min(claim.len());
                question = format!("{}...", &claim[..display_len]);
            }
        }

        if question.is_empty() {
            question = "Unknown market".to_string();
        }

        info!("Parsed claim => question: '{}', outcome: '{}'",
            &question[..question.len().min(100)], proposed_outcome);

        (question, proposed_outcome)
    }
}
