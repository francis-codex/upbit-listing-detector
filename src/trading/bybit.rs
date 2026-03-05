use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use reqwest::Client;
use tracing::debug;

use super::exchange::{format_qty, Exchange, OrderResult};
use super::signing::hmac_sha256;

pub struct BybitExchange {
    client: Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    qty_step_cache: Mutex<HashMap<String, f64>>,
}

impl BybitExchange {
    pub fn new(client: Client, base_url: &str, api_key: &str, api_secret: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            qty_step_cache: Mutex::new(HashMap::new()),
        }
    }

    fn timestamp_ms() -> String {
        chrono::Utc::now().timestamp_millis().to_string()
    }

    /// Build signed headers for Bybit v5 API.
    /// Bybit signs: timestamp + api_key + recv_window + body
    fn signed_headers(&self, body: &str) -> Vec<(&'static str, String)> {
        let timestamp = Self::timestamp_ms();
        let recv_window = "5000";
        let sign_payload = format!("{}{}{}{}", timestamp, self.api_key, recv_window, body);
        let signature = hmac_sha256(&self.api_secret, &sign_payload);

        vec![
            ("X-BAPI-API-KEY", self.api_key.clone()),
            ("X-BAPI-TIMESTAMP", timestamp),
            ("X-BAPI-RECV-WINDOW", recv_window.to_string()),
            ("X-BAPI-SIGN", signature),
        ]
    }

    /// Make an authenticated POST request.
    async fn signed_post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let body_str = body.to_string();
        let headers = self.signed_headers(&body_str);

        let mut req = self.client.post(&url).header("Content-Type", "application/json");
        for (key, val) in headers {
            req = req.header(key, val);
        }

        let resp = req
            .body(body_str)
            .send()
            .await
            .with_context(|| format!("Bybit POST {path} failed"))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Bybit response")?;

        let ret_code = json["retCode"].as_i64().unwrap_or(-1);
        if ret_code != 0 {
            let msg = json["retMsg"].as_str().unwrap_or("unknown");
            anyhow::bail!("Bybit API error {ret_code}: {msg}");
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
            .with_context(|| format!("Bybit GET {path} failed"))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Bybit response")?;

        let ret_code = json["retCode"].as_i64().unwrap_or(-1);
        if ret_code != 0 {
            let msg = json["retMsg"].as_str().unwrap_or("unknown");
            anyhow::bail!("Bybit API error {ret_code}: {msg}");
        }

        Ok(json)
    }
}

#[async_trait::async_trait]
impl Exchange for BybitExchange {
    fn name(&self) -> &str {
        "Bybit"
    }

    async fn symbol_exists(&self, symbol: &str) -> Result<bool> {
        let path = format!(
            "/v5/market/instruments-info?category=linear&symbol={}",
            symbol
        );
        match self.public_get(&path).await {
            Ok(json) => {
                let list = json["result"]["list"].as_array();
                Ok(list.map_or(false, |l| !l.is_empty()))
            }
            Err(e) => {
                debug!(error = %e, symbol = symbol, "Bybit symbol check failed");
                Ok(false)
            }
        }
    }

    async fn get_volume(&self, symbol: &str) -> Result<f64> {
        let path = format!("/v5/market/tickers?category=linear&symbol={}", symbol);
        let json = self.public_get(&path).await?;

        let volume_str = json["result"]["list"][0]["turnover24h"]
            .as_str()
            .unwrap_or("0");
        let volume: f64 = volume_str.parse().unwrap_or(0.0);
        Ok(volume)
    }

    async fn set_leverage(&self, symbol: &str, leverage: u32) -> Result<()> {
        let body = serde_json::json!({
            "category": "linear",
            "symbol": symbol,
            "buyLeverage": leverage.to_string(),
            "sellLeverage": leverage.to_string(),
        });

        match self.signed_post("/v5/position/set-leverage", &body).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                // Error 110043 means leverage is already set to that value
                if err_str.contains("110043") {
                    debug!(symbol = symbol, leverage = leverage, "Leverage already set");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn get_qty_step(&self, symbol: &str) -> Result<f64> {
        if let Some(&step) = self.qty_step_cache.lock().unwrap().get(symbol) {
            return Ok(step);
        }
        let path = format!(
            "/v5/market/instruments-info?category=linear&symbol={}",
            symbol
        );
        let json = self.public_get(&path).await?;
        let step_str = json["result"]["list"][0]["lotSizeFilter"]["qtyStep"]
            .as_str()
            .context("Missing qtyStep in Bybit instrument info")?;
        let step: f64 = step_str
            .parse()
            .context("Failed to parse qtyStep")?;
        self.qty_step_cache.lock().unwrap().insert(symbol.to_string(), step);
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

        let body = serde_json::json!({
            "category": "linear",
            "symbol": symbol,
            "side": "Buy",
            "orderType": "Market",
            "qty": qty_str,
        });

        let json = self.signed_post("/v5/order/create", &body).await?;

        let order_id = json["result"]["orderId"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(OrderResult {
            order_id,
            symbol: symbol.to_string(),
            side: "Buy".to_string(),
            filled_qty: qty,
            avg_price: price,
        })
    }

    async fn close_long(&self, symbol: &str, qty: f64) -> Result<OrderResult> {
        let step = self.get_qty_step(symbol).await?;
        let qty_str = format_qty(qty, step);
        if qty_str.parse::<f64>().unwrap_or(0.0) <= 0.0 {
            anyhow::bail!("Close quantity too small for {symbol} (qty {qty}, step {step})");
        }

        let body = serde_json::json!({
            "category": "linear",
            "symbol": symbol,
            "side": "Sell",
            "orderType": "Market",
            "qty": qty_str,
            "reduceOnly": true,
        });

        let json = self.signed_post("/v5/order/create", &body).await?;

        let order_id = json["result"]["orderId"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let price = self.get_price(symbol).await.unwrap_or(0.0);

        Ok(OrderResult {
            order_id,
            symbol: symbol.to_string(),
            side: "Sell".to_string(),
            filled_qty: qty,
            avg_price: price,
        })
    }

    async fn get_price(&self, symbol: &str) -> Result<f64> {
        let path = format!("/v5/market/tickers?category=linear&symbol={}", symbol);
        let json = self.public_get(&path).await?;

        let price_str = json["result"]["list"][0]["lastPrice"]
            .as_str()
            .unwrap_or("0");
        let price: f64 = price_str.parse().unwrap_or(0.0);
        Ok(price)
    }
}
