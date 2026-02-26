use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::alerts::telegram::TelegramAlert;
use crate::alerts::discord::DiscordAlert;
use crate::cache::redis::RedisCache;
use crate::config::Config;
use crate::stats::Stats;

/// Market as returned by the Upbit market/all endpoint.
#[derive(Debug, Deserialize, Clone)]
pub struct Market {
    pub market: String,
    pub korean_name: String,
    pub english_name: String,
}

/// Run the market API polling loop forever.
///
/// Polls `GET /v1/market/all` at the configured interval, compares the
/// returned set of market codes against the cached set in Redis, and fires
/// an alert for every newly-seen KRW-* code.
pub async fn run(
    config: Arc<Config>,
    redis: RedisCache,
    client: Client,
    telegram: Arc<TelegramAlert>,
    discord: Option<Arc<DiscordAlert>>,
    stats: Arc<Stats>,
) -> Result<()> {
    let interval = Duration::from_secs(config.polling.market_interval_seconds);
    let url = &config.api.market_endpoint;

    info!(url = url, interval_s = interval.as_secs(), "Market API detector starting");

    // Initial seed: populate Redis with current markets so we don't
    // alert on every existing pair at startup.
    match fetch_markets(&client, url).await {
        Ok(markets) => {
            let codes: HashSet<String> = markets.iter().map(|m| m.market.clone()).collect();
            info!(count = codes.len(), "Seeding initial market set");
            redis.set_markets(&codes).await?;
        }
        Err(e) => {
            warn!(error = %e, "Failed to seed markets on startup; will detect all on first poll");
        }
    }

    loop {
        sleep_with_jitter(interval).await;

        stats.market_polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match fetch_markets(&client, url).await {
            Ok(markets) => {
                if let Err(e) = process_markets(&markets, &redis, &telegram, discord.as_deref(), &stats).await {
                    error!(error = %e, "Error processing markets");
                }
            }
            Err(e) => {
                error!(error = %e, "Market API request failed");
            }
        }
    }
}

/// Fetch the full market list with retry + exponential backoff.
async fn fetch_markets(client: &Client, url: &str) -> Result<Vec<Market>> {
    let mut delay = Duration::from_secs(1);
    let max_retries = 3u32;

    for attempt in 0..max_retries {
        match client.get(url).send().await {
            Ok(resp) => {
                let markets: Vec<Market> = resp
                    .json()
                    .await
                    .context("Failed to parse market JSON")?;
                debug!(count = markets.len(), "Fetched markets");
                return Ok(markets);
            }
            Err(e) if attempt < max_retries - 1 => {
                warn!(
                    attempt = attempt + 1,
                    error = %e,
                    retry_in_ms = delay.as_millis() as u64,
                    "Market API request failed, retrying"
                );
                sleep(delay).await;
                delay *= 2;
            }
            Err(e) => {
                return Err(e).context("Market API request failed after all retries");
            }
        }
    }
    unreachable!()
}

/// Compare current markets to cached set, alert on new KRW-* entries.
async fn process_markets(
    markets: &[Market],
    redis: &RedisCache,
    telegram: &TelegramAlert,
    discord: Option<&DiscordAlert>,
    stats: &Stats,
) -> Result<()> {
    let cached = redis.get_markets().await?;
    if cached.is_empty() {
        // First run after a cache wipe; seed and return
        let codes: HashSet<String> = markets.iter().map(|m| m.market.clone()).collect();
        redis.set_markets(&codes).await?;
        return Ok(());
    }

    let current: HashSet<String> = markets.iter().map(|m| m.market.clone()).collect();
    let new_codes: Vec<&Market> = markets
        .iter()
        .filter(|m| !cached.contains(&m.market))
        .collect();

    if new_codes.is_empty() {
        return Ok(());
    }

    for market in &new_codes {
        let is_krw = market.market.starts_with("KRW-");
        if is_krw {
            info!(
                market = %market.market,
                korean = %market.korean_name,
                english = %market.english_name,
                "🚨 NEW KRW LISTING DETECTED via Market API"
            );
        } else {
            info!(
                market = %market.market,
                korean = %market.korean_name,
                english = %market.english_name,
                "New market detected (non-KRW)"
            );
        }

        // Send alerts for KRW markets (highest priority)
        if is_krw {
            stats.new_listings_detected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Err(e) = telegram
                .send_new_market_alert(&market.market, &market.korean_name, &market.english_name)
                .await
            {
                error!(error = %e, market = %market.market, "Failed to send Telegram alert");
            }

            if let Some(discord) = discord {
                if let Err(e) = discord
                    .send_new_market_alert(&market.market, &market.korean_name, &market.english_name)
                    .await
                {
                    error!(error = %e, market = %market.market, "Failed to send Discord alert");
                }
            }
        }
    }

    // Update cached set
    redis.set_markets(&current).await?;
    Ok(())
}

/// Sleep for `base` duration plus 0–500 ms of random jitter.
async fn sleep_with_jitter(base: Duration) {
    let jitter_ms = rand::random::<u64>() % 500;
    sleep(base + Duration::from_millis(jitter_ms)).await;
}
