use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub api: ApiConfig,
    pub polling: PollingConfig,
    pub redis: RedisConfig,
    pub telegram: TelegramConfig,
    pub discord: Option<DiscordConfig>,
    pub filters: FilterConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub market_endpoint: String,
    pub websocket_endpoint: String,
    pub notice_endpoint: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PollingConfig {
    pub market_interval_seconds: u64,
    pub notice_interval_seconds: u64,
    pub websocket_reconnect_delay_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    pub url: String,
    pub key_prefix: String,
    pub connection_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DiscordConfig {
    pub webhook_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilterConfig {
    pub min_confidence: f32,
}

impl Config {
    /// Load configuration from config.toml and environment variables.
    /// Environment variables override file values for secrets.
    pub fn load() -> Result<Self> {
        // Load .env file if present (non-fatal if missing)
        let _ = dotenvy::dotenv();

        let config_path = Self::find_config_file();
        let mut config: Config = if let Some(path) = config_path {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config file: {}", path.display()))?;
            toml::from_str(&contents).context("Failed to parse config.toml")?
        } else {
            Self::default_config()
        };

        // Override with environment variables where set
        if let Ok(val) = std::env::var("UPBIT_MARKET_API") {
            config.api.market_endpoint = val;
        }
        if let Ok(val) = std::env::var("UPBIT_WS_URL") {
            config.api.websocket_endpoint = val;
        }
        if let Ok(val) = std::env::var("UPBIT_NOTICE_API") {
            config.api.notice_endpoint = val;
        }
        if let Ok(val) = std::env::var("REDIS_URL") {
            config.redis.url = val;
        }
        if let Ok(val) = std::env::var("TELEGRAM_BOT_TOKEN") {
            config.telegram.bot_token = val;
        }
        if let Ok(val) = std::env::var("TELEGRAM_CHAT_ID") {
            config.telegram.chat_id = val;
        }
        if let Ok(val) = std::env::var("DISCORD_WEBHOOK_URL") {
            config.discord = Some(DiscordConfig { webhook_url: val });
        }

        config.validate()?;
        Ok(config)
    }

    fn find_config_file() -> Option<std::path::PathBuf> {
        let candidates = ["config.toml", "/etc/upbit-detector/config.toml"];
        for path in &candidates {
            let p = Path::new(path);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
        None
    }

    fn default_config() -> Self {
        Config {
            api: ApiConfig {
                market_endpoint: "https://api.upbit.com/v1/market/all".to_string(),
                websocket_endpoint: "wss://api.upbit.com/websocket/v1".to_string(),
                notice_endpoint: String::new(),
            },
            polling: PollingConfig {
                market_interval_seconds: 2,
                notice_interval_seconds: 3,
                websocket_reconnect_delay_seconds: 5,
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1:6379".to_string(),
                key_prefix: "upbit:".to_string(),
                connection_timeout_seconds: 5,
            },
            telegram: TelegramConfig {
                bot_token: String::new(),
                chat_id: String::new(),
            },
            discord: None,
            filters: FilterConfig {
                min_confidence: 0.6,
            },
        }
    }

    fn validate(&self) -> Result<()> {
        if self.telegram.bot_token.is_empty() {
            anyhow::bail!("TELEGRAM_BOT_TOKEN must be set (via config.toml or environment variable)");
        }
        if self.telegram.chat_id.is_empty() {
            anyhow::bail!("TELEGRAM_CHAT_ID must be set (via config.toml or environment variable)");
        }
        if self.polling.market_interval_seconds == 0 {
            anyhow::bail!("market_interval_seconds must be > 0");
        }
        if self.polling.notice_interval_seconds == 0 {
            anyhow::bail!("notice_interval_seconds must be > 0");
        }
        Ok(())
    }
}
