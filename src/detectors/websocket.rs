use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::alerts::discord::DiscordAlert;
use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::Config;
use crate::detectors::market_api::Market;

/// Run the WebSocket monitor loop forever.
///
/// Connects to the Upbit WebSocket, subscribes to ticker data for all
/// KRW markets, and watches for previously-unseen market codes appearing
/// in the stream. Auto-reconnects on disconnect.
pub async fn run(
    config: Arc<Config>,
    redis: RedisCache,
    client: Client,
    telegram: Arc<TelegramAlert>,
    discord: Option<Arc<DiscordAlert>>,
) -> Result<()> {
    let ws_url = &config.api.websocket_endpoint;
    let market_url = &config.api.market_endpoint;
    let reconnect_delay = Duration::from_secs(config.polling.websocket_reconnect_delay_seconds);

    info!(url = ws_url, "WebSocket monitor starting");

    loop {
        match connect_and_listen(
            ws_url,
            market_url,
            &client,
            &redis,
            &telegram,
            discord.as_deref(),
        )
        .await
        {
            Ok(()) => {
                warn!("WebSocket stream ended cleanly, reconnecting");
            }
            Err(e) => {
                error!(error = %e, "WebSocket error, reconnecting");
            }
        }
        sleep(reconnect_delay).await;
    }
}

async fn connect_and_listen(
    ws_url: &str,
    market_url: &str,
    client: &Client,
    redis: &RedisCache,
    telegram: &TelegramAlert,
    discord: Option<&DiscordAlert>,
) -> Result<()> {
    // Fetch current market codes to subscribe to
    let markets = fetch_krw_codes(client, market_url).await?;
    let codes: Vec<String> = markets.iter().map(|m| m.market.clone()).collect();

    if codes.is_empty() {
        warn!("No KRW markets found, skipping WebSocket connection");
        return Ok(());
    }

    info!(count = codes.len(), "Subscribing to KRW market tickers");

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("WebSocket connection failed")?;

    info!("WebSocket connected");

    let (mut write, mut read) = ws_stream.split();

    // Send subscription message
    let subscribe_msg = serde_json::json!([
        { "ticket": "upbit-listing-detector" },
        { "type": "ticker", "codes": codes },
        { "format": "DEFAULT" }
    ]);

    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .context("Failed to send WebSocket subscription")?;

    debug!("Subscription message sent");

    // Track known codes for this session
    let mut known_codes: HashSet<String> = codes.into_iter().collect();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                handle_text_message(&text, &mut known_codes, redis, telegram, discord).await;
            }
            Ok(Message::Binary(data)) => {
                // Upbit sends binary (gzip) messages for ticker data
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    handle_text_message(&text, &mut known_codes, redis, telegram, discord).await;
                }
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = write.send(Message::Pong(data)).await {
                    warn!(error = %e, "Failed to send pong");
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket closed by server");
                return Ok(());
            }
            Err(e) => {
                return Err(e).context("WebSocket read error");
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_text_message(
    text: &str,
    known_codes: &mut HashSet<String>,
    redis: &RedisCache,
    telegram: &TelegramAlert,
    discord: Option<&DiscordAlert>,
) {
    // Parse ticker message to extract market code
    let code = match extract_market_code(text) {
        Some(c) => c,
        None => return,
    };

    // If the code is new to this WebSocket session, check Redis
    if known_codes.contains(&code) {
        return;
    }

    // New code seen in stream that we didn't subscribe to (shouldn't happen normally)
    // or Upbit injected a new code. Check Redis.
    let cached = match redis.get_markets().await {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Redis error during WebSocket processing");
            return;
        }
    };

    if cached.contains(&code) {
        // Already known, just update local set
        known_codes.insert(code);
        return;
    }

    // Genuinely new market detected via WebSocket
    if code.starts_with("KRW-") {
        info!(market = %code, "🚨 NEW KRW LISTING DETECTED via WebSocket");

        let symbol = code.strip_prefix("KRW-").unwrap_or(&code);
        if let Err(e) = telegram
            .send_new_market_alert(&code, symbol, symbol)
            .await
        {
            error!(error = %e, "Failed to send Telegram alert from WebSocket");
        }

        if let Some(discord) = discord {
            if let Err(e) = discord
                .send_new_market_alert(&code, symbol, symbol)
                .await
            {
                error!(error = %e, "Failed to send Discord alert from WebSocket");
            }
        }
    }

    // Update Redis and local tracking
    if let Err(e) = redis.add_market(&code).await {
        error!(error = %e, "Failed to update Redis from WebSocket");
    }
    known_codes.insert(code);
}

/// Extract the market code (e.g., "KRW-BTC") from a WebSocket ticker message.
fn extract_market_code(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get("code")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

/// Fetch current KRW-* market codes from the REST API.
async fn fetch_krw_codes(client: &Client, url: &str) -> Result<Vec<Market>> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("Failed to fetch markets for WebSocket")?;

    let markets: Vec<Market> = resp
        .json()
        .await
        .context("Failed to parse markets for WebSocket")?;

    let krw: Vec<Market> = markets
        .into_iter()
        .filter(|m| m.market.starts_with("KRW-"))
        .collect();

    Ok(krw)
}
