# UPBIT LISTING DETECTOR - Complete Implementation Plan

## EXECUTIVE SUMMARY

**What We're Building:** Real-time Upbit listing detection system in Rust  
**Target Latency:** 1-5 seconds (post-listing) + 2-5 seconds (pre-announcement)  
**Tech Stack:** Rust + Tokio + Redis + Telegram  
**Timeline:** 4-5 days to production  
**Budget:** $15-20/month

---

## PHASE 1: TECHNICAL FOUNDATION

### What We Know For Sure

1. **Upbit has NO public notices API** ✅ Confirmed
2. **Market data API exists** ✅ `https://api.upbit.com/v1/market/all`
3. **WebSocket exists** ✅ `wss://api.upbit.com/websocket/v1`
4. **Notice board requires scraping** ✅ No way around it
5. **Geo-restrictions exist** ✅ Must use Seoul-based server or proxy

### Our Two-Path Detection Strategy

```
PATH 1: INSTANT DETECTION (when trading goes live)
┌──────────────────────────────────────┐
│  WebSocket Monitor                   │
│  + API Polling Backup                │
│  = 1-3 second latency                │
└──────────────────────────────────────┘

PATH 2: ADVANCE WARNING (announcement posted)
┌──────────────────────────────────────┐
│  Notice Board Scraping               │
│  (Reverse-engineered API or DOM)    │
│  = 2-5 second latency                │
│  = 1-3 hours advance notice          │
└──────────────────────────────────────┘
```

**Why Both Paths:**
- Path 1 catches when trading starts (guaranteed detection)
- Path 2 gives advance warning (competitive edge)
- Combined = maximum coverage

---

## PHASE 2: ARCHITECTURE DEEP DIVE

### System Components

```
┌─────────────────────────────────────────────────┐
│             RUST MAIN BINARY                    │
│  upbit-listing-detector (single executable)     │
├─────────────────────────────────────────────────┤
│                                                  │
│  ┌───────────────┐  ┌───────────────┐          │
│  │  Market API   │  │  WebSocket    │          │
│  │  Poller       │  │  Monitor      │  PATH 1  │
│  └───────┬───────┘  └───────┬───────┘          │
│          │                   │                  │
│          └───────┬───────────┘                  │
│                  │                              │
│         ┌────────▼────────┐                    │
│         │ Detection Engine│                    │
│         │ - Deduplication │                    │
│         │ - Filtering     │                    │
│         │ - Enrichment    │                    │
│         └────────┬────────┘                    │
│                  │                              │
│  ┌───────────────▼────────┐                   │
│  │ Notice Board Scraper   │  PATH 2           │
│  │ (Reverse-eng API)      │                   │
│  └───────────────┬────────┘                   │
│                  │                             │
└──────────────────┼─────────────────────────────┘
                   │
      ┌────────────▼─────────────┐
      │   REDIS CACHE            │
      │   - Market codes         │
      │   - Notice IDs           │
      │   - Alert history        │
      └──────────────────────────┘
                   │
      ┌────────────▼─────────────┐
      │   TELEGRAM/DISCORD       │
      │   Alert dispatch         │
      └──────────────────────────┘
```

---

## PHASE 3: TECHNOLOGY CHOICES (FINAL)

### Rust Crates Selected

| Purpose | Crate | Version | Rationale |
|---------|-------|---------|-----------|
| **Async Runtime** | `tokio` | 1.x | Industry standard, battle-tested |
| **HTTP Client** | `reqwest` | 0.13 | Fast, ergonomic, built on hyper |
| **WebSocket** | `tokio-tungstenite` | 0.26+ | Production-ready, Tokio-native |
| **JSON** | `serde_json` | 1.0 | Zero-cost abstractions |
| **Redis** | `redis` | 0.25 | Async support, connection pooling |
| **Logging** | `tracing` | 0.1 | Structured logging, async-aware |
| **Error Handling** | `anyhow` | 1.0 | Ergonomic error propagation |
| **Regex** | `regex` | 1.10 | For keyword filtering |
| **Time** | `chrono` | 0.4 | Korean timezone handling |

**Why These Specific Crates:**
- **reqwest over hyper:** Higher-level API, same performance
- **tokio-tungstenite over alternatives:** Best Tokio integration
- **anyhow over thiserror:** We don't need typed errors here
- **redis over alternatives:** Most mature async Rust client

---

## PHASE 4: IMPLEMENTATION ROADMAP

