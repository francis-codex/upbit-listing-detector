use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::UserConfig;

use super::exchange::Exchange;

/// Tracks an open position for TP/SL/timeout monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPosition {
    pub id: String,
    pub user_id: String,
    pub symbol: String,
    pub exchange_name: String,
    pub entry_price: f64,
    pub quantity: f64,
    pub remaining_qty: f64,
    pub leverage: u32,
    pub opened_at_epoch: i64,
    /// Which TP levels (by index) have already been hit
    pub tp_levels_hit: Vec<usize>,
}

/// Outcome of a position monitor cycle.
enum MonitorAction {
    TakeProfit { level_idx: usize, close_qty: f64, current_price: f64 },
    StopLoss { current_price: f64 },
    TimeExit { current_price: f64 },
    Continue,
}

/// Run the position monitor loop for a single position.
/// This runs in its own tokio task and exits when the position is fully closed.
pub async fn monitor_position(
    position: OpenPosition,
    exchange: Arc<dyn Exchange>,
    config: UserConfig,
    redis: RedisCache,
    telegram: Arc<TelegramAlert>,
) {
    let symbol = &position.symbol;
    let user_id = &position.user_id;
    let entry_price = position.entry_price;
    let mut remaining_qty = position.remaining_qty;
    let mut tp_levels_hit = position.tp_levels_hit.clone();
    let position_id = &position.id;
    let opened_at = Instant::now()
        - Duration::from_secs(
            (chrono::Utc::now().timestamp() - position.opened_at_epoch).max(0) as u64,
        );
    let time_limit = Duration::from_secs(config.time_exit.minutes * 60);

    info!(
        user = user_id,
        symbol = symbol,
        entry_price = entry_price,
        qty = remaining_qty,
        exchange = position.exchange_name,
        "Position monitor started"
    );

    loop {
        sleep(Duration::from_secs(2)).await;

        // Get current price
        let current_price = match exchange.get_price(symbol).await {
            Ok(p) if p > 0.0 => p,
            Ok(_) => {
                warn!(symbol = symbol, "Got zero price, skipping cycle");
                continue;
            }
            Err(e) => {
                warn!(symbol = symbol, error = %e, "Failed to get price, skipping cycle");
                continue;
            }
        };

        let pnl_pct = ((current_price - entry_price) / entry_price) * 100.0;

        // Determine action
        let action = evaluate_action(
            pnl_pct,
            current_price,
            remaining_qty,
            &tp_levels_hit,
            &config,
            opened_at,
            time_limit,
        );

        match action {
            MonitorAction::TakeProfit { level_idx, close_qty, current_price } => {
                let tp_pct = config.take_profit.levels[level_idx].percent;
                info!(
                    user = user_id,
                    symbol = symbol,
                    tp_level = level_idx,
                    pnl_pct = format!("{:.1}", pnl_pct),
                    close_qty = format!("{:.3}", close_qty),
                    "Take profit triggered"
                );

                match exchange.close_long(symbol, close_qty).await {
                    Ok(_) => {
                        remaining_qty -= close_qty;
                        tp_levels_hit.push(level_idx);

                        let msg = format!(
                            "\u{2705} *TP{} HIT — {}*\n\
                             \n\
                             *User:* {}\n\
                             *Exchange:* {}\n\
                             *Entry:* ${:.4}\n\
                             *Exit:* ${:.4}\n\
                             *PnL:* +{:.1}%\n\
                             *Closed:* {:.3} (remaining: {:.3})",
                            level_idx + 1,
                            symbol,
                            user_id,
                            position.exchange_name,
                            entry_price,
                            current_price,
                            tp_pct,
                            close_qty,
                            remaining_qty,
                        );
                        let _ = telegram.send_message(&msg).await;

                        // Persist updated state
                        let updated = OpenPosition {
                            remaining_qty,
                            tp_levels_hit: tp_levels_hit.clone(),
                            ..position.clone()
                        };
                        let _ = redis.save_position(user_id, &updated).await;
                    }
                    Err(e) => {
                        error!(symbol = symbol, error = %e, "Failed to close TP order");
                    }
                }

                if remaining_qty <= 0.001 {
                    info!(user = user_id, symbol = symbol, "Position fully closed via TP");
                    let _ = redis.remove_position(user_id, position_id).await;
                    return;
                }
            }
            MonitorAction::StopLoss { current_price } => {
                warn!(
                    user = user_id,
                    symbol = symbol,
                    pnl_pct = format!("{:.1}", pnl_pct),
                    "Stop loss triggered"
                );

                match exchange.close_long(symbol, remaining_qty).await {
                    Ok(_) => {
                        let msg = format!(
                            "\u{1f534} *STOP LOSS — {}*\n\
                             \n\
                             *User:* {}\n\
                             *Exchange:* {}\n\
                             *Entry:* ${:.4}\n\
                             *Exit:* ${:.4}\n\
                             *PnL:* {:.1}%\n\
                             *Closed:* {:.3}",
                            symbol,
                            user_id,
                            position.exchange_name,
                            entry_price,
                            current_price,
                            pnl_pct,
                            remaining_qty,
                        );
                        let _ = telegram.send_message(&msg).await;
                    }
                    Err(e) => {
                        error!(symbol = symbol, error = %e, "Failed to close SL order");
                    }
                }

                let _ = redis.remove_position(user_id, position_id).await;
                return;
            }
            MonitorAction::TimeExit { current_price } => {
                warn!(
                    user = user_id,
                    symbol = symbol,
                    pnl_pct = format!("{:.1}", pnl_pct),
                    minutes = config.time_exit.minutes,
                    "Time exit triggered"
                );

                match exchange.close_long(symbol, remaining_qty).await {
                    Ok(_) => {
                        let msg = format!(
                            "\u{23f0} *TIME EXIT — {}*\n\
                             \n\
                             *User:* {}\n\
                             *Exchange:* {}\n\
                             *Entry:* ${:.4}\n\
                             *Exit:* ${:.4}\n\
                             *PnL:* {:.1}%\n\
                             *Closed:* {:.3}\n\
                             *Reason:* {} min timeout",
                            symbol,
                            user_id,
                            position.exchange_name,
                            entry_price,
                            current_price,
                            pnl_pct,
                            remaining_qty,
                            config.time_exit.minutes,
                        );
                        let _ = telegram.send_message(&msg).await;
                    }
                    Err(e) => {
                        error!(symbol = symbol, error = %e, "Failed to close time exit order");
                    }
                }

                let _ = redis.remove_position(user_id, position_id).await;
                return;
            }
            MonitorAction::Continue => {}
        }
    }
}

