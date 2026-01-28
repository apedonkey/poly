//! Position Monitor - monitors open positions for auto-sell triggers
//!
//! Listens to real-time price updates and checks if any position should be sold:
//! - Take Profit: price increased by X%
//! - Stop Loss: price decreased by X%
//! - Trailing Stop: price dropped X% from peak
//! - Time Exit: position held for X hours

use super::types::{ExitTrigger, PositionPeak};
use crate::db::Database;
use crate::services::price_ws::PriceUpdate;
use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

/// Sell signal sent to the auto-seller
#[derive(Debug, Clone)]
pub struct SellSignal {
    pub position_id: i64,
    pub wallet_address: String,
    pub token_id: String,
    pub current_price: Decimal,
    pub trigger: ExitTrigger,
    pub size: Decimal,
    pub market_question: String,
}

/// Position Monitor service
pub struct PositionMonitor {
    db: Arc<Database>,
    /// In-memory cache of position peaks for trailing stops
    peaks: Arc<RwLock<HashMap<i64, PositionPeak>>>,
}

impl PositionMonitor {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            peaks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Run the position monitor
    ///
    /// Listens for price updates and checks all positions against their triggers
    pub async fn run(
        &self,
        mut price_rx: broadcast::Receiver<PriceUpdate>,
        sell_tx: mpsc::Sender<SellSignal>,
    ) {
        info!("Position monitor started");

        loop {
            match price_rx.recv().await {
                Ok(update) => {
                    if let Err(e) = self.check_positions(&update, &sell_tx).await {
                        warn!("Error checking positions: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("Position monitor lagged {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Price channel closed, shutting down position monitor");
                    break;
                }
            }
        }
    }

    /// Check all positions with the given token_id for exit triggers
    async fn check_positions(
        &self,
        update: &PriceUpdate,
        sell_tx: &mpsc::Sender<SellSignal>,
    ) -> Result<()> {
        let current_price = Decimal::from_str(&update.price)?;

        // Get all open positions with this token_id
        let positions = self.db.get_positions_by_token_id(&update.token_id).await?;

        for position in positions {
            // Get auto-trading settings for this wallet
            let settings = self.db.get_auto_trading_settings(&position.wallet_address).await?;
            if !settings.enabled {
                continue; // Auto-trading not enabled for this wallet
            }

            let entry_price = position.entry_price;
            let pnl_percent = (current_price - entry_price) / entry_price;

            // Check Take Profit
            if settings.take_profit_enabled {
                let target_percent = Decimal::try_from(settings.take_profit_percent)?;
                if pnl_percent >= target_percent {
                    info!(
                        "[Auto-Sell] Take Profit triggered for position {} at {:.1}%",
                        position.id,
                        pnl_percent * Decimal::from(100)
                    );

                    let signal = SellSignal {
                        position_id: position.id,
                        wallet_address: position.wallet_address.clone(),
                        token_id: update.token_id.clone(),
                        current_price,
                        trigger: ExitTrigger::TakeProfit {
                            price: current_price,
                            pnl_percent,
                        },
                        size: position.size,
                        market_question: position.question.clone(),
                    };

                    if sell_tx.send(signal).await.is_err() {
                        warn!("Failed to send sell signal - channel closed");
                    }
                    continue;
                }
            }

            // Check Stop Loss
            if settings.stop_loss_enabled {
                let stop_percent = Decimal::try_from(settings.stop_loss_percent)?;
                if pnl_percent <= -stop_percent {
                    info!(
                        "[Auto-Sell] Stop Loss triggered for position {} at {:.1}%",
                        position.id,
                        pnl_percent * Decimal::from(100)
                    );

                    let signal = SellSignal {
                        position_id: position.id,
                        wallet_address: position.wallet_address.clone(),
                        token_id: update.token_id.clone(),
                        current_price,
                        trigger: ExitTrigger::StopLoss {
                            price: current_price,
                            pnl_percent,
                        },
                        size: position.size,
                        market_question: position.question.clone(),
                    };

                    if sell_tx.send(signal).await.is_err() {
                        warn!("Failed to send sell signal - channel closed");
                    }
                    continue;
                }
            }

            // Check Trailing Stop
            if settings.trailing_stop_enabled {
                let trail_percent = Decimal::try_from(settings.trailing_stop_percent)?;

                // Update peak price
                let mut peaks = self.peaks.write().await;
                let peak = peaks.entry(position.id).or_insert_with(|| PositionPeak {
                    position_id: position.id,
                    peak_price: entry_price,
                    peak_at: chrono::Utc::now(),
                });

                // Update peak if current price is higher
                if current_price > peak.peak_price {
                    peak.peak_price = current_price;
                    peak.peak_at = chrono::Utc::now();

                    // Also persist to DB
                    if let Err(e) = self.db.update_position_peak(position.id, current_price).await {
                        warn!("Failed to persist peak price: {}", e);
                    }
                }

                // Check if dropped from peak by trailing percent
                let drop_from_peak = (peak.peak_price - current_price) / peak.peak_price;
                if drop_from_peak >= trail_percent && current_price > entry_price {
                    // Only trigger trailing stop if we're still in profit
                    info!(
                        "[Auto-Sell] Trailing Stop triggered for position {} (dropped {:.1}% from peak)",
                        position.id,
                        drop_from_peak * Decimal::from(100)
                    );

                    let signal = SellSignal {
                        position_id: position.id,
                        wallet_address: position.wallet_address.clone(),
                        token_id: update.token_id.clone(),
                        current_price,
                        trigger: ExitTrigger::TrailingStop {
                            peak: peak.peak_price,
                            price: current_price,
                            drop_percent: drop_from_peak,
                        },
                        size: position.size,
                        market_question: position.question.clone(),
                    };

                    // Remove from cache since position will be closed
                    peaks.remove(&position.id);

                    if sell_tx.send(signal).await.is_err() {
                        warn!("Failed to send sell signal - channel closed");
                    }
                    continue;
                }
            }

            // Check Time Exit
            if settings.time_exit_enabled {
                let hours_held = (chrono::Utc::now() - position.opened_at).num_hours() as f64;
                if hours_held >= settings.time_exit_hours {
                    info!(
                        "[Auto-Sell] Time Exit triggered for position {} after {:.1}h",
                        position.id, hours_held
                    );

                    let signal = SellSignal {
                        position_id: position.id,
                        wallet_address: position.wallet_address.clone(),
                        token_id: update.token_id.clone(),
                        current_price,
                        trigger: ExitTrigger::TimeExit {
                            hours_held,
                            price: current_price,
                        },
                        size: position.size,
                        market_question: position.question.clone(),
                    };

                    if sell_tx.send(signal).await.is_err() {
                        warn!("Failed to send sell signal - channel closed");
                    }
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Load peak prices from database on startup
    pub async fn load_peaks(&self) -> Result<()> {
        let db_peaks = self.db.get_all_position_peaks().await?;
        let mut peaks = self.peaks.write().await;

        for p in db_peaks {
            peaks.insert(p.position_id, p);
        }

        info!("Loaded {} position peaks from database", peaks.len());
        Ok(())
    }
}
