use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::UserConfig;

use super::binance::BinanceExchange;
use super::bybit::BybitExchange;
use super::exchange::Exchange;
use super::position::{monitor_position, OpenPosition};
use super::TradeSignal;

/// Per-user bundle of exchange clients, config, and Telegram sender.
pub struct UserContext {
    pub config: UserConfig,
    pub bybit: Arc<BybitExchange>,
    pub binance: Arc<BinanceExchange>,
    pub telegram: Arc<TelegramAlert>,
}

/// Run the trade executor loop.
/// Receives TradeSignals from detectors via mpsc channel.
/// For each signal, fans out to ALL users in parallel.
pub async fn run(
    mut rx: mpsc::Receiver<TradeSignal>,
    enabled: bool,
    users: Vec<UserContext>,
    redis: RedisCache,
) -> Result<()> {
    info!("Trade executor started (enabled={}, users={})", enabled, users.len());

    // Wrap users in Arc for cheap cloning into spawned tasks
    let users = Arc::new(users);

    // Resume any positions from Redis on startup for each user
    for user in users.iter() {
        resume_positions(
            user,
            &redis,
        )
        .await;
    }

    while let Some(signal) = rx.recv().await {
        if !enabled {
            info!(
                symbol = signal.symbol,
                source = signal.source,
                "Trading disabled, ignoring signal"
            );
            continue;
        }

        info!(
            symbol = signal.symbol,
            source = signal.source,
            confidence = ?signal.confidence,
            "Received trade signal, broadcasting to {} users",
            users.len(),
        );

        // Fan out to all users in parallel
        for (idx, _) in users.iter().enumerate() {
            let signal = signal.clone();
            let users = users.clone();
            let redis = redis.clone();

            tokio::spawn(async move {
                let user = &users[idx];
                if let Err(e) = handle_signal_for_user(
                    signal,
                    user,
                    &redis,
                ).await {
                    error!(
                        user = user.config.name,
                        error = %e,
                        "Trade signal handling failed"
                    );
                }
            });
        }
    }

    warn!("Trade executor channel closed, exiting");
    Ok(())
}