### Day 1: Core Detection Engine

**Deliverables:**
1. Project structure setup
2. Market API polling implementation
3. Redis connection and caching
4. Basic alert system (console output)

**Code to Write (~300 lines):**
```rust
// src/main.rs - Entry point
// src/detectors/market_api.rs - Market code poller
// src/cache/redis.rs - Redis wrapper
// src/config.rs - Configuration
```

**Testing:**
- Verify market API calls work
- Confirm new market detection logic
- Test Redis read/write

**Success Criteria:**
- Can detect when KRW-BTC appears in market list
- Redis correctly caches market codes
- Console shows "NEW LISTING DETECTED"

---

### Day 2: WebSocket + Filtering

**Deliverables:**
1. WebSocket monitoring implementation
2. Keyword-based filtering system
3. Korean + English keyword database
4. Duplicate detection

**Code to Write (~250 lines):**
```rust
// src/detectors/websocket.rs - WebSocket monitor
// src/filters/keywords.rs - Keyword matcher
// src/filters/parser.rs - Token info extractor
```

**Testing:**
- WebSocket connects and stays connected
- Filtering correctly identifies listing announcements
- False positives are rejected

**Success Criteria:**
- WebSocket detects new market codes
- Filter matches "상장" and "listing" keywords
- Rejects "점검" and "이벤트" keywords

---

### Day 3: Notice Board + Telegram

**Deliverables:**
1. Notice board scraping (reverse-engineered API)
2. Telegram bot integration
3. Alert formatting
4. Rate limiting

**Code to Write (~200 lines):**
```rust
// src/detectors/notice_api.rs - Notice scraper
// src/alerts/telegram.rs - Telegram integration
// src/alerts/discord.rs - Discord webhook
```

**Manual Task:**
- Use browser DevTools to find notice API endpoint (10 minutes)
- Create Telegram bot (5 minutes)

**Testing:**
- Notice scraper detects new posts
- Telegram messages arrive within 2 seconds
- Messages format correctly

**Success Criteria:**
- Notice board scraper works
- Telegram bot sends formatted alerts
- Rate limits respected

---

### Day 4: Production Hardening

**Deliverables:**
1. Error recovery and retry logic
2. Graceful shutdown handling
3. Systemd service files
4. Docker containerization
5. Monitoring and metrics

**Code to Write (~150 lines):**
```rust
// Retry logic for failed API calls
// Graceful shutdown on SIGTERM
// Health check endpoint
```

**Infrastructure:**
```bash
# systemd/upbit-detector.service
# Dockerfile
# docker-compose.yml
```

**Testing:**
- Kill process, verify it restarts
- Simulate API failures, verify retries
- Check logs are structured and readable

**Success Criteria:**
- System runs for 24 hours without crashes
- Restarts automatically after failure
- Logs show clear error messages

---

### Day 5: Deployment & Validation

**Tasks:**
1. Deploy to Seoul-based server (AWS EC2 t3.micro)
2. Configure environment variables
3. Set up log rotation
4. Run for 24 hours and monitor

**Validation Checklist:**
- [ ] Market API poller running every 2 seconds
- [ ] WebSocket stays connected
- [ ] Notice scraper running every 3 seconds
- [ ] Redis connection stable
- [ ] Telegram alerts arrive in <3 seconds
- [ ] No memory leaks (check htop)
- [ ] CPU usage <10%

---

## PHASE 5: FILTERING SYSTEM (DETAILED)

### Korean + English Keyword Database

**PRIMARY KEYWORDS (MUST match at least one):**
```rust
const PRIMARY_KR: &[&str] = &[
    "상장",        // Listing
    "거래 지원",   // Trading support
    "신규 상장",   // New listing
    "마켓 추가",   // Market addition
];

const PRIMARY_EN: &[&str] = &[
    "listing",
    "new coin",
    "new token",
    "trading support",
    "market addition",
];
```

**SECONDARY KEYWORDS (Boost confidence):**
```rust
const SECONDARY_KR: &[&str] = &[
    "원화 마켓",   // KRW market
    "입출금",      // Deposit/withdrawal
    "거래 시작",   // Trading starts
];

const SECONDARY_EN: &[&str] = &[
    "krw market",
    "btc market",
    "deposit",
    "withdrawal",
];
```

