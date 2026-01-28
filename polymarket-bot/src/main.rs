//! Polymarket Trading Bot CLI
//!
//! A trading bot for Polymarket prediction markets.

use anyhow::Result;
use clap::{Parser, Subcommand};
use polymarket_bot::{Config, Database, DiscordWebhook, Executor, Scanner, StrategyRunner};
use std::collections::HashSet;
use std::time::Duration;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "polymarket-bot")]
#[command(about = "Trading bot for Polymarket prediction markets")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan markets and show all opportunities
    Scan {
        /// Maximum number of opportunities to show per strategy
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Exclude sports markets
        #[arg(long)]
        no_sports: bool,
    },

    /// Show only sniper opportunities (markets closing soon)
    Snipe {
        /// Maximum hours until close
        #[arg(short = 'H', long, default_value = "12")]
        max_hours: f64,

        /// Maximum number of opportunities to show
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Exclude sports markets
        #[arg(long)]
        no_sports: bool,
    },

    /// Show only NO bias opportunities
    Bias {
        /// Minimum edge percentage
        #[arg(short, long, default_value = "10")]
        min_edge: f64,

        /// Maximum number of opportunities to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Run the bot continuously (paper trading)
    Run {
        /// Scan interval in seconds
        #[arg(short, long, default_value = "60")]
        interval: u64,

        /// Auto-execute opportunities above this EV threshold
        #[arg(short, long)]
        auto_execute: Option<f64>,
    },

    /// Show bot statistics
    Stats,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .compact()
        .init();

    // Load configuration
    let config = Config::from_env()?;

    match cli.command {
        Commands::Scan { limit, no_sports } => scan_markets(&config, limit, no_sports).await?,
        Commands::Snipe { max_hours, limit, no_sports } => snipe_markets(&config, max_hours, limit, no_sports).await?,
        Commands::Bias { min_edge, limit } => bias_markets(&config, min_edge, limit).await?,
        Commands::Run { interval, auto_execute } => run_bot(&config, interval, auto_execute).await?,
        Commands::Stats => show_stats(&config).await?,
    }

    Ok(())
}

async fn scan_markets(config: &Config, limit: usize, no_sports: bool) -> Result<()> {
    println!("\n{}", "=".repeat(70));
    println!("  POLYMARKET SCANNER");
    println!("  Paper Trading: {} | Sports Filter: {}",
        if config.paper_trading { "YES" } else { "NO - LIVE MODE" },
        if no_sports { "EXCLUDED" } else { "INCLUDED" });
    println!("{}\n", "=".repeat(70));

    let scanner = Scanner::new(config.clone());
    let runner = StrategyRunner::new(config);

    println!("Fetching markets from Gamma API...\n");
    let markets = scanner.fetch_markets().await?;
    println!("Found {} active markets with sufficient liquidity\n", markets.len());

    let mut opportunities = runner.find_all_opportunities(&markets);

    // Filter out sports if requested
    if no_sports {
        opportunities.sniper = opportunities.sniper.into_iter()
            .filter(|o| !is_sports_market(&o.question, o.category.as_deref()))
            .collect();
    }

    // Display sniper opportunities
    print_sniper_opportunities(&opportunities.sniper, limit);

    // Display NO bias opportunities
    print_no_bias_opportunities(&opportunities.no_bias, limit);

    // Summary
    println!("\n{}", "-".repeat(70));
    println!(
        "Total: {} sniper + {} bias = {} opportunities",
        opportunities.sniper.len(),
        opportunities.no_bias.len(),
        opportunities.total_count()
    );

    // Save to database
    let db = Database::new(&config.database_path).await?;
    db.record_scan(
        markets.len() as i64,
        opportunities.sniper.len() as i64,
        opportunities.no_bias.len() as i64,
    )
    .await?;

    Ok(())
}

async fn snipe_markets(config: &Config, max_hours: f64, limit: usize, no_sports: bool) -> Result<()> {
    println!("\n{}", "=".repeat(70));
    println!("  SNIPER MODE - Markets closing within {:.1} hours | Sports: {}",
        max_hours,
        if no_sports { "EXCLUDED" } else { "INCLUDED" });
    println!("{}\n", "=".repeat(70));

    let scanner = Scanner::new(config.clone());
    let markets = scanner.fetch_closing_soon(max_hours).await?;

    println!("Found {} markets closing soon\n", markets.len());

    let runner = StrategyRunner::new(config);
    let mut opportunities = runner.sniper.find_opportunities(&markets);

    // Filter out sports if requested
    if no_sports {
        opportunities = opportunities.into_iter()
            .filter(|o| !is_sports_market(&o.question, o.category.as_deref()))
            .collect();
        println!("After filtering sports: {} opportunities\n", opportunities.len());
    }

    print_sniper_opportunities(&opportunities, limit);

    Ok(())
}

