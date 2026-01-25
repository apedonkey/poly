//! Trading strategies for Polymarket

pub mod no_bias;
pub mod sniper;

pub use no_bias::NoBiasStrategy;
pub use sniper::SniperStrategy;

use crate::config::Config;
use crate::types::{Opportunity, TrackedMarket};

/// Trait for trading strategies
pub trait Strategy {
    /// Find trading opportunities from a list of markets
    fn find_opportunities(&self, markets: &[TrackedMarket]) -> Vec<Opportunity>;

    /// Get strategy name for display
    fn name(&self) -> &'static str;
}

/// Combined strategy runner
pub struct StrategyRunner {
    pub sniper: SniperStrategy,
    pub no_bias: NoBiasStrategy,
}

impl StrategyRunner {
    pub fn new(config: &Config) -> Self {
        Self {
            sniper: SniperStrategy::new(config.sniper.clone()),
            no_bias: NoBiasStrategy::new(config.no_bias.clone()),
        }
    }

    /// Run all strategies and collect opportunities
    pub fn find_all_opportunities(&self, markets: &[TrackedMarket]) -> AllOpportunities {
        AllOpportunities {
            sniper: self.sniper.find_opportunities(markets),
            no_bias: self.no_bias.find_opportunities(markets),
        }
    }
}

/// Collection of opportunities from all strategies
pub struct AllOpportunities {
    pub sniper: Vec<Opportunity>,
    pub no_bias: Vec<Opportunity>,
}

impl AllOpportunities {
    pub fn total_count(&self) -> usize {
        self.sniper.len() + self.no_bias.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sniper.is_empty() && self.no_bias.is_empty()
    }
}