fn evaluate_action(
    pnl_pct: f64,
    current_price: f64,
    remaining_qty: f64,
    tp_levels_hit: &[usize],
    config: &UserConfig,
    opened_at: Instant,
    time_limit: Duration,
) -> MonitorAction {
    // Check stop loss first
    if pnl_pct <= -(config.stop_loss.percent) {
        return MonitorAction::StopLoss { current_price };
    }

    // Check time exit
    if opened_at.elapsed() >= time_limit {
        return MonitorAction::TimeExit { current_price };
    }

    // Check take profit levels (in order)
    for (idx, level) in config.take_profit.levels.iter().enumerate() {
        if tp_levels_hit.contains(&idx) {
            continue;
        }
        if pnl_pct >= level.percent {
            let close_qty = remaining_qty * level.close_fraction;
            // Ensure we don't try to close more than remaining
            let close_qty = close_qty.min(remaining_qty);
            return MonitorAction::TakeProfit {
                level_idx: idx,
                close_qty,
                current_price,
            };
        }
    }

    MonitorAction::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> UserConfig {
        UserConfig {
            name: "test".to_string(),
            telegram_chat_id: "123".to_string(),
            position_size_usd: 50.0,
            leverage: 2,
            max_open_positions: 3,
            take_profit: crate::config::TakeProfitConfig {
                levels: vec![
                    crate::config::TakeProfitLevel { percent: 80.0, close_fraction: 0.5 },
                    crate::config::TakeProfitLevel { percent: 100.0, close_fraction: 1.0 },
                ],
            },
            stop_loss: crate::config::StopLossConfig { percent: 15.0 },
            time_exit: crate::config::TimeExitConfig { minutes: 30 },
            bybit: Default::default(),
            binance: Default::default(),
        }
    }

    #[test]
    fn test_stop_loss_triggers() {
        let config = test_config();
        let action = evaluate_action(
            -15.5, 0.85, 10.0, &[], &config,
            Instant::now(), Duration::from_secs(1800),
        );
        assert!(matches!(action, MonitorAction::StopLoss { .. }));
    }

    #[test]
    fn test_tp1_triggers() {
        let config = test_config();
        let action = evaluate_action(
            82.0, 1.82, 10.0, &[], &config,
            Instant::now(), Duration::from_secs(1800),
        );
        match action {
            MonitorAction::TakeProfit { level_idx, close_qty, .. } => {
                assert_eq!(level_idx, 0);
                assert!((close_qty - 5.0).abs() < 0.01);
            }
            _ => panic!("Expected TakeProfit"),
        }
    }

    #[test]
    fn test_tp2_after_tp1() {
        let config = test_config();
        let action = evaluate_action(
            105.0, 2.05, 5.0, &[0], &config,
            Instant::now(), Duration::from_secs(1800),
        );
        match action {
            MonitorAction::TakeProfit { level_idx, close_qty, .. } => {
                assert_eq!(level_idx, 1);
                assert!((close_qty - 5.0).abs() < 0.01);
            }
            _ => panic!("Expected TakeProfit"),
        }
    }

    #[test]
    fn test_continue_when_no_trigger() {
        let config = test_config();
        let action = evaluate_action(
            20.0, 1.20, 10.0, &[], &config,
            Instant::now(), Duration::from_secs(1800),
        );
        assert!(matches!(action, MonitorAction::Continue));
    }
}
