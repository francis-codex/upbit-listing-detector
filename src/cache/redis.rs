use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::collections::HashSet;
use tracing::{debug, error, info};

/// Redis-backed state cache for market codes, notice IDs, and alert history.
pub struct RedisCache {
    conn: ConnectionManager,
    prefix: String,
}

impl RedisCache {
    pub async fn new(url: &str, prefix: &str) -> Result<Self> {
        let client = redis::Client::open(url)
            .with_context(|| format!("Invalid Redis URL: {url}"))?;

        let conn = ConnectionManager::new(client)
            .await
            .context("Failed to connect to Redis")?;

        info!("Connected to Redis at {url}");

        Ok(Self {
            conn,
            prefix: prefix.to_string(),
        })
    }

    fn key(&self, suffix: &str) -> String {
        format!("{}{}", self.prefix, suffix)
    }

    // ── Market codes ──────────────────────────────────────────────────

    /// Retrieve the set of known market codes from Redis.
    pub async fn get_markets(&self) -> Result<HashSet<String>> {
        let key = self.key("markets");
        let members: Vec<String> = self
            .conn
            .clone()
            .smembers(&key)
            .await
            .context("Redis SMEMBERS failed for markets")?;
        debug!(count = members.len(), "Loaded markets from Redis");
        Ok(members.into_iter().collect())
    }

    /// Replace the entire set of known market codes.
    pub async fn set_markets(&self, markets: &HashSet<String>) -> Result<()> {
        let key = self.key("markets");
        let mut conn = self.conn.clone();

        // Use a pipeline: delete old set, add all new members
        let mut pipe = redis::pipe();
        pipe.del(&key);
        if !markets.is_empty() {
            let members: Vec<&str> = markets.iter().map(|s| s.as_str()).collect();
            pipe.sadd(&key, members);
        }
        pipe.query_async::<()>(&mut conn)
            .await
            .context("Redis pipeline failed for set_markets")?;

        debug!(count = markets.len(), "Stored markets in Redis");
        Ok(())
    }

    /// Add a single market code to the known set.
    pub async fn add_market(&self, code: &str) -> Result<()> {
        let key = self.key("markets");
        self.conn
            .clone()
            .sadd::<_, _, ()>(&key, code)
            .await
            .context("Redis SADD failed for market")?;
        Ok(())
    }

    // ── Notice IDs ────────────────────────────────────────────────────

    /// Check whether a notice ID has already been seen.
    pub async fn is_notice_seen(&self, id: &str) -> Result<bool> {
        let key = self.key("notices");
        let seen: bool = self
            .conn
            .clone()
            .sismember(&key, id)
            .await
            .context("Redis SISMEMBER failed for notices")?;
        Ok(seen)
    }

    /// Mark a notice ID as seen.
    pub async fn mark_notice_seen(&self, id: &str) -> Result<()> {
        let key = self.key("notices");
        self.conn
            .clone()
            .sadd::<_, _, ()>(&key, id)
            .await
            .context("Redis SADD failed for notice")?;
        debug!(notice_id = id, "Marked notice as seen");
        Ok(())
    }

    // ── Alert deduplication ───────────────────────────────────────────

    /// Record the timestamp of the last alert for a given token symbol,
    /// returning true if an alert was already sent within `cooldown_secs`.
    pub async fn is_alert_recent(&self, token: &str, cooldown_secs: u64) -> Result<bool> {
        let key = self.key(&format!("alert:{token}"));
        let ts: Option<i64> = self
            .conn
            .clone()
            .get(&key)
            .await
            .context("Redis GET failed for alert timestamp")?;

        if let Some(ts) = ts {
            let now = chrono::Utc::now().timestamp();
            return Ok((now - ts) < cooldown_secs as i64);
        }
        Ok(false)
    }

    /// Record that an alert was just sent for a token.
    pub async fn record_alert(&self, token: &str) -> Result<()> {
        let key = self.key(&format!("alert:{token}"));
        let now = chrono::Utc::now().timestamp();
        self.conn
            .clone()
            .set_ex::<_, _, ()>(&key, now, 3600) // expire after 1 hour
            .await
            .context("Redis SETEX failed for alert timestamp")?;
        Ok(())
    }

    /// Health check – ping Redis.
    pub async fn ping(&self) -> Result<()> {
        redis::cmd("PING")
            .query_async::<String>(&mut self.conn.clone())
            .await
            .context("Redis PING failed")?;
        Ok(())
    }
}

impl Clone for RedisCache {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            prefix: self.prefix.clone(),
        }
    }
}
