use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{debug, error, info};

use crate::filters::parser::ListingInfo;

/// Telegram alert sender using the Bot API.
#[derive(Clone)]
pub struct TelegramAlert {
    client: Client,
    bot_token: String,
    chat_id: String,
}

impl TelegramAlert {
    pub fn new(client: Client, bot_token: &str, chat_id: &str) -> Self {
        Self {
            client,
            bot_token: bot_token.to_string(),
            chat_id: chat_id.to_string(),
        }
    }

    /// Send a formatted listing alert to Telegram.
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

        let trading_time = info
            .trading_start_time
            .as_deref()
            .unwrap_or("Not specified");

        let confidence_pct = (info.confidence * 100.0) as u32;

        let link_line = match link {
            Some(url) => format!("\nLink: {url}"),
            None => String::new(),
        };

        let message = format!(
            "\u{1f6a8} *NEW UPBIT LISTING DETECTED*\n\
             \n\
             *Token:* {symbol}\n\
             *Markets:* {markets}\n\
             *Trading Starts:* {trading_time}\n\
             *Confidence:* {confidence_pct}%\n\
             \n\
             *Title:* {title}{link_line}\n\
             *Source:* {source}\n\
             \n\
             \u{23f0} Detected at: {timestamp}",
            symbol = info.token_symbol,
            markets = markets_str,
        );

        self.send_message(&message).await
    }

    /// Send a raw new-market alert (from market API detection).
    pub async fn send_new_market_alert(
        &self,
        market_code: &str,
        korean_name: &str,
        english_name: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap());
        let timestamp = now.format("%Y-%m-%d %H:%M:%S KST");

        let message = format!(
            "\u{1f6a8} *NEW MARKET DETECTED ON UPBIT*\n\
             \n\
             *Market Code:* `{market_code}`\n\
             *Korean Name:* {korean_name}\n\
             *English Name:* {english_name}\n\
             \n\
             *Source:* Market API\n\
             \u{23f0} Detected at: {timestamp}"
        );

        self.send_message(&message).await
    }

    /// Send a plain text message via Telegram Bot API.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );

        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "Markdown",
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Telegram message")?;

        if response.status().is_success() {
            info!("Telegram alert sent successfully");
            debug!(text = text, "Telegram message body");
        } else {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            error!(
                status = %status,
                body = body,
                "Telegram API returned error"
            );
            anyhow::bail!("Telegram API error: {status} - {body}");
        }

        Ok(())
    }
}