/// Check if a market is sports-related based on question text and category
fn is_sports_market(question: &str, category: Option<&str>) -> bool {
    let text = question.to_lowercase();

    // Check category first
    if let Some(cat) = category {
        let cat_lower = cat.to_lowercase();
        if cat_lower.contains("sport") || cat_lower.contains("nba") || cat_lower.contains("nfl")
            || cat_lower.contains("mlb") || cat_lower.contains("soccer") || cat_lower.contains("football") {
            return true;
        }
    }

    // Sports keywords
    let keywords = [
        "win on 2026", "win on 2025", " vs ", " vs. ", "match", "game",
        "nba", "nfl", "mlb", "nhl", "mls", "premier league", "la liga",
        "bundesliga", "serie a", "ligue 1", "champions league", "playoffs",
        "eredivisie", "liga portugal", "saudi pro", "super bowl",
        "lakers", "celtics", "warriors", "yankees", "dodgers",
        "soccer", "football", "basketball", "baseball", "hockey",
        "tennis", "golf", "formula 1", "f1", "ufc", "boxing", "mma",
        "esports", "call of duty", "league of legends",
        "feyenoord", "psv", "ajax", "barcelona", "real madrid",
        "manchester", "liverpool", "arsenal", "chelsea",
        "end in a draw", "will .* win on",
        "spread:", "over/under", "total goals", "total points",
        "marseille", "river plate", "rosario", "racing club", "boca juniors",
        "olimpia", "flamengo", "palmeiras", "corinthians", "santos",
        "ligaprofesional", "ligue1", "laliga", "seriea",
    ];

    for keyword in keywords {
        if text.contains(keyword) {
            return true;
        }
    }

    false
}

async fn bias_markets(config: &Config, min_edge: f64, limit: usize) -> Result<()> {
    println!("\n{}", "=".repeat(70));
    println!("  NO BIAS MODE - Minimum edge: {:.1}%", min_edge);
    println!("{}\n", "=".repeat(70));

    let scanner = Scanner::new(config.clone());
    let markets = scanner.fetch_markets().await?;

    let mut runner = StrategyRunner::new(config);
    runner.no_bias = polymarket_bot::strategies::NoBiasStrategy::new(
        polymarket_bot::config::NoBiasConfig {
            min_edge: min_edge / 100.0,
            ..Default::default()
        },
    );

    let opportunities = runner.no_bias.find_opportunities(&markets);
    print_no_bias_opportunities(&opportunities, limit);

    Ok(())
}

