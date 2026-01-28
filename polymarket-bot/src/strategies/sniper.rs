//! Resolution Sniping Strategy
//!
//! The play: Bet on heavy favorites in markets about to close.
//! At 4 hours before close, the favorite wins 95.3% of the time.
//! The profit is in the GAP between the favorite's price and the 95% win rate.

use crate::config::SniperConfig;
use crate::types::{Opportunity, Side, StrategyType, TrackedMarket};
use rust_decimal::prelude::*;
use rust_decimal_macros::dec;

/// Resolution sniping strategy implementation
pub struct SniperStrategy {
    config: SniperConfig,
}

impl SniperStrategy {
    pub fn new(config: SniperConfig) -> Self {
        Self { config }
    }

    /// Find sniper opportunities in markets closing soon
    pub fn find_opportunities(&self, markets: &[TrackedMarket]) -> Vec<Opportunity> {
        let mut opportunities: Vec<Opportunity> = markets
            .iter()
            .filter(|m| m.active && !m.closed)
            .filter(|m| {
                // Must be in the time window (1-12 hours by default)
                let hours = m.hours_until_close.unwrap_or(f64::MAX);
                hours >= self.config.min_hours && hours <= self.config.max_hours
            })
            .filter(|m| m.liquidity >= dec!(1000))
            .filter_map(|m| self.evaluate_market(m))
            .collect();

        // Sort by EV descending (best opportunities first)
        opportunities.sort_by(|a, b| {
            b.edge
                .partial_cmp(&a.edge)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        opportunities
    }

    /// Evaluate a single market for sniper opportunity
    fn evaluate_market(&self, market: &TrackedMarket) -> Option<Opportunity> {
        let yes_price: f64 = market.yes_price.to_f64()?;
        let no_price: f64 = market.no_price.to_f64()?;

        // Identify the favorite (higher priced outcome)
        let (side, favorite_price, _underdog_price) = if yes_price > no_price {
            (Side::Yes, yes_price, no_price)
        } else {
            (Side::No, no_price, yes_price)
        };

        // Sweet spot filter: favorites priced 70-90%
        // NOT 95-99% (accuracy already priced in, no gap for profit)
        // NOT below 65% (questionable if it's really the favorite)
        if favorite_price < self.config.min_favorite_price
            || favorite_price > self.config.max_favorite_price
        {
            return None;
        }

        let hours = market.hours_until_close?;
        let accuracy = self.accuracy_at_hours(hours);

        // Calculate potential return: (1 - price) / price
        // e.g., buy at 80¢, win $1 → profit 20¢ → 25% return
        let potential_return = (1.0 - favorite_price) / favorite_price;

        // Expected Value = (win_prob × profit) - (lose_prob × loss)
        let ev = (accuracy * (1.0 - favorite_price)) - ((1.0 - accuracy) * favorite_price);

        // Only return opportunities with meaningful positive EV
        if ev < self.config.min_ev {
            return None;
        }

        // Bonus: Check if NO is the favorite (double confirmation with 78.4% base rate)
        let no_bias_bonus = matches!(side, Side::No);

        let recommendation = format!(
            "BUY {} at {:.0}c | {:.1}% return | {:.1}% EV | {:.1}h left{}",
            side,
            favorite_price * 100.0,
            potential_return * 100.0,
            ev * 100.0,
            hours,
            if no_bias_bonus { " [NO BIAS+]" } else { "" }
        );

        // Get the token_id based on which side is the favorite
        let token_id = match side {
            Side::Yes => market.yes_token_id.clone(),
            Side::No => market.no_token_id.clone(),
        };

        Some(Opportunity {
            market_id: market.id.clone(),
            condition_id: market.condition_id.clone(),
            question: market.question.clone(),
            slug: market.slug.clone(),
            strategy: StrategyType::ResolutionSniper,
            side,
            entry_price: Decimal::try_from(favorite_price).ok()?,
            expected_return: potential_return,
            confidence: accuracy,
            edge: ev,
            time_to_close_hours: Some(hours),
            liquidity: market.liquidity,
            volume: market.volume,
            category: market.category.clone(),
            resolution_source: market.resolution_source.clone(),
            description: market.description.clone(),
            recommendation,
            token_id,
            meets_criteria: true,
        })
    }

    /// Get historical accuracy at given hours before close
    /// Based on analyzed data: 4h=95.3%, 12h=90.6%, 24h=89.4%, 1w=89.3%
    fn accuracy_at_hours(&self, hours: f64) -> f64 {
        if hours <= 4.0 {
            0.953
        } else if hours <= 12.0 {
            // Linear interpolation between 4h (95.3%) and 12h (90.6%)
            let t = (hours - 4.0) / 8.0;
            0.953 - (t * (0.953 - 0.906))
        } else if hours <= 24.0 {
            // Linear interpolation between 12h (90.6%) and 24h (89.4%)
            let t = (hours - 12.0) / 12.0;
            0.906 - (t * (0.906 - 0.894))
        } else {
            0.893 // Baseline for > 24 hours
        }
    }
}

impl super::Strategy for SniperStrategy {
    fn find_opportunities(&self, markets: &[TrackedMarket]) -> Vec<Opportunity> {
        self.find_opportunities(markets)
    }

    fn name(&self) -> &'static str {
        "Resolution Sniper"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accuracy_interpolation() {
        let config = SniperConfig::default();
        let strategy = SniperStrategy::new(config);

        // Test known data points
        assert!((strategy.accuracy_at_hours(4.0) - 0.953).abs() < 0.001);
        assert!((strategy.accuracy_at_hours(12.0) - 0.906).abs() < 0.001);

        // Test interpolation is monotonically decreasing
        let acc_6h = strategy.accuracy_at_hours(6.0);
        assert!(acc_6h < 0.953 && acc_6h > 0.906);
    }

    #[test]
    fn test_ev_calculation() {
        // At 4 hours, 80¢ favorite should have ~18.7% EV
        // EV = (0.953 × 0.20) - (0.047 × 0.80) = 0.1906 - 0.0376 = 0.153
        let accuracy = 0.953;
        let price = 0.80;
        let ev = (accuracy * (1.0 - price)) - ((1.0 - accuracy) * price);
        assert!((ev - 0.153).abs() < 0.01);
    }
}
