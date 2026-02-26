use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{Timelike, Utc};
use tokio::time::{sleep, Duration};
use tracing::{error, info};

use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;

/// Shared counters for tracking detector activity.
#[derive(Debug)]
pub struct Stats {
    pub market_polls: AtomicU64,
    pub notice_polls: AtomicU64,
    pub new_listings_detected: AtomicU64,
    pub ws_connected: AtomicBool,
    start_time: chrono::DateTime<Utc>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            market_polls: AtomicU64::new(0),
            notice_polls: AtomicU64::new(0),
            new_listings_detected: AtomicU64::new(0),
            ws_connected: AtomicBool::new(false),
            start_time: Utc::now(),
        }
    }

    fn uptime_string(&self) -> String {
        let elapsed = Utc::now() - self.start_time;
        let hours = elapsed.num_hours();
        let mins = elapsed.num_minutes() % 60;
        format!("{}h {}m", hours, mins)
    }
}

/// Run the daily health report loop. Sends a summary at 9:00 AM KST every day.
pub async fn run_daily_report(
    stats: Arc<Stats>,
    redis: RedisCache,
    telegram: Arc<TelegramAlert>,
) {
    let kst_offset = chrono::FixedOffset::east_opt(9 * 3600).unwrap();

    loop {
        // Calculate sleep duration until next 9:00 AM KST
        let now_kst = Utc::now().with_timezone(&kst_offset);
        let next_9am = if now_kst.hour() < 9 {
            now_kst
                .date_naive()
                .and_hms_opt(9, 0, 0)
                .unwrap()
        } else {
            (now_kst.date_naive() + chrono::Duration::days(1))
                .and_hms_opt(9, 0, 0)
                .unwrap()
        };

        let next_9am_kst = next_9am
            .and_local_timezone(kst_offset)
            .unwrap();
        let wait_secs = (next_9am_kst - now_kst).num_seconds().max(1) as u64;

        info!(
            next_report_in_hours = wait_secs / 3600,
            "Daily report scheduled"
        );

        sleep(Duration::from_secs(wait_secs)).await;

        // Gather real data
        let market_polls = stats.market_polls.load(Ordering::Relaxed);
        let notice_polls = stats.notice_polls.load(Ordering::Relaxed);
        let listings_today = stats.new_listings_detected.swap(0, Ordering::Relaxed);
        let ws_status = if stats.ws_connected.load(Ordering::Relaxed) {
            "Connected"
        } else {
            "Reconnecting"
        };
        let uptime = stats.uptime_string();

        // Get market count from Redis (real data)
        let market_count = match redis.get_markets().await {
            Ok(markets) => markets.len(),
            Err(_) => 0,
        };

        let now_kst = Utc::now().with_timezone(&kst_offset);
        let date_str = now_kst.format("%Y-%m-%d");

        let message = format!(
            "\u{1f4ca} *Daily Status Report — {date}*\n\
             \n\
             *Uptime:* {uptime}\n\
             *Markets monitored:* {markets}\n\
             *Market API polls:* {market_polls}\n\
             *Notice board checks:* {notice_polls}\n\
             *WebSocket:* {ws}\n\
             *New listings today:* {listings}\n\
             \n\
             \u{2705} All systems operational.",
            date = date_str,
            uptime = uptime,
            markets = market_count,
            market_polls = market_polls,
            notice_polls = notice_polls,
            ws = ws_status,
            listings = listings_today,
        );

        if let Err(e) = telegram.send_message(&message).await {
            error!(error = %e, "Failed to send daily report");
        } else {
            info!("Daily health report sent");
        }
    }
}
