use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub api: ApiConfig,
    pub polling: PollingConfig,
    pub redis: RedisConfig,
    pub telegram: TelegramConfig,
    pub discord: Option<DiscordConfig>,
    pub filters: FilterConfig,
    #[serde(default)]
    pub trading: TradingConfig,
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
    #[allow(dead_code)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct TradingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub users: Vec<UserConfig>,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            users: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserConfig {
    pub name: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    #[serde(default = "default_position_size")]
    pub position_size_usd: f64,
    #[serde(default = "default_leverage")]
    pub leverage: u32,
    #[serde(default = "default_max_positions")]
    pub max_open_positions: u32,
    #[serde(default)]
    pub take_profit: TakeProfitConfig,
    #[serde(default)]
    pub stop_loss: StopLossConfig,
    #[serde(default)]
    pub time_exit: TimeExitConfig,
    #[serde(default)]
    pub bybit: ExchangeCredentials,
    #[serde(default)]
    pub binance: ExchangeCredentials,
}

fn default_position_size() -> f64 { 50.0 }
fn default_leverage() -> u32 { 2 }
fn default_max_positions() -> u32 { 3 }

#[derive(Debug, Deserialize, Clone)]
pub struct TakeProfitConfig {
    #[serde(default = "default_tp_levels")]
    pub levels: Vec<TakeProfitLevel>,
}

impl Default for TakeProfitConfig {
    fn default() -> Self {
        Self { levels: default_tp_levels() }
    }
}

fn default_tp_levels() -> Vec<TakeProfitLevel> {
    vec![
        TakeProfitLevel { percent: 80.0, close_fraction: 0.5 },
        TakeProfitLevel { percent: 100.0, close_fraction: 1.0 },
    ]
}

#[derive(Debug, Deserialize, Clone)]
pub struct TakeProfitLevel {
    pub percent: f64,
    pub close_fraction: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StopLossConfig {
    #[serde(default = "default_sl_percent")]
    pub percent: f64,
}

impl Default for StopLossConfig {
    fn default() -> Self {
        Self { percent: default_sl_percent() }
    }
}

fn default_sl_percent() -> f64 { 15.0 }

#[derive(Debug, Deserialize, Clone)]
pub struct TimeExitConfig {
    #[serde(default = "default_time_exit_minutes")]
    pub minutes: u64,
}

impl Default for TimeExitConfig {
    fn default() -> Self {
        Self { minutes: default_time_exit_minutes() }
    }
}

fn default_time_exit_minutes() -> u64 { 30 }

#[derive(Debug, Deserialize, Clone)]
pub struct ExchangeCredentials {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default = "default_true")]
    pub testnet: bool,
    #[serde(default)]
    pub base_url: String,
}

fn default_true() -> bool { true }

