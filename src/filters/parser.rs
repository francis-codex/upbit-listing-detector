use regex::Regex;
use std::sync::LazyLock;

/// Extracted information about a listing announcement.
#[derive(Debug, Clone)]
pub struct ListingInfo {
    /// Token symbol, e.g. "SOL"
    pub token_symbol: String,
    /// Full token name if discoverable
    pub token_name: Option<String>,
    /// Markets the token is listed on, e.g. ["KRW", "BTC"]
    pub markets: Vec<String>,
    /// Trading start time as free-text, if mentioned
    pub trading_start_time: Option<String>,
    /// Confidence score from keyword filtering
    pub confidence: f32,
}

/// Quote currencies that should not be treated as listed tokens.
const QUOTE_CURRENCIES: &[&str] = &["KRW", "BTC", "USDT", "ETH", "BUSD"];

// Pre-compiled regex patterns for token extraction.
static RE_PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[(\uff08]([A-Z]{2,10})[)\uff09]").unwrap());
static RE_TRADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Z]{2,10})\s*거래").unwrap());
static RE_SLASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Z]{2,10})[/\uff0f]").unwrap());
static RE_MARKET_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(KRW|BTC|USDT)-([A-Z]{2,10})").unwrap());
static RE_TIME_KR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})[./-](\d{1,2})[./-](\d{1,2})\s*(\d{1,2}:\d{2})").unwrap()
});

/// Parse a listing announcement title/body and extract token information.
pub fn parse_listing(text: &str, confidence: f32) -> Option<ListingInfo> {
    let symbols = extract_symbols(text);
    if symbols.is_empty() {
        return None;
    }

    let token_symbol = symbols[0].clone();
    let markets = extract_markets(text);
    let trading_start_time = extract_time(text);

    Some(ListingInfo {
        token_symbol,
        token_name: None,
        markets,
        trading_start_time,
        confidence,
    })
}

/// Extract all candidate token symbols from text, filtering out quote currencies.
fn extract_symbols(text: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let regexes: &[&Regex] = &[&RE_PARENS, &RE_TRADING, &RE_SLASH, &RE_MARKET_CODE];

    for re in regexes {
        for cap in re.captures_iter(text) {
            // RE_MARKET_CODE captures the token in group 2, others in group 1
            let idx = if re.as_str().contains("(KRW|BTC|USDT)-") {
                2
            } else {
                1
            };
            if let Some(m) = cap.get(idx) {
                let sym = m.as_str().to_uppercase();
                if !QUOTE_CURRENCIES.contains(&sym.as_str()) && seen.insert(sym.clone()) {
                    symbols.push(sym);
                }
            }
        }
    }
    symbols
}

/// Extract market types from text (KRW, BTC, USDT).
fn extract_markets(text: &str) -> Vec<String> {
    let mut markets = Vec::new();
    for code in &["KRW", "BTC", "USDT"] {
        if text.contains(code) {
            markets.push(code.to_string());
        }
    }
    if markets.is_empty() {
        // Check Korean text for market references
        if text.contains("원화") {
            markets.push("KRW".to_string());
        }
    }
    markets
}

/// Extract a trading start time from text.
fn extract_time(text: &str) -> Option<String> {
    RE_TIME_KR.find(text).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parens_extraction() {
        let info = parse_listing("원화(KRW) 마켓 디지털 자산 추가 (SOL)", 0.9).unwrap();
        assert_eq!(info.token_symbol, "SOL");
        assert!(info.markets.contains(&"KRW".to_string()));
    }

    #[test]
    fn test_trading_keyword_extraction() {
        let info = parse_listing("SOL 거래 지원 안내", 0.8).unwrap();
        assert_eq!(info.token_symbol, "SOL");
    }

    #[test]
    fn test_market_code_extraction() {
        let info = parse_listing("KRW-DOGE 마켓 추가", 0.9).unwrap();
        assert_eq!(info.token_symbol, "DOGE");
        assert!(info.markets.contains(&"KRW".to_string()));
    }

    #[test]
    fn test_slash_extraction() {
        let info = parse_listing("SOL/KRW 거래 시작", 0.8).unwrap();
        assert_eq!(info.token_symbol, "SOL");
    }

    #[test]
    fn test_filters_out_quote_currencies() {
        // "KRW" alone should not be extracted as token
        let info = parse_listing("원화(KRW) 마켓 안내", 0.5);
        assert!(info.is_none());
    }

    #[test]
    fn test_time_extraction() {
        let info = parse_listing("(SOL) 거래 시작 2026.01.27 13:00", 0.9).unwrap();
        assert_eq!(info.trading_start_time.as_deref(), Some("2026.01.27 13:00"));
    }

    #[test]
    fn test_multiple_symbols_takes_first() {
        let info = parse_listing("(SOL) (DOGE) 상장", 0.9).unwrap();
        assert_eq!(info.token_symbol, "SOL");
    }
}
