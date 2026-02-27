use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::alerts::telegram::TelegramAlert;
use crate::cache::redis::RedisCache;
use crate::config::TradingConfig;

use super::binance::BinanceExchange;
use super::bybit::BybitExchange;
use super::exchange::Exchange;
use super::position::{monitor_position, OpenPosition};
use super::TradeSignal;

/// Run the trade executor loop.
/// Receives TradeSignals from detectors via mpsc channel.
/// For each signal, checks both exchanges, picks the best, and opens a trade.
pub async fn run(
    mut rx: mpsc::Receiver<TradeSignal>,
    config: TradingConfig,
    bybit: Arc<BybitExchange>,
    binance: Arc<BinanceExchange>,
    redis: RedisCache,
    telegram: Arc<TelegramAlert>,
) -> Result<()> {
    info!("Trade executor started (enabled={})", config.enabled);

    // Resume any positions from Redis on startup
    resume_positions(
        &config,
        bybit.clone(),
        binance.clone(),
        &redis,
        telegram.clone(),
    )
    .await;

    while let Some(signal) = rx.recv().await {
        if !config.enabled {
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
            "Received trade signal"
        );

        let config = config.clone();
        let bybit = bybit.clone();
        let binance = binance.clone();
        let redis = redis.clone();
        let telegram = telegram.clone();

        // Handle each signal in a separate task so we don't block the receiver
        tokio::spawn(async move {
            if let Err(e) =
                handle_signal(signal, &config, bybit, binance, &redis, telegram.clone()).await
            {
                error!(error = %e, "Trade signal handling failed");
            }
        });
    }

    warn!("Trade executor channel closed, exiting");
    Ok(())
}

async fn handle_signal(
    signal: TradeSignal,
    config: &TradingConfig,
    bybit: Arc<BybitExchange>,
    binance: Arc<BinanceExchange>,
    redis: &RedisCache,
    telegram: Arc<TelegramAlert>,
) -> Result<()> {
    let futures_symbol = format!("{}USDT", signal.symbol);

    // Dedup: check if we already traded this symbol recently
    if redis.is_trade_recent(&signal.symbol).await? {
        info!(symbol = signal.symbol, "Trade already placed recently, skipping");
        return Ok(());
    }

    // Check max open positions
    let positions = redis.get_open_positions().await?;
    if positions.len() >= config.max_open_positions as usize {
        warn!(
            symbol = signal.symbol,
            open = positions.len(),
            max = config.max_open_positions,
            "Max open positions reached, skipping"
        );
        return Ok(());
    }

    // Check both exchanges in parallel
    let (bybit_exists, binance_exists) = tokio::join!(
        bybit.symbol_exists(&futures_symbol),
        binance.symbol_exists(&futures_symbol),
    );

    let bybit_ok = bybit_exists.unwrap_or(false);
    let binance_ok = binance_exists.unwrap_or(false);

    if !bybit_ok && !binance_ok {
        info!(
            symbol = futures_symbol,
            "Symbol not found on either exchange"
        );
        return Ok(());
    }

    // Pick exchange with more volume (or whichever has it)
    let exchange: Arc<dyn Exchange> = if bybit_ok && binance_ok {
        let (bybit_vol, binance_vol) = tokio::join!(
            bybit.get_volume(&futures_symbol),
            binance.get_volume(&futures_symbol),
        );
        let bv = bybit_vol.unwrap_or(0.0);
        let bnv = binance_vol.unwrap_or(0.0);
        info!(
            bybit_volume = bv,
            binance_volume = bnv,
            "Comparing exchange volumes"
        );
        if bv >= bnv {
            bybit as Arc<dyn Exchange>
        } else {
            binance as Arc<dyn Exchange>
        }
    } else if bybit_ok {
        bybit as Arc<dyn Exchange>
    } else {
        binance as Arc<dyn Exchange>
    };

    let exchange_name = exchange.name().to_string();
    info!(
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
        order_id = result.order_id,
        symbol = result.symbol,
        qty = result.filled_qty,
        price = result.avg_price,
        exchange = exchange_name,
        "Position opened"
    );

    // Record trade dedup
    redis.record_trade(&signal.symbol).await?;

    // Build position object
    let position = OpenPosition {
        id: uuid::Uuid::new_v4().to_string(),
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
    redis.save_position(&position).await?;

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
    let _ = telegram.send_message(&msg).await;

    // Spawn position monitor
    let monitor_config = config.clone();
    let monitor_redis = redis.clone();
    let monitor_telegram = telegram.clone();
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

/// On startup, resume monitoring any positions saved in Redis.
async fn resume_positions(
    config: &TradingConfig,
    bybit: Arc<BybitExchange>,
    binance: Arc<BinanceExchange>,
    redis: &RedisCache,
    telegram: Arc<TelegramAlert>,
) {
    let positions = match redis.get_open_positions().await {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "Failed to load positions from Redis");
            return;
        }
    };

    if positions.is_empty() {
        return;
    }

    info!(count = positions.len(), "Resuming open positions from Redis");

    for position in positions {
        let exchange: Arc<dyn Exchange> = if position.exchange_name == "Bybit" {
            bybit.clone()
        } else {
            binance.clone()
        };

        let config = config.clone();
        let redis = redis.clone();
        let telegram = telegram.clone();

        let msg = format!(
            "\u{1f504} *RESUMING POSITION — {}*\n\
             *Exchange:* {}\n\
             *Entry:* ${:.4}\n\
             *Remaining Qty:* {:.3}",
            position.symbol, position.exchange_name, position.entry_price, position.remaining_qty,
        );
        let _ = telegram.send_message(&msg).await;

        tokio::spawn(async move {
            monitor_position(position, exchange, config, redis, telegram).await;
        });
    }
}
