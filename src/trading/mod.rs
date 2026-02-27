pub mod binance;
pub mod bybit;
pub mod exchange;
pub mod executor;
pub mod position;
pub mod signing;

/// A trade signal emitted by any detector when a listing is detected.
#[derive(Debug, Clone)]
pub struct TradeSignal {
    /// Token symbol, e.g. "CFG"
    pub symbol: String,
    /// Source detector that produced this signal
    pub source: String,
    /// Confidence from keyword filter (0.0–1.0), None for market/ws detectors
    pub confidence: Option<f32>,
}
