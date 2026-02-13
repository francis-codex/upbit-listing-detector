use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{debug, error, info};

use crate::filters::parser::ListingInfo;

/// Discord webhook alert sender.
pub struct DiscordAlert {
    client: Client,
    webhook_url: String,
}

impl DiscordAlert {
    pub fn new(client: Client, webhook_url: &str) -> Self {
        Self {
            client,
            webhook_url: webhook_url.to_string(),
        }
    }

    /// Send a formatted listing alert via Discord webhook.
    pub async fn send_listing_alert(
        &self,
        info: &ListingInfo,
        title: &str,
        link: Option<&str>,
        source: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap());
        let timestamp = now.format("%Y-%m-%d %H:%M:%S KST");

        let markets_str = if info.markets.is_empty() {
            "Unknown".to_string()
        } else {
            info.markets.join(", ")
        };

        let confidence_pct = (info.confidence * 100.0) as u32;

        let trading_time = info
            .trading_start_time
            .as_deref()
            .unwrap_or("Not specified");

        let link_line = match link {
            Some(url) => format!("\n[View Notice]({url})"),
            None => String::new(),
        };

        let body = serde_json::json!({
            "embeds": [{
                "title": "\u{1f6a8} New Upbit Listing Detected",
                "color": 16711680,
                "fields": [
                    { "name": "Token", "value": info.token_symbol, "inline": true },
                    { "name": "Markets", "value": markets_str, "inline": true },
                    { "name": "Confidence", "value": format!("{confidence_pct}%"), "inline": true },
                    { "name": "Trading Starts", "value": trading_time, "inline": false },
                    { "name": "Title", "value": format!("{title}{link_line}"), "inline": false },
                    { "name": "Source", "value": source, "inline": true },
                ],
                "footer": { "text": format!("Detected at {timestamp}") },
            }]
        });

        let response = self
            .client
            .post(&self.webhook_url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Discord webhook")?;

        if response.status().is_success() {
            info!("Discord alert sent successfully");
        } else {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            error!(status = %status, body = body, "Discord webhook error");
            anyhow::bail!("Discord webhook error: {status} - {body}");
        }

        Ok(())
    }

    /// Send a new-market alert.
    pub async fn send_new_market_alert(
        &self,
        market_code: &str,
        korean_name: &str,
        english_name: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap());
        let timestamp = now.format("%Y-%m-%d %H:%M:%S KST");

        let body = serde_json::json!({
            "embeds": [{
                "title": "\u{1f6a8} New Market Detected on Upbit",
                "color": 16711680,
                "fields": [
                    { "name": "Market Code", "value": format!("`{market_code}`"), "inline": true },
                    { "name": "Korean Name", "value": korean_name, "inline": true },
                    { "name": "English Name", "value": english_name, "inline": true },
                ],
                "footer": { "text": format!("Market API | Detected at {timestamp}") },
            }]
        });

        let response = self
            .client
            .post(&self.webhook_url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Discord webhook")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            anyhow::bail!("Discord webhook error: {status} - {body}");
        }

        Ok(())
    }
}
