use anyhow::Result;
use async_trait::async_trait;

/// Represents a filled order returned by an exchange.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OrderResult {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub filled_qty: f64,
    pub avg_price: f64,
}

/// Round a quantity down to the exchange's step size and format it as a string.
pub fn format_qty(qty: f64, step: f64) -> String {
    let rounded = (qty / step).floor() * step;
    let decimals = (-step.log10()).ceil().max(0.0) as usize;
    format!("{:.prec$}", rounded, prec = decimals)
}

/// Unified exchange interface for futures trading.
#[async_trait]
pub trait Exchange: Send + Sync {
    /// Human-readable exchange name (e.g. "Bybit", "Binance").
    fn name(&self) -> &str;

    /// Check whether a USDT-margined linear perpetual exists for the given symbol.
    /// Returns true if e.g. "CFGUSDT" is a valid trading pair.
    async fn symbol_exists(&self, symbol: &str) -> Result<bool>;

    /// Get the 24h trading volume (in USDT) for a symbol.
    /// Returns 0.0 if the symbol doesn't exist.
    async fn get_volume(&self, symbol: &str) -> Result<f64>;

    /// Set the leverage for a symbol. Must be called before opening a position.
    async fn set_leverage(&self, symbol: &str, leverage: u32) -> Result<()>;

    /// Get the minimum quantity step size for a symbol (e.g. 0.001 for BTC).
    async fn get_qty_step(&self, symbol: &str) -> Result<f64>;

    /// Open a long market order for the given notional USD size.
    /// The implementation should calculate the appropriate quantity from the current price.
    async fn open_long(&self, symbol: &str, size_usd: f64) -> Result<OrderResult>;

    /// Close (reduce) a position by selling the given quantity at market.
    async fn close_long(&self, symbol: &str, qty: f64) -> Result<OrderResult>;

    /// Get the current mark/last price for a symbol.
    async fn get_price(&self, symbol: &str) -> Result<f64>;
}
