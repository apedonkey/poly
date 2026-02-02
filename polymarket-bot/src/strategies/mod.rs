//! Trading strategies for Polymarket

pub mod mint_maker;
pub mod sniper;

pub use mint_maker::MintMakerStrategy;
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
    pub mint_maker: MintMakerStrategy,
}

impl StrategyRunner {
    pub fn new(config: &Config) -> Self {
        Self {
            sniper: SniperStrategy::new(config.sniper.clone()),
            mint_maker: MintMakerStrategy::new(config.mint_maker.clone()),
        }
    }

    /// Run all strategies and collect opportunities
    pub fn find_all_opportunities(&self, markets: &[TrackedMarket]) -> AllOpportunities {
        AllOpportunities {
            sniper: self.sniper.find_opportunities(markets),
        }
    }
}

/// Collection of opportunities from all strategies
pub struct AllOpportunities {
    pub sniper: Vec<Opportunity>,
}

impl AllOpportunities {
    pub fn total_count(&self) -> usize {
        self.sniper.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sniper.is_empty()
    }
}