async fn run_bot(config: &Config, interval: u64, auto_execute: Option<f64>) -> Result<()> {
    println!("\n{}", "=".repeat(70));
    println!("  CONTINUOUS MODE");
    println!("  Interval: {}s | Auto-execute: {:?}", interval, auto_execute);
    println!("  Paper Trading: {}", if config.paper_trading { "YES" } else { "NO - LIVE MODE" });
    if config.discord_webhook_url.is_some() {
        println!("  Discord Webhook: ENABLED (sniper alerts)");
    }
    println!("{}\n", "=".repeat(70));

    let scanner = Scanner::new(config.clone());
    let runner = StrategyRunner::new(config);
    let db = Database::new(&config.database_path).await?;
    let executor = Executor::new(config.clone(), db);

    // Set up Discord webhook if configured
    let discord = config.discord_webhook_url.as_ref().map(|url| DiscordWebhook::new(url.clone()));

    // Track which opportunities we've already sent to avoid duplicates
    let mut sent_alerts: HashSet<String> = HashSet::new();

    println!("Starting continuous scan loop (Ctrl+C to stop)...\n");

    loop {
        match scanner.fetch_markets().await {
            Ok(markets) => {
                let opportunities = runner.find_all_opportunities(&markets);

                if !opportunities.is_empty() {
                    println!("\n--- Scan at {} ---", chrono::Utc::now().format("%H:%M:%S"));

                    for opp in opportunities.sniper.iter().take(3) {
                        println!("  [SNIPER] {}", opp.recommendation);

                        // Send Discord alert for new sniper opportunities
                        if let Some(ref webhook) = discord {
                            if !sent_alerts.contains(&opp.market_id) {
                                webhook.send_sniper_alert(opp).await;
                                sent_alerts.insert(opp.market_id.clone());
                            }
                        }

                        if let Some(threshold) = auto_execute {
                            if opp.edge >= threshold / 100.0 {
                                match executor.execute(opp).await {
                                    Ok(result) => info!("Execution result: {:?}", result),
                                    Err(e) => error!("Execution failed: {}", e),
                                }
                            }
                        }
                    }

                    for opp in opportunities.no_bias.iter().take(3) {
                        println!("  [BIAS]   {}", opp.recommendation);
                    }
                }

                // Show exposure status
                match executor.check_exposure().await {
                    Ok(status) => println!("  {}", status),
                    Err(e) => error!("Failed to check exposure: {}", e),
                }
            }
            Err(e) => {
                error!("Scan failed: {}", e);
            }
        }

        // Clean up old alerts periodically (keep last 1000)
        if sent_alerts.len() > 1000 {
            sent_alerts.clear();
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

async fn show_stats(config: &Config) -> Result<()> {
    let db = Database::new(&config.database_path).await?;
    let stats = db.get_stats().await?;

    println!("\n{}", "=".repeat(70));
    println!("  BOT STATISTICS");
    println!("{}\n", "=".repeat(70));

    println!("Overall Performance:");
    println!("  Total Trades:    {}", stats.total_trades);
    println!("  Winning Trades:  {}", stats.winning_trades);
    println!("  Losing Trades:   {}", stats.losing_trades);
    println!("  Win Rate:        {:.1}%", stats.win_rate());
    println!("  Total PnL:       ${:.2}", stats.total_pnl);

    println!("\nBy Strategy:");
    println!("  Sniper:");
    println!("    Trades: {} | Wins: {} | Win Rate: {:.1}%",
        stats.sniper_trades, stats.sniper_wins, stats.sniper_win_rate());
    println!("  NO Bias:");
    println!("    Trades: {} | Wins: {} | Win Rate: {:.1}%",
        stats.no_bias_trades, stats.no_bias_wins, stats.no_bias_win_rate());

    // Show open positions
    let positions = db.get_open_positions().await?;
    if !positions.is_empty() {
        println!("\nOpen Positions ({}):", positions.len());
        for pos in positions.iter().take(10) {
            println!("  {} {} @ {} - {}",
                pos.side, pos.entry_price, pos.size,
                if pos.question.len() > 40 {
                    format!("{}...", &pos.question[..40])
                } else {
                    pos.question.clone()
                }
            );
        }
    }

    Ok(())
}

fn print_sniper_opportunities(opportunities: &[polymarket_bot::Opportunity], limit: usize) {
    if opportunities.is_empty() {
        println!("No sniper opportunities found.\n");
        return;
    }

    println!("SNIPER OPPORTUNITIES (closing in 1-12 hours)");
    println!("{}", "-".repeat(70));

    for (i, opp) in opportunities.iter().take(limit).enumerate() {
        let no_bias_bonus = if matches!(opp.side, polymarket_bot::Side::No) {
            " [NO BIAS+]"
        } else {
            ""
        };

        println!("\n{}. \"{}\"", i + 1, opp.short_question(60));
        println!("   {} at {}c (favorite) | Liquidity: ${:.0}K",
            opp.side,
            opp.price_cents(),
            opp.liquidity.to_string().parse::<f64>().unwrap_or(0.0) / 1000.0
        );
        println!("   Return: {:.1}% | EV: {:.1}% | Time: {}{}",
            opp.return_percent(),
            opp.ev_percent(),
            opp.time_display(),
            no_bias_bonus
        );
        if let Some(src) = &opp.resolution_source {
            println!("   Source: {}", src);
        }
    }

    if opportunities.len() > limit {
        println!("\n   ... and {} more", opportunities.len() - limit);
    }

    println!();
}

fn print_no_bias_opportunities(opportunities: &[polymarket_bot::Opportunity], limit: usize) {
    if opportunities.is_empty() {
        println!("No NO bias opportunities found.\n");
        return;
    }

    println!("NO BIAS OPPORTUNITIES (longer dated)");
    println!("{}", "-".repeat(70));

    for (i, opp) in opportunities.iter().take(limit).enumerate() {
        println!("\n{}. \"{}\"", i + 1, opp.short_question(60));
        println!("   NO at {}c | Edge: {:.1}% vs 78.4% base rate",
            opp.price_cents(),
            opp.edge * 100.0
        );
        println!("   Liquidity: ${:.0}K | Time: {}",
            opp.liquidity.to_string().parse::<f64>().unwrap_or(0.0) / 1000.0,
            opp.time_display()
        );
        if let Some(cat) = &opp.category {
            println!("   Category: {}", cat);
        }
    }

    if opportunities.len() > limit {
        println!("\n   ... and {} more", opportunities.len() - limit);
    }

    println!();
}
