mod alerts;
mod cache;
mod config;
mod detectors;
mod filters;
mod stats;
mod trading;

use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{error, info};

use crate::alerts::discord::DiscordAlert;
use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::Config;
use crate::trading::bybit::BybitExchange;
use crate::trading::binance::BinanceExchange;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(false)
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting Upbit Listing Detector"
    );

    // Load configuration
    let config = Arc::new(Config::load().context("Failed to load configuration")?);
    info!("Configuration loaded successfully");

    // Build a shared reqwest client (connection pooling + reuse)
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .pool_max_idle_per_host(5)
        .user_agent("upbit-listing-detector/0.1")
        .build()
        .context("Failed to build HTTP client")?;

    // Connect to Redis
    let redis = RedisCache::new(&config.redis.url, &config.redis.key_prefix)
        .await
        .context("Failed to connect to Redis")?;

    // Verify Redis connectivity
    redis.ping().await.context("Redis ping failed")?;
    info!("Redis connection verified");

    // Build alert senders
    let telegram = Arc::new(TelegramAlert::new(
        client.clone(),
        &config.telegram.bot_token,
        &config.telegram.chat_id,
    ));

    let discord: Option<Arc<DiscordAlert>> = config.discord.as_ref().map(|d| {
        Arc::new(DiscordAlert::new(client.clone(), &d.webhook_url))
    });

    // Send startup notification
    if let Err(e) = telegram
        .send_message("\u{2705} *Upbit Listing Detector started*\nAll systems operational.")
        .await
    {
        error!(error = %e, "Failed to send startup notification (non-fatal)");
    }

    // Shared stats for daily health report
    let stats = Arc::new(stats::Stats::new());

    // Create trade signal channel (mpsc)
    let (trade_tx, trade_rx) = tokio::sync::mpsc::channel(64);

    // Build exchange clients
    let bybit_cfg = &config.trading.bybit;
    let bybit_url = if bybit_cfg.base_url.is_empty() {
        if bybit_cfg.testnet {
            "https://api-testnet.bybit.com"
        } else {
            "https://api.bybit.com"
        }
    } else {
        &bybit_cfg.base_url
    };
    let bybit = Arc::new(BybitExchange::new(
        client.clone(),
        bybit_url,
        &bybit_cfg.api_key,
        &bybit_cfg.api_secret,
    ));

    let binance_cfg = &config.trading.binance;
    let binance_url = if binance_cfg.base_url.is_empty() {
        if binance_cfg.testnet {
            "https://testnet.binancefuture.com"
        } else {
            "https://fapi.binance.com"
        }
    } else {
        &binance_cfg.base_url
    };
    let binance = Arc::new(BinanceExchange::new(
        client.clone(),
        binance_url,
        &binance_cfg.api_key,
        &binance_cfg.api_secret,
    ));

    if config.trading.enabled {
        info!(
            bybit_url = bybit_url,
            binance_url = binance_url,
            leverage = config.trading.leverage,
            size_usd = config.trading.position_size_usd,
            "Trading enabled"
        );
    } else {
        info!("Trading disabled (set trading.enabled = true to activate)");
    }

    info!("All components initialized. Starting detection loops.");

    // Set up graceful shutdown
    let shutdown = tokio::signal::ctrl_c();

    // Run all detectors + executor concurrently.
    // tokio::select! returns when ANY branch completes (or errors).
    tokio::select! {
        result = detectors::market_api::run(
            config.clone(),
            redis.clone(),
            client.clone(),
            telegram.clone(),
            discord.clone(),
            stats.clone(),
            trade_tx.clone(),
        ) => {
            error!(error = ?result, "Market API detector exited");
        }

        result = detectors::websocket::run(
            config.clone(),
            redis.clone(),
            client.clone(),
            telegram.clone(),
            discord.clone(),
            stats.clone(),
            trade_tx.clone(),
        ) => {
            error!(error = ?result, "WebSocket monitor exited");
        }

        result = detectors::notice_api::run(
            config.clone(),
            redis.clone(),
            client.clone(),
            telegram.clone(),
            discord.clone(),
            stats.clone(),
            trade_tx.clone(),
        ) => {
            error!(error = ?result, "Notice detector exited");
        }

        result = trading::executor::run(
            trade_rx,
            config.trading.clone(),
            bybit,
            binance,
            redis.clone(),
            telegram.clone(),
        ) => {
            error!(error = ?result, "Trade executor exited");
        }

        _ = stats::run_daily_report(
            stats.clone(),
            redis.clone(),
            telegram.clone(),
        ) => {
            error!("Daily report loop exited");
        }

        _ = shutdown => {
            info!("Received shutdown signal, exiting gracefully");
        }
    }

    // Send shutdown notification (best-effort)
    let _ = telegram
        .send_message("\u{1f6d1} *Upbit Listing Detector stopped*")
        .await;

    info!("Shutdown complete");
    Ok(())
}
