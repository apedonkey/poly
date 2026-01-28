//! NO Bias Strategy
//!
//! The play: Exploit the structural bias that 78.4% of markets resolve NO.
//! People create markets hoping YES happens, which inflates YES prices.
//! Buy NO when it's undervalued relative to the historical resolution rate.

use crate::config::NoBiasConfig;
use crate::types::{Opportunity, Side, StrategyType, TrackedMarket};
use rust_decimal::prelude::*;
use rust_decimal_macros::dec;

/// NO bias strategy implementation
pub struct NoBiasStrategy {
    config: NoBiasConfig,
}

impl NoBiasStrategy {
    pub fn new(config: NoBiasConfig) -> Self {
        Self { config }
    }

    /// Find NO bias opportunities in markets
    pub fn find_opportunities(&self, markets: &[TrackedMarket]) -> Vec<Opportunity> {
        let mut opportunities: Vec<Opportunity> = markets
            .iter()
            .filter(|m| m.active && !m.closed)
            .filter(|m| {
                // Skip markets closing soon (sniper handles those)
                m.hours_until_close.unwrap_or(f64::MAX) > self.config.min_hours
            })
            .filter(|m| m.liquidity >= dec!(1000))
            .filter(|m| {
                // Skip fairly-priced categories (sports, crypto)
                !self.is_excluded_category(m.category.as_deref())
            })
            .filter_map(|m| self.evaluate_market(m))
            .collect();

        // Sort by edge descending (best opportunities first)
        opportunities.sort_by(|a, b| {
            b.edge
                .partial_cmp(&a.edge)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        opportunities
    }

    /// Check if category should be excluded (fairly priced markets)
    fn is_excluded_category(&self, category: Option<&str>) -> bool {
        let Some(cat) = category else {
            return false;
        };

        self.config
            .excluded_categories
            .iter()
            .any(|excluded| cat.to_lowercase().contains(&excluded.to_lowercase()))
    }

    /// Evaluate a single market for NO bias opportunity
    fn evaluate_market(&self, market: &TrackedMarket) -> Option<Opportunity> {
        let no_price: f64 = market.no_price.to_f64()?;
        let yes_price: f64 = market.yes_price.to_f64()?;

        // YES should be in tradeable range (not already decided)
        if yes_price < 0.20 || yes_price > 0.80 {
            return None;
        }

        // Calculate edge: historical NO rate - current NO price
        let edge = self.config.historical_no_rate - no_price;

        // Need meaningful edge (at least 10% by default)
        if edge < self.config.min_edge {
            return None;
        }

        // Expected return if NO wins
        let potential_return = (1.0 - no_price) / no_price;

        // Expected Value using historical NO rate as probability estimate
        // Note: This is category-agnostic. Real implementation should adjust
        // based on market type (politics, entertainment, etc.)
        let _ev = (self.config.historical_no_rate * (1.0 - no_price))
            - ((1.0 - self.config.historical_no_rate) * no_price);

        let hours_str = market
            .hours_until_close
            .map(|h| {
                if h > 24.0 * 7.0 {
                    format!("{:.0} weeks", h / (24.0 * 7.0))
                } else if h > 24.0 {
                    format!("{:.0} days", h / 24.0)
                } else {
                    format!("{:.0}h", h)
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let recommendation = format!(
            "BUY NO at {:.0}c | {:.1}% edge vs {:.1}% base rate | {} left",
            no_price * 100.0,
            edge * 100.0,
            self.config.historical_no_rate * 100.0,
            hours_str
        );

        Some(Opportunity {
            market_id: market.id.clone(),
            condition_id: market.condition_id.clone(),
            question: market.question.clone(),
            slug: market.slug.clone(),
            strategy: StrategyType::NoBias,
            side: Side::No,
            entry_price: market.no_price,
            expected_return: potential_return,
            confidence: self.config.historical_no_rate,
            edge,
            time_to_close_hours: market.hours_until_close,
            liquidity: market.liquidity,
            volume: market.volume,
            category: market.category.clone(),
            resolution_source: market.resolution_source.clone(),
            description: market.description.clone(),
            recommendation,
            token_id: market.no_token_id.clone(),
            meets_criteria: true,
        })
    }
}

impl super::Strategy for NoBiasStrategy {
    fn find_opportunities(&self, markets: &[TrackedMarket]) -> Vec<Opportunity> {
        self.find_opportunities(markets)
    }

    fn name(&self) -> &'static str {
        "NO Bias"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_calculation() {
        // If NO is priced at 35¢ and historical rate is 78.4%
        // Edge = 0.784 - 0.35 = 0.434 (43.4%)
        let historical_rate = 0.784;
        let no_price = 0.35;
        let edge = historical_rate - no_price;
        assert!((edge - 0.434).abs() < 0.001);
    }

    #[test]
    fn test_excluded_categories() {
        let config = NoBiasConfig::default();
        let strategy = NoBiasStrategy::new(config);

        assert!(strategy.is_excluded_category(Some("Sports")));
        assert!(strategy.is_excluded_category(Some("Crypto")));
        assert!(strategy.is_excluded_category(Some("NBA Basketball")));
        assert!(!strategy.is_excluded_category(Some("Politics")));
        assert!(!strategy.is_excluded_category(Some("Entertainment")));
        assert!(!strategy.is_excluded_category(None));
    }
}