**EXCLUSION KEYWORDS (Reject if found):**
```rust
const EXCLUSION_KR: &[&str] = &[
    "점검",        // Maintenance
    "일시 중단",   // Suspension
    "상장폐지",    // Delisting
    "이벤트",      // Event
];

const EXCLUSION_EN: &[&str] = &[
    "maintenance",
    "suspension",
    "delisting",
    "event",
];
```

### Filtering Algorithm

```
1. Check EXCLUSION keywords → Reject if found
2. Check PRIMARY keywords → Need at least 1
3. Check SECONDARY keywords → Boost confidence
4. Calculate confidence score (0.0 - 1.0)
5. Require >= 0.6 confidence to alert
```

**Example Scenarios:**

```
Notice: "업비트 원화(KRW) 마켓 디지털 자산 추가 (SOL)"
PRIMARY: "마켓 추가" ✅
SECONDARY: "원화" ✅
EXCLUSION: None ✅
CONFIDENCE: 0.95 → ALERT ✅

Notice: "디지털 자산 입출금 일시 중단 (MATIC)"
EXCLUSION: "일시 중단" ✅
CONFIDENCE: 0.0 → REJECT ❌

Notice: "BTC 거래 이벤트"
EXCLUSION: "이벤트" ✅
CONFIDENCE: 0.0 → REJECT ❌
```

---

## PHASE 6: DEPLOYMENT STRATEGY

### Server Selection

**Option 1: AWS EC2 (Seoul) - RECOMMENDED**
```
Instance: t3.micro (1 vCPU, 1GB RAM)
Region: ap-northeast-2 (Seoul)
Cost: $7.50/month
Latency to Upbit: ~20-30ms
```

**Option 2: Google Cloud (Seoul)**
```
Instance: e2-micro (2 vCPU, 1GB RAM)
Region: asia-northeast3 (Seoul)
Cost: $7.11/month (free tier available)
```

**Option 3: Vultr Seoul**
```
Instance: 1 vCPU, 1GB RAM
Cost: $6/month
Latency: ~25-35ms
```

### Deployment Steps

**1. Provision Server**
```bash
# Launch Ubuntu 24.04 LTS in Seoul region
# Open ports: 22 (SSH), optional: 9090 (metrics)
```

**2. Install Dependencies**
```bash
ssh ubuntu@<server-ip>
sudo apt update && sudo apt upgrade -y
sudo apt install -y redis-server docker.io docker-compose
sudo systemctl enable redis-server
sudo systemctl start redis-server
```

**3. Deploy Application**
```bash
# Copy binary
scp target/release/upbit-detector ubuntu@<server>:/usr/local/bin/

# Copy systemd service
scp systemd/upbit-detector.service ubuntu@<server>:/etc/systemd/system/

# Enable and start
sudo systemctl enable upbit-detector
sudo systemctl start upbit-detector

# Check status
sudo systemctl status upbit-detector
sudo journalctl -u upbit-detector -f
```

---

## PHASE 7: CONFIGURATION

### Environment Variables

```bash
# config.env
UPBIT_MARKET_API=https://api.upbit.com/v1/market/all
UPBIT_WS_URL=wss://api.upbit.com/websocket/v1
UPBIT_NOTICE_API=<TO_BE_DISCOVERED>

REDIS_URL=redis://127.0.0.1:6379
REDIS_KEY_PREFIX=upbit:

TELEGRAM_BOT_TOKEN=<YOUR_TOKEN>
TELEGRAM_CHAT_ID=<YOUR_CHAT_ID>

POLL_INTERVAL_SECONDS=2
NOTICE_POLL_INTERVAL_SECONDS=3

LOG_LEVEL=info
RUST_BACKTRACE=1
```

### Config File (config.toml)

```toml
[api]
market_endpoint = "https://api.upbit.com/v1/market/all"
websocket_endpoint = "wss://api.upbit.com/websocket/v1"
notice_endpoint = "" # To be filled after discovery

[polling]
market_interval_seconds = 2
notice_interval_seconds = 3
websocket_reconnect_delay_seconds = 5

[redis]
url = "redis://127.0.0.1:6379"
key_prefix = "upbit:"
connection_timeout_seconds = 5

[telegram]
bot_token = ""
chat_id = ""

[filters]
min_confidence = 0.6
```

---

## PHASE 8: MONITORING & OBSERVABILITY

### Metrics to Track

```rust
// Prometheus metrics
upbit_api_requests_total
upbit_api_errors_total
upbit_new_listings_total
upbit_websocket_connected (0 or 1)
upbit_redis_operations_total
upbit_alert_sent_total
upbit_detection_latency_seconds
```

