use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::alerts::discord::DiscordAlert;
use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::Config;
use crate::filters::keywords;
use crate::filters::parser;

/// A single notice from the Upbit notice board.
///
/// The actual JSON structure depends on the reverse-engineered endpoint.
/// Adjust fields as needed once the real API shape is discovered.
#[derive(Debug, Deserialize, Clone)]
pub struct Notice {
    /// Unique notice ID (numeric or string depending on endpoint).
    #[serde(alias = "id", alias = "noticeId")]
    pub id: serde_json::Value,

    /// Notice title text (Korean).
    #[serde(alias = "title", alias = "subject")]
    pub title: String,

    /// Full URL to the notice detail page (may need construction).
    #[serde(default)]
    pub url: Option<String>,

    /// Publication date string.
    #[serde(default, alias = "createdAt", alias = "created_at", alias = "date")]
    pub created_at: Option<String>,
}

impl Notice {
    fn id_string(&self) -> String {
        match &self.id {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }
}

/// Wrapper for the notice API response.
/// Handles both `{ "data": { "list": [...] } }` and bare `[...]` shapes.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NoticeResponse {
    Wrapped {
        data: NoticeData,
    },
    WrappedSimple {
        #[serde(alias = "notices", alias = "list")]
        list: Vec<Notice>,
    },
    Bare(Vec<Notice>),
}

#[derive(Debug, Deserialize)]
struct NoticeData {
    #[serde(alias = "notices", alias = "list")]
    list: Vec<Notice>,
}

impl NoticeResponse {
    fn into_notices(self) -> Vec<Notice> {
        match self {
            NoticeResponse::Wrapped { data } => data.list,
            NoticeResponse::WrappedSimple { list } => list,
            NoticeResponse::Bare(v) => v,
        }
    }
}

/// Run the notice board scraping loop forever.
///
/// Polls the configured notice endpoint at the configured interval,
/// filters new notices through the keyword system, and sends alerts
/// for likely listing announcements.
pub async fn run(
    config: Arc<Config>,
    redis: RedisCache,
    client: Client,
    telegram: Arc<TelegramAlert>,
    discord: Option<Arc<DiscordAlert>>,
) -> Result<()> {
    let endpoint = &config.api.notice_endpoint;
    if endpoint.is_empty() {
        warn!(
            "Notice endpoint is not configured. \
             Set UPBIT_NOTICE_API or api.notice_endpoint in config.toml. \
             Skipping notice detection."
        );
        // Sleep forever so tokio::select! doesn't immediately return
        loop {
            sleep(Duration::from_secs(3600)).await;
        }
    }

    let interval = Duration::from_secs(config.polling.notice_interval_seconds);
    let min_confidence = config.filters.min_confidence;

    info!(
        url = endpoint,
        interval_s = interval.as_secs(),
        "Notice board detector starting"
    );

    // Seed: mark existing notices as seen so we don't fire on old posts.
    if let Ok(notices) = fetch_notices(&client, endpoint).await {
        for notice in &notices {
            let _ = redis.mark_notice_seen(&notice.id_string()).await;
        }
        info!(count = notices.len(), "Seeded existing notices");
    }

    loop {
        sleep(interval).await;

        match fetch_notices(&client, endpoint).await {
            Ok(notices) => {
                for notice in notices {
                    if let Err(e) = process_notice(
                        &notice,
                        &redis,
                        &telegram,
                        discord.as_deref(),
                        min_confidence,
                    )
                    .await
                    {
                        error!(error = %e, id = %notice.id_string(), "Error processing notice");
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Notice API request failed");
            }
        }
    }
}

/// Fetch notices from the reverse-engineered API endpoint.
async fn fetch_notices(client: &Client, url: &str) -> Result<Vec<Notice>> {
    let mut delay = Duration::from_secs(1);
    let max_retries = 3u32;

    for attempt in 0..max_retries {
        match client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .header("Accept", "application/json")
            .header("Referer", "https://upbit.com/service_center/notice")
            .send()
            .await
        {
            Ok(resp) => {
                let text = resp.text().await.context("Failed to read notice response body")?;
                let parsed: NoticeResponse =
                    serde_json::from_str(&text).context("Failed to parse notice JSON")?;
                let notices = parsed.into_notices();
                debug!(count = notices.len(), "Fetched notices");
                return Ok(notices);
            }
            Err(e) if attempt < max_retries - 1 => {
                warn!(
                    attempt = attempt + 1,
                    error = %e,
                    "Notice API request failed, retrying"
                );
                sleep(delay).await;
                delay *= 2;
            }
            Err(e) => {
                return Err(e).context("Notice API request failed after all retries");
            }
        }
    }
    unreachable!()
}

/// Process a single notice: deduplicate, filter, alert.
async fn process_notice(
    notice: &Notice,
    redis: &RedisCache,
    telegram: &TelegramAlert,
    discord: Option<&DiscordAlert>,
    min_confidence: f32,
) -> Result<()> {
    let id = notice.id_string();

    // Deduplicate
    if redis.is_notice_seen(&id).await? {
        return Ok(());
    }
    redis.mark_notice_seen(&id).await?;

    debug!(id = id, title = %notice.title, "Processing new notice");

    // Run keyword filter
    let filter_result = keywords::is_listing_announcement(&notice.title);

    if !filter_result.is_listing || filter_result.confidence < min_confidence {
        debug!(
            id = id,
            confidence = filter_result.confidence,
            "Notice rejected by filter"
        );
        return Ok(());
    }

    // Parse token info
    let listing_info = parser::parse_listing(&notice.title, filter_result.confidence);

    info!(
        id = id,
        title = %notice.title,
        confidence = filter_result.confidence,
        token = ?listing_info.as_ref().map(|l| &l.token_symbol),
        "🚨 LISTING ANNOUNCEMENT DETECTED via Notice Board"
    );

    let link = notice.url.as_deref();

    match listing_info {
        Some(info) => {
            if let Err(e) = telegram
                .send_listing_alert(&info, &notice.title, link, "Notice Board")
                .await
            {
                error!(error = %e, "Failed to send Telegram notice alert");
            }
            if let Some(discord) = discord {
                if let Err(e) = discord
                    .send_listing_alert(&info, &notice.title, link, "Notice Board")
                    .await
                {
                    error!(error = %e, "Failed to send Discord notice alert");
                }
            }
        }
        None => {
            // Couldn't parse token details but confidence is high enough;
            // send a generic alert with just the title.
            let generic = parser::ListingInfo {
                token_symbol: "UNKNOWN".to_string(),
                token_name: None,
                markets: vec![],
                trading_start_time: None,
                confidence: filter_result.confidence,
            };
            if let Err(e) = telegram
                .send_listing_alert(&generic, &notice.title, link, "Notice Board")
                .await
            {
                error!(error = %e, "Failed to send Telegram generic notice alert");
            }
        }
    }

    Ok(())
}