impl Default for ExchangeCredentials {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            testnet: true,
            base_url: String::new(),
        }
    }
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

        // Per-user env var overrides: {UPPERNAME}_BYBIT_API_KEY, etc.
        for user in &mut config.trading.users {
            let prefix = user.name.to_uppercase();
            if let Ok(val) = std::env::var(format!("{prefix}_BYBIT_API_KEY")) {
                user.bybit.api_key = val;
            }
            if let Ok(val) = std::env::var(format!("{prefix}_BYBIT_API_SECRET")) {
                user.bybit.api_secret = val;
            }
            if let Ok(val) = std::env::var(format!("{prefix}_BINANCE_API_KEY")) {
                user.binance.api_key = val;
            }
            if let Ok(val) = std::env::var(format!("{prefix}_BINANCE_API_SECRET")) {
                user.binance.api_secret = val;
            }
            if let Ok(val) = std::env::var(format!("{prefix}_TELEGRAM_CHAT_ID")) {
                user.telegram_chat_id = val;
            }
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
            trading: TradingConfig::default(),
        }
    }

    /// Validate trading-specific rules only (for unit testing without full config).
    fn validate_trading(&self) -> Result<()> {
        if self.trading.enabled {
            if self.trading.users.is_empty() {
                anyhow::bail!("trading.enabled is true but no [[trading.users]] configured");
            }

            let mut names = HashSet::new();
            for user in &self.trading.users {
                if user.name.is_empty() {
                    anyhow::bail!("trading.users: each user must have a non-empty name");
                }
                if !names.insert(&user.name) {
                    anyhow::bail!("trading.users: duplicate user name '{}'", user.name);
                }
                if user.telegram_chat_id.is_empty() {
                    anyhow::bail!(
                        "trading.users['{}']: telegram_chat_id must be set (via config or {}_TELEGRAM_CHAT_ID env var)",
                        user.name,
                        user.name.to_uppercase(),
                    );
                }
                if user.bybit.api_key.is_empty() && user.binance.api_key.is_empty() {
                    anyhow::bail!(
                        "trading.users['{}']: at least one exchange (bybit or binance) must have API keys configured",
                        user.name,
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.telegram.bot_token.is_empty() {
            anyhow::bail!("TELEGRAM_BOT_TOKEN must be set (via config.toml or environment variable)");
        }
        if self.telegram.chat_id.is_empty() {
            anyhow::bail!("TELEGRAM_CHAT_ID must be set (via config.toml or environment variable)");
        }
        if self.polling.notice_interval_seconds == 0 {
            anyhow::bail!("notice_interval_seconds must be > 0");
        }

        self.validate_trading()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Config with valid non-trading fields so validate() doesn't
    /// fail on telegram/polling checks. Only trading fields vary per test.
    fn base_config(trading: TradingConfig) -> Config {
        Config {
            api: ApiConfig {
                market_endpoint: "https://example.com".into(),
                websocket_endpoint: "wss://example.com".into(),
                notice_endpoint: String::new(),
            },
            polling: PollingConfig {
                market_interval_seconds: 2,
                notice_interval_seconds: 3,
                websocket_reconnect_delay_seconds: 5,
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1".into(),
                key_prefix: "test:".into(),
                connection_timeout_seconds: 5,
            },
            telegram: TelegramConfig {
                bot_token: "tok".into(),
                chat_id: "123".into(),
            },
            discord: None,
            filters: FilterConfig { min_confidence: 0.6 },
            trading,
        }
    }

    fn valid_user(name: &str) -> UserConfig {
        UserConfig {
            name: name.into(),
            telegram_chat_id: "999".into(),
            position_size_usd: 50.0,
            leverage: 2,
            max_open_positions: 3,
            take_profit: TakeProfitConfig::default(),
            stop_loss: StopLossConfig::default(),
            time_exit: TimeExitConfig::default(),
            bybit: ExchangeCredentials {
                api_key: "key".into(),
                api_secret: "secret".into(),
                ..Default::default()
            },
            binance: ExchangeCredentials::default(),
        }
    }

    // ── Parse multi-user TOML ────────────────────────────────────────

    #[test]
    fn parse_multi_user_toml() {
        let toml_str = r#"
            enabled = true
            [[users]]
            name = "alice"
            telegram_chat_id = "111"
            [users.bybit]
            api_key = "akey"
            api_secret = "asec"

            [[users]]
            name = "bob"
            telegram_chat_id = "222"
            leverage = 5
            position_size_usd = 100.0
            [users.binance]
            api_key = "bkey"
            api_secret = "bsec"
        "#;

        let trading: TradingConfig = toml::from_str(toml_str).unwrap();
        assert!(trading.enabled);
        assert_eq!(trading.users.len(), 2);
        assert_eq!(trading.users[0].name, "alice");
        assert_eq!(trading.users[1].name, "bob");
        assert_eq!(trading.users[1].leverage, 5);
        assert_eq!(trading.users[1].position_size_usd, 100.0);
    }

    // ── Default values ───────────────────────────────────────────────

    #[test]
    fn defaults_applied_for_minimal_user() {
        let toml_str = r#"
            enabled = true
            [[users]]
            name = "min"
            telegram_chat_id = "999"
            [users.bybit]
            api_key = "k"
            api_secret = "s"
        "#;

        let trading: TradingConfig = toml::from_str(toml_str).unwrap();
        let u = &trading.users[0];
        assert_eq!(u.leverage, 2);
        assert_eq!(u.position_size_usd, 50.0);
        assert_eq!(u.max_open_positions, 3);
        assert_eq!(u.stop_loss.percent, 15.0);
        assert_eq!(u.time_exit.minutes, 30);
        assert_eq!(u.take_profit.levels.len(), 2);
        assert!(u.bybit.testnet);
    }

    // ── Validation: duplicate names rejected ─────────────────────────

    #[test]
    fn reject_duplicate_user_names() {
        let trading = TradingConfig {
            enabled: true,
            users: vec![valid_user("alice"), valid_user("alice")],
        };
        let cfg = base_config(trading);
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("duplicate user name"),
            "expected duplicate name error, got: {err}"
        );
    }

    // ── Validation: missing telegram_chat_id rejected ────────────────

    #[test]
    fn reject_missing_telegram_chat_id() {
        let mut user = valid_user("alice");
        user.telegram_chat_id = String::new();
        let trading = TradingConfig {
            enabled: true,
            users: vec![user],
        };
        let cfg = base_config(trading);
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("telegram_chat_id must be set"),
            "expected telegram_chat_id error, got: {err}"
        );
    }

    // ── Validation: no exchange keys rejected ────────────────────────

    #[test]
    fn reject_no_exchange_keys() {
        let mut user = valid_user("alice");
        user.bybit = ExchangeCredentials::default();
        user.binance = ExchangeCredentials::default();
        let trading = TradingConfig {
            enabled: true,
            users: vec![user],
        };
        let cfg = base_config(trading);
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("at least one exchange"),
            "expected exchange keys error, got: {err}"
        );
    }

    // ── Validation: no users when enabled ────────────────────────────

    #[test]
    fn reject_enabled_with_no_users() {
        let trading = TradingConfig {
            enabled: true,
            users: vec![],
        };
        let cfg = base_config(trading);
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("no [[trading.users]] configured"),
            "expected no-users error, got: {err}"
        );
    }

    // ── Validation: passes when disabled ─────────────────────────────

    #[test]
    fn passes_when_disabled_no_users() {
        let trading = TradingConfig {
            enabled: false,
            users: vec![],
        };
        let cfg = base_config(trading);
        cfg.validate().expect("should pass when trading disabled");
    }
}
