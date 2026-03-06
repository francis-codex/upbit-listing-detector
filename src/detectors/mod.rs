pub mod notice_api;
pub mod websocket;

/// Market as returned by the Upbit market/all endpoint.
#[derive(Debug, serde::Deserialize, Clone)]
#[allow(dead_code)]
pub struct Market {
    pub market: String,
    pub korean_name: String,
    pub english_name: String,
}
