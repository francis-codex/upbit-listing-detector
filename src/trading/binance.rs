use anyhow::{Context, Result};
use reqwest::Client;
use tracing::debug;

use super::exchange::{format_qty, Exchange, OrderResult};
use super::signing::hmac_sha256;

pub struct BinanceExchange {
    client: Client,
    base_url: String,
    api_key: String,
    api_secret: String,
}

impl BinanceExchange {
    pub fn new(client: Client, base_url: &str, api_key: &str, api_secret: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
        }
    }

    fn timestamp_ms() -> String {
        chrono::Utc::now().timestamp_millis().to_string()
    }

    /// Sign a query string for Binance USDS-M Futures.
    /// Binance signs the entire query string with HMAC-SHA256.
    fn sign_query(&self, query: &str) -> String {
        let signature = hmac_sha256(&self.api_secret, query);
        format!("{}&signature={}", query, signature)
    }

    /// Make an authenticated POST request with query params.
    async fn signed_post(&self, path: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let timestamp = Self::timestamp_ms();

        let mut query_parts: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        query_parts.push(format!("timestamp={}", timestamp));
        let query = query_parts.join("&");
        let signed_query = self.sign_query(&query);

        let full_url = format!("{}?{}", url, signed_query);

        let resp = self
            .client
            .post(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .with_context(|| format!("Binance POST {path} failed"))?;

        let status = resp.status();
        let json: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Binance response")?;

        if !status.is_success() {
            let code = json["code"].as_i64().unwrap_or(-1);
            let msg = json["msg"].as_str().unwrap_or("unknown");
            anyhow::bail!("Binance API error {code}: {msg}");
        }

        Ok(json)
    }

    /// Make an unauthenticated GET request.
    async fn public_get(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Binance GET {path} failed"))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Binance response")?;

        Ok(json)
    }
}

#[async_trait::async_trait]
impl Exchange for BinanceExchange {
    fn name(&self) -> &str {
        "Binance"
    }

    async fn symbol_exists(&self, symbol: &str) -> Result<bool> {
        let json = self.public_get("/fapi/v1/exchangeInfo").await?;

        let symbols = json["symbols"].as_array();
        Ok(symbols.map_or(false, |arr| {
            arr.iter().any(|s| {
                s["symbol"].as_str() == Some(symbol)
                    && s["status"].as_str() == Some("TRADING")
                    && s["contractType"].as_str() == Some("PERPETUAL")
            })
        }))
    }

    async fn get_volume(&self, symbol: &str) -> Result<f64> {
        let path = format!("/fapi/v1/ticker/24hr?symbol={}", symbol);
        match self.public_get(&path).await {
            Ok(json) => {
                let volume_str = json["quoteVolume"].as_str().unwrap_or("0");
                let volume: f64 = volume_str.parse().unwrap_or(0.0);
                Ok(volume)
            }
            Err(_) => Ok(0.0),
        }
    }

    async fn set_leverage(&self, symbol: &str, leverage: u32) -> Result<()> {
        let lev_str = leverage.to_string();
        let params = [("symbol", symbol), ("leverage", &lev_str)];

        match self.signed_post("/fapi/v1/leverage", &params).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                // -4028 means leverage unchanged
                if err_str.contains("-4028") {
                    debug!(symbol = symbol, leverage = leverage, "Leverage already set");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn get_qty_step(&self, symbol: &str) -> Result<f64> {
        let path = format!("/fapi/v1/exchangeInfo?symbol={}", symbol);
        let json = self.public_get(&path).await?;
        let symbols = json["symbols"]
            .as_array()
            .context("Missing symbols in Binance exchangeInfo")?;
        let info = symbols
            .iter()
            .find(|s| s["symbol"].as_str() == Some(symbol))
            .context("Symbol not found in Binance exchangeInfo")?;
        let filters = info["filters"]
            .as_array()
            .context("Missing filters in Binance symbol info")?;
        let lot_size = filters
            .iter()
            .find(|f| f["filterType"].as_str() == Some("LOT_SIZE"))
            .context("Missing LOT_SIZE filter")?;
        let step_str = lot_size["stepSize"]
            .as_str()
            .context("Missing stepSize in LOT_SIZE filter")?;
        let step: f64 = step_str.parse().context("Failed to parse stepSize")?;
        Ok(step)
    }

    async fn open_long(&self, symbol: &str, size_usd: f64) -> Result<OrderResult> {
        let price = self.get_price(symbol).await?;
        if price <= 0.0 {
            anyhow::bail!("Invalid price {price} for {symbol}");
        }

        let step = self.get_qty_step(symbol).await?;
        let qty = size_usd / price;
        let qty_str = format_qty(qty, step);
        if qty_str.parse::<f64>().unwrap_or(0.0) <= 0.0 {
            anyhow::bail!(
                "Order size ${size_usd} too small for {symbol} at price {price} (step {step})"
            );
        }

        let params = [
            ("symbol", symbol),
            ("side", "BUY"),
            ("type", "MARKET"),
            ("quantity", &qty_str),
        ];

        let json = self.signed_post("/fapi/v1/order", &params).await?;

        let order_id = json["orderId"].as_i64().unwrap_or(0).to_string();
        let avg_price = json["avgPrice"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(price);
        let filled = json["executedQty"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(qty);

        Ok(OrderResult {
            order_id,
            symbol: symbol.to_string(),
            side: "BUY".to_string(),
            filled_qty: filled,
            avg_price,
        })
    }

    async fn close_long(&self, symbol: &str, qty: f64) -> Result<OrderResult> {
        let step = self.get_qty_step(symbol).await?;
        let qty_str = format_qty(qty, step);
        if qty_str.parse::<f64>().unwrap_or(0.0) <= 0.0 {
            anyhow::bail!("Close quantity too small for {symbol} (qty {qty}, step {step})");
        }

        let params = [
            ("symbol", symbol),
            ("side", "SELL"),
            ("type", "MARKET"),
            ("quantity", &qty_str),
            ("reduceOnly", "true"),
        ];

        let json = self.signed_post("/fapi/v1/order", &params).await?;

        let order_id = json["orderId"].as_i64().unwrap_or(0).to_string();
        let avg_price = json["avgPrice"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let filled = json["executedQty"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(qty);

        Ok(OrderResult {
            order_id,
            symbol: symbol.to_string(),
            side: "SELL".to_string(),
            filled_qty: filled,
            avg_price,
        })
    }

    async fn get_price(&self, symbol: &str) -> Result<f64> {
        let path = format!("/fapi/v1/ticker/price?symbol={}", symbol);
        let json = self.public_get(&path).await?;

        let price_str = json["price"].as_str().unwrap_or("0");
        let price: f64 = price_str.parse().unwrap_or(0.0);
        Ok(price)
    }
}
