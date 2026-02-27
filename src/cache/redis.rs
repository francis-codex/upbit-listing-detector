use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::collections::HashSet;
use tracing::{debug, error, info};

use crate::trading::position::OpenPosition;

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

    // ── Trade deduplication ─────────────────────────────────────────

    /// Check if a trade was already placed for this symbol recently (1h TTL).
    pub async fn is_trade_recent(&self, symbol: &str) -> Result<bool> {
        let key = self.key(&format!("trade:{symbol}"));
        let exists: bool = self
            .conn
            .clone()
            .exists(&key)
            .await
            .context("Redis EXISTS failed for trade key")?;
        Ok(exists)
    }

    /// Record that a trade was placed for a symbol (1h TTL).
    pub async fn record_trade(&self, symbol: &str) -> Result<()> {
        let key = self.key(&format!("trade:{symbol}"));
        let now = chrono::Utc::now().timestamp();
        self.conn
            .clone()
            .set_ex::<_, _, ()>(&key, now, 3600)
            .await
            .context("Redis SETEX failed for trade key")?;
        Ok(())
    }

    // ── Position persistence ──────────────────────────────────────────

    /// Save an open position to Redis (hash map keyed by position ID).
    pub async fn save_position(&self, position: &OpenPosition) -> Result<()> {
        let key = self.key("positions");
        let json = serde_json::to_string(position).context("Failed to serialize position")?;
        self.conn
            .clone()
            .hset::<_, _, _, ()>(&key, &position.id, &json)
            .await
            .context("Redis HSET failed for position")?;
        debug!(id = position.id, symbol = position.symbol, "Position saved");
        Ok(())
    }

    /// Remove a closed position from Redis.
    pub async fn remove_position(&self, position_id: &str) -> Result<()> {
        let key = self.key("positions");
        self.conn
            .clone()
            .hdel::<_, _, ()>(&key, position_id)
            .await
            .context("Redis HDEL failed for position")?;
        debug!(id = position_id, "Position removed");
        Ok(())
    }

    /// Get all open positions from Redis.
    pub async fn get_open_positions(&self) -> Result<Vec<OpenPosition>> {
        let key = self.key("positions");
        let entries: std::collections::HashMap<String, String> = self
            .conn
            .clone()
            .hgetall(&key)
            .await
            .context("Redis HGETALL failed for positions")?;

        let mut positions = Vec::new();
        for (_id, json) in entries {
            match serde_json::from_str::<OpenPosition>(&json) {
                Ok(p) => positions.push(p),
                Err(e) => {
                    error!(error = %e, "Failed to deserialize position from Redis");
                }
            }
        }
        Ok(positions)
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
