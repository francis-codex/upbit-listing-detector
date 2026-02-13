# Upbit Listing Detector

Real-time cryptocurrency listing detection system for the Upbit exchange. Monitors multiple data sources and sends alerts via Telegram within seconds of a new listing.

## Detection Paths

| Path | Source | Latency | What it catches |
|------|--------|---------|-----------------|
| **Market API** | `GET /v1/market/all` | 1–3 s | New trading pairs going live |
| **WebSocket** | `wss://api.upbit.com/websocket/v1` | 1–2 s | New market codes in ticker stream |
| **Notice Board** | Reverse-engineered API | 2–5 s | Listing announcements (1–3 h advance) |

## Quick Start

### Prerequisites

- Rust 1.75+ (`rustup` recommended)
- Redis 6+
- A Telegram bot token and chat ID

### 1. Clone and configure

```bash
git clone <repo-url>
cd upbit-listing-detector
cp .env.example .env
# Edit .env with your Telegram credentials
```

### 2. Run locally

```bash
# Start Redis
redis-server &

# Run the detector
cargo run
```

### 3. Production build

```bash
cargo build --release
# Binary at: target/release/upbit-listing-detector
```

## Configuration

Configuration is loaded from `config.toml` with environment variable overrides:

| Env Variable | Config Key | Description |
|---|---|---|
| `TELEGRAM_BOT_TOKEN` | `telegram.bot_token` | Telegram Bot API token (required) |
| `TELEGRAM_CHAT_ID` | `telegram.chat_id` | Telegram chat/channel ID (required) |
| `UPBIT_NOTICE_API` | `api.notice_endpoint` | Notice board API endpoint |
| `REDIS_URL` | `redis.url` | Redis connection URL |
| `DISCORD_WEBHOOK_URL` | `discord.webhook_url` | Optional Discord webhook |
| `RUST_LOG` | — | Log level (`trace`, `debug`, `info`, `warn`, `error`) |

### Finding the Notice API Endpoint

The notice board does not have an official API. To find the endpoint:

1. Open Chrome DevTools → **Network** tab
2. Visit [https://upbit.com/service_center/notice](https://upbit.com/service_center/notice)
3. Filter by **Fetch/XHR** requests
4. Look for a request returning JSON with notice data
5. Copy that URL into `UPBIT_NOTICE_API` or `config.toml`

The system works without the notice endpoint — Market API and WebSocket detection will still function.

## Deployment

### Docker Compose (recommended)

```bash
cp .env.example .env
# Edit .env with credentials
docker compose up -d
docker compose logs -f detector
```

### Bare metal + systemd

```bash
# Build release binary
cargo build --release

# Copy files to server
scp target/release/upbit-listing-detector user@server:/usr/local/bin/
scp .env config.toml user@server:/opt/upbit-detector/
scp systemd/upbit-detector.service user@server:/etc/systemd/system/

# On the server
sudo systemctl daemon-reload
sudo systemctl enable --now upbit-detector
sudo journalctl -u upbit-detector -f
```

### AWS EC2 (Seoul region)

For lowest latency to Upbit servers, deploy to `ap-northeast-2` (Seoul):

```bash
# Recommended: t3.micro (1 vCPU, 1 GB RAM) — ~$7.50/month
# Install Redis
sudo apt update && sudo apt install -y redis-server
sudo systemctl enable redis-server
```

## Architecture

```
┌─────────────────────────────────────────┐
│           Main Process (Tokio)          │
├──────────┬──────────┬───────────────────┤
│ Market   │ WebSocket│ Notice Board      │
│ API Poll │ Monitor  │ Scraper           │
│ (2s)     │ (live)   │ (3s)              │
└────┬─────┴────┬─────┴────┬──────────────┘
     │          │          │
     └──────────┼──────────┘
                │
      ┌─────────▼─────────┐
      │  Detection Engine  │
      │  + Keyword Filter  │
      │  + Token Parser    │
      └─────────┬─────────┘
                │
      ┌─────────▼─────────┐
      │    Redis Cache     │
      │  (deduplication)   │
      └─────────┬─────────┘
                │
      ┌─────────▼─────────┐
      │ Telegram / Discord │
      └───────────────────┘
```

## Keyword Filtering

The system uses a three-stage filter:

1. **Exclusion** — Rejects notices about maintenance, delisting, events
2. **Primary** — Requires at least one listing-related keyword (Korean or English)
3. **Secondary** — Boosts confidence for additional listing-related terms

Minimum confidence threshold: **0.6** (configurable).

## Monitoring

Check logs:
```bash
# systemd
sudo journalctl -u upbit-detector --since today

# Docker
docker compose logs -f detector
```

Health indicators in logs:
- `Market API detector starting` — market polling is running
- `WebSocket connected` — WebSocket is live
- `Notice board detector starting` — notice scraping is active
- `Fetched markets` (debug) — each successful API call

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Failed to connect to Redis" | Redis not running | `sudo systemctl start redis` |
| "TELEGRAM_BOT_TOKEN must be set" | Missing credentials | Set in `.env` or `config.toml` |
| "Notice endpoint is not configured" | No notice URL | See "Finding the Notice API Endpoint" |
| "Market API request failed" | Network/Upbit issue | Auto-retries; check connectivity |
| "WebSocket error, reconnecting" | Connection dropped | Auto-reconnects every 5s |
| No alerts received | Bot not started | Send `/start` to your bot in Telegram |

## License

MIT License — see [LICENSE](LICENSE) for details.