### Log Levels

```
ERROR: Failed API calls, Redis disconnections, alert failures
WARN: Retry attempts, rate limit hits, WebSocket reconnects
INFO: New listings detected, alerts sent, startup/shutdown
DEBUG: Each API call, each notice processed, filtering decisions
TRACE: Full request/response bodies
```

### Health Checks

```bash
# Simple HTTP endpoint on :8080/health
curl localhost:8080/health
# Returns: {"status": "healthy", "uptime_seconds": 3600}
```

---

## PHASE 9: EDGE CASES & ERROR HANDLING

### Network Failures
```rust
// Retry logic with exponential backoff
async fn with_retry<F, T>(f: F, max_retries: u32) -> Result<T>
where
    F: Fn() -> Future<Output = Result<T>>,
{
    let mut delay = Duration::from_secs(1);
    for attempt in 0..max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt < max_retries - 1 => {
                warn!("Attempt {} failed: {}, retrying in {:?}", attempt, e, delay);
                sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
            Err(e) => return Err(e),
        }
    }
}
```

### WebSocket Disconnections
```rust
// Auto-reconnect loop
loop {
    match connect_websocket().await {
        Ok(ws) => {
            info!("WebSocket connected");
            if let Err(e) = handle_websocket(ws).await {
                error!("WebSocket error: {}", e);
            }
        }
        Err(e) => error!("Connection failed: {}", e),
    }
    sleep(Duration::from_secs(5)).await;
}
```

### Rate Limiting
```rust
// Upbit allows 600 req/min for market endpoint
// At 2-second intervals = 30 req/min (safe)
// Add jitter to avoid thundering herd
let jitter = rand::random::<u64>() % 500; // 0-500ms
sleep(Duration::from_millis(2000 + jitter)).await;
```

---

## PHASE 10: TESTING STRATEGY

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_keyword_filtering() {
        assert!(is_listing("원화 마켓 디지털 자산 추가"));
        assert!(!is_listing("디지털 자산 입출금 일시 중단"));
    }
    
    #[test]
    fn test_token_extraction() {
        let info = parse_listing("SOL 거래 지원");
        assert_eq!(info.token_symbol, "SOL");
    }
}
```

### Integration Tests
```bash
# Test against real Upbit API (safe endpoints)
cargo test --test integration_tests
```

### Load Tests
```bash
# Simulate 24 hours of operation
# Check memory doesn't leak
# Verify WebSocket stays connected
```

---

## SUCCESS METRICS

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Detection Latency (Market API)** | <3s | Time from listing live to alert sent |
| **Detection Latency (Notice)** | <5s | Time from notice posted to alert sent |
| **Uptime** | >99.9% | systemd status over 30 days |
| **False Positive Rate** | <5% | Manual verification of alerts |
| **False Negative Rate** | 0% | Must catch ALL listings |
| **Memory Usage** | <50MB | htop monitoring |
| **CPU Usage** | <10% | htop monitoring |

---

## RISK MITIGATION

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Upbit changes notice page structure | Medium | High | Monitor for 404s, have fallback to market API |
| IP banned for scraping | Low | High | Use residential proxy, respect rate limits |
| Redis crashes | Low | Medium | Auto-restart systemd service, local cache fallback |
| WebSocket disconnects | Medium | Low | Auto-reconnect logic, API polling backup |
| Server goes down | Low | High | Set up health check monitoring, auto-restart |

---

## FINAL CHECKLIST

Before going live:
- [ ] All tests passing
- [ ] Redis connection stable
- [ ] Telegram bot configured and tested
- [ ] Notice API endpoint discovered and working
- [ ] Deployed to Seoul-based server
- [ ] Systemd service enabled
- [ ] 24-hour test run completed
- [ ] Monitoring dashboard set up
- [ ] Backup alerting method configured
- [ ] Documentation complete

---

## SUPPORT & MAINTENANCE

**Daily:**
- Check logs for errors: `journalctl -u upbit-detector --since today`
- Verify alerts are being sent

**Weekly:**
- Review detection latency metrics
- Check for false positives
- Update keyword database if needed

**Monthly:**
- Review overall uptime
- Optimize polling intervals if needed
- Update dependencies

---

**Project Timeline:** 4-5 days  
**Estimated Cost:** $15-20/month  
**Expected Uptime:** >99.9%  
**Detection Latency:** 1-5 seconds
