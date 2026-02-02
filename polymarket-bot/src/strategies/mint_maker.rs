//! Mint Maker strategy - market making on 15-min crypto Up/Down markets
//!
//! Detects eligible markets and calculates two-sided bid prices.
//! The actual order management runs in its own service loop (not the opportunity pipeline).

use crate::config::MintMakerConfig;
use crate::types::{CryptoAsset, MintMakerMarket, TrackedMarket};
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::debug;

/// Mint Maker strategy for finding eligible 15-min crypto markets
pub struct MintMakerStrategy {
    pub config: MintMakerConfig,
}

impl MintMakerStrategy {
    pub fn new(config: MintMakerConfig) -> Self {
        Self { config }
    }

    /// Find eligible 15-min crypto Up/Down markets from scanned markets
    pub fn find_markets(&self, markets: &[TrackedMarket]) -> Vec<MintMakerMarket> {
        let mut eligible = Vec::new();

        for market in markets {
            if let Some(mm) = self.check_market(market) {
                eligible.push(mm);
            }
        }

        debug!("MintMaker: found {} eligible markets", eligible.len());
        eligible
    }

    /// Check if a single market is eligible for mint making
    fn check_market(&self, market: &TrackedMarket) -> Option<MintMakerMarket> {
        // Must be active and not closed
        if !market.active || market.closed {
            return None;
        }

        // Must have both token IDs
        let yes_token = market.yes_token_id.as_ref()?;
        let no_token = market.no_token_id.as_ref()?;

        // Must have valid time to close
        let hours = market.hours_until_close?;
        let minutes = hours * 60.0;

        // Check time window
        if minutes < self.config.min_minutes_to_close || minutes > self.config.max_minutes_to_close {
            return None;
        }

        // Detect crypto asset from question
        let asset = self.detect_crypto_asset(&market.question)?;

        // Check if this asset is in our configured list
        let asset_str = asset.to_string();
        if !self.config.assets.iter().any(|a| a.eq_ignore_ascii_case(&asset_str)) {
            return None;
        }

        // Must be an Up/Down style market (above/below/higher/lower patterns)
        if !self.is_up_down_market(&market.question) {
            return None;
        }

        Some(MintMakerMarket {
            market_id: market.id.clone(),
            condition_id: market.condition_id.clone(),
            question: market.question.clone(),
            slug: market.slug.clone(),
            asset,
            yes_token_id: yes_token.clone(),
            no_token_id: no_token.clone(),
            yes_price: market.yes_price,
            no_price: market.no_price,
            minutes_to_close: minutes,
            neg_risk: market.neg_risk,
        })
    }

    /// Detect which crypto asset a market question is about
    fn detect_crypto_asset(&self, question: &str) -> Option<CryptoAsset> {
        let q = question.to_lowercase();

        for asset in CryptoAsset::all() {
            for keyword in asset.keywords() {
                // Word boundary check
                if contains_word(&q, keyword) {
                    return Some(*asset);
                }
            }
        }

        None
    }

    /// Check if market is an Up/Down style (price above/below threshold)
    fn is_up_down_market(&self, question: &str) -> bool {
        let q = question.to_lowercase();
        let patterns = [
            "above", "below", "higher", "lower", "over", "under",
            "up", "down", "rise", "fall", "hit", "reach", "exceed",
        ];
        patterns.iter().any(|p| q.contains(p))
    }

    /// Calculate bid prices for a market.
    /// Returns (yes_bid, no_bid) if the pair is profitable.
    pub fn calculate_bids(
        &self,
        market: &MintMakerMarket,
        _user_size: Decimal,
    ) -> Option<(Decimal, Decimal)> {
        let offset = Decimal::from(self.config.bid_offset_cents) / Decimal::from(100);

        // Bid below current price
        let yes_bid = market.yes_price - offset;
        let no_bid = market.no_price - offset;

        // Validate bids are positive
        if yes_bid <= Decimal::ZERO || no_bid <= Decimal::ZERO {
            return None;
        }

        // Validate pair cost
        let pair_cost = yes_bid + no_bid;
        let max_cost = Decimal::from_str(&format!("{:.4}", self.config.max_pair_cost)).unwrap_or(Decimal::from_str("0.98").unwrap());
        if pair_cost > max_cost {
            return None;
        }

        // Calculate profit (payout is always $1.00 per pair)
        let profit = Decimal::ONE - pair_cost;
        let min_profit = Decimal::from_str(&format!("{:.4}", self.config.min_spread_profit)).unwrap_or(Decimal::from_str("0.01").unwrap());
        if profit < min_profit {
            return None;
        }

        Some((yes_bid, no_bid))
    }
}

/// Helper for word boundary matching
fn contains_word(text: &str, word: &str) -> bool {
    let text = text.to_lowercase();
    let word = word.to_lowercase();

    for (i, _) in text.match_indices(&word) {
        let before_ok = i == 0 || !text.chars().nth(i - 1).unwrap_or(' ').is_alphanumeric();
        let after_ok = i + word.len() >= text.len()
            || !text.chars().nth(i + word.len()).unwrap_or(' ').is_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }

    false
}