async fn handle_signal_for_user(
    signal: TradeSignal,
    user: &UserContext,
    redis: &RedisCache,
) -> Result<()> {
    let config = &user.config;
    let user_id = &config.name;
    let futures_symbol = format!("{}USDT", signal.symbol);

    // Dedup: check if this user already traded this symbol recently
    if redis.is_trade_recent(user_id, &signal.symbol).await? {
        info!(user = user_id, symbol = signal.symbol, "Trade already placed recently, skipping");
        return Ok(());
    }

    // Check this user's max open positions
    let positions = redis.get_open_positions(user_id).await?;
    if positions.len() >= config.max_open_positions as usize {
        warn!(
            user = user_id,
            symbol = signal.symbol,
            open = positions.len(),
            max = config.max_open_positions,
            "Max open positions reached, skipping"
        );
        return Ok(());
    }

    // Check both exchanges in parallel
    let (bybit_exists, binance_exists) = tokio::join!(
        user.bybit.symbol_exists(&futures_symbol),
        user.binance.symbol_exists(&futures_symbol),
    );

    let bybit_ok = bybit_exists.unwrap_or(false);
    let binance_ok = binance_exists.unwrap_or(false);

    if !bybit_ok && !binance_ok {
        info!(
            user = user_id,
            symbol = futures_symbol,
            "Symbol not found on either exchange"
        );
        return Ok(());
    }

    // Pick exchange with more volume (or whichever has it)
    let exchange: Arc<dyn Exchange> = if bybit_ok && binance_ok {
        let (bybit_vol, binance_vol) = tokio::join!(
            user.bybit.get_volume(&futures_symbol),
            user.binance.get_volume(&futures_symbol),
        );
        let bv = bybit_vol.unwrap_or(0.0);
        let bnv = binance_vol.unwrap_or(0.0);
        info!(
            user = user_id,
            bybit_volume = bv,
            binance_volume = bnv,
            "Comparing exchange volumes"
        );
        if bv >= bnv {
            user.bybit.clone() as Arc<dyn Exchange>
        } else {
            user.binance.clone() as Arc<dyn Exchange>
        }
    } else if bybit_ok {
        user.bybit.clone() as Arc<dyn Exchange>
    } else {
        user.binance.clone() as Arc<dyn Exchange>
    };

    let exchange_name = exchange.name().to_string();
    info!(
        user = user_id,
        symbol = futures_symbol,
        exchange = exchange_name,
        "Opening position"
    );

    // Set leverage
    if let Err(e) = exchange
        .set_leverage(&futures_symbol, config.leverage)
        .await
    {
        error!(error = %e, "Failed to set leverage, proceeding anyway");
    }

    // Open long
    let result = exchange
        .open_long(&futures_symbol, config.position_size_usd)
        .await?;

    info!(
        user = user_id,
        order_id = result.order_id,
        symbol = result.symbol,
        qty = result.filled_qty,
        price = result.avg_price,
        exchange = exchange_name,
        "Position opened"
    );

    // Record trade dedup
    redis.record_trade(user_id, &signal.symbol).await?;

    // Build position object
    let position = OpenPosition {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user_id.clone(),
        symbol: futures_symbol.clone(),
        exchange_name: exchange_name.clone(),
        entry_price: result.avg_price,
        quantity: result.filled_qty,
        remaining_qty: result.filled_qty,
        leverage: config.leverage,
        opened_at_epoch: chrono::Utc::now().timestamp(),
        tp_levels_hit: vec![],
    };

    // Persist position
    redis.save_position(user_id, &position).await?;

    // Send Telegram notification
    let tp_msg = config
        .take_profit
        .levels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            format!(
                "*TP{}:* +{}% (close {}%)",
                i + 1,
                l.percent,
                (l.close_fraction * 100.0) as u32,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let msg = format!(
        "\u{1f4c8} *TRADE OPENED — {}*\n\
         \n\
         *User:* {}\n\
         *Exchange:* {}\n\
         *Entry Price:* ${:.4}\n\
         *Size:* {:.3} ({:.0} USD)\n\
         *Leverage:* {}x\n\
         *Source:* {}\n\
         \n\
         {}\n\
         *SL:* -{}%\n\
         *Timeout:* {} min",
        futures_symbol,
        user_id,
        exchange_name,
        result.avg_price,
        result.filled_qty,
        config.position_size_usd,
        config.leverage,
        signal.source,
        tp_msg,
        config.stop_loss.percent,
        config.time_exit.minutes,
    );
    let _ = user.telegram.send_message(&msg).await;

    // Spawn position monitor
    let monitor_config = config.clone();
    let monitor_redis = redis.clone();
    let monitor_telegram = user.telegram.clone();
    tokio::spawn(async move {
        monitor_position(
            position,
            exchange,
            monitor_config,
            monitor_redis,
            monitor_telegram,
        )
        .await;
    });

    Ok(())
}

/// On startup, resume monitoring any positions saved in Redis for a user.
async fn resume_positions(
    user: &UserContext,
    redis: &RedisCache,
) {
    let user_id = &user.config.name;
    let positions = match redis.get_open_positions(user_id).await {
        Ok(p) => p,
        Err(e) => {
            error!(user = user_id, error = %e, "Failed to load positions from Redis");
            return;
        }
    };

    if positions.is_empty() {
        return;
    }

    info!(user = user_id, count = positions.len(), "Resuming open positions from Redis");

    for position in positions {
        let exchange: Arc<dyn Exchange> = if position.exchange_name == "Bybit" {
            user.bybit.clone()
        } else {
            user.binance.clone()
        };

        let config = user.config.clone();
        let redis = redis.clone();
        let telegram = user.telegram.clone();

        let msg = format!(
            "\u{1f504} *RESUMING POSITION — {}*\n\
             *User:* {}\n\
             *Exchange:* {}\n\
             *Entry:* ${:.4}\n\
             *Remaining Qty:* {:.3}",
            position.symbol, user_id, position.exchange_name, position.entry_price, position.remaining_qty,
        );
        let _ = user.telegram.send_message(&msg).await;

        tokio::spawn(async move {
            monitor_position(position, exchange, config, redis, telegram).await;
        });
    }
}
