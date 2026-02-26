# Upbit Listing Detector — User Guide

## What This System Does

This system watches the Upbit cryptocurrency exchange 24/7 and sends you a Telegram alert the moment a new coin is listed. It checks three different sources simultaneously so you hear about listings as fast as possible — typically within 1–5 seconds.

You don't need to do anything to keep it running. It runs automatically on a server and will message you in Telegram whenever something happens.

---

## What Alerts Look Like

### New Listing Alert

When a new coin is listed (or announced), you'll receive a message like this:

> **NEW UPBIT LISTING DETECTED**
>
> **Token:** ABC
> **Markets:** KRW, BTC, USDT
> **Trading Starts:** 2026-02-26 14:00 KST
> **Confidence:** 95%
>
> **Title:** [ABC] 신규 거래지원 안내
> **Source:** Notice Board
>
> Detected at: 2026-02-26 11:32:15 KST

**What each field means:**

| Field | Meaning |
|-------|---------|
| **Token** | The ticker symbol of the new coin (e.g. ABC) |
| **Markets** | Which trading pairs are available (KRW = Korean Won, BTC = Bitcoin, USDT = Tether) |
| **Trading Starts** | When you can actually start buying/selling (if known) |
| **Confidence** | How sure the system is that this is a real listing (higher = more certain) |
| **Source** | How the listing was detected — "Notice Board" means an official announcement; "Market API" or "WebSocket" means the trading pair already went live |

### New Market Alert

Sometimes the system detects a new trading pair going live before any announcement. That looks like:

> **NEW MARKET DETECTED ON UPBIT**
>
> **Market Code:** KRW-ABC
> **Korean Name:** 에이비씨
> **English Name:** ABC Token
>
> **Source:** Market API
> Detected at: 2026-02-26 14:00:03 KST

This means trading is already open — act immediately if you want to.

---

## Daily Health Report

Every day at **9:00 AM KST**, you'll receive a status message confirming the system is running:

> **Daily Status Report — 2026-02-26**
>
> **Uptime:** 72h 15m
> **Markets monitored:** 284
> **Market API polls:** 128,400
> **Notice board checks:** 85,200
> **WebSocket:** Connected
> **New listings today:** 0
>
> All systems operational.

**What to look for:**

- **Uptime** — How long the system has been running without a restart. Higher is better.
- **WebSocket: Connected** — This should always say "Connected." If it says "Reconnecting," the system is recovering automatically.
- **New listings today** — How many new listings were detected in the past 24 hours. Zero is normal on most days.

If you stop receiving this daily report, the system may be down — contact support.

---

## What To Do When You Get an Alert

1. **Check the source.** "Notice Board" alerts come 1–3 hours before trading starts, giving you time to prepare. "Market API" or "WebSocket" alerts mean the pair is already live.
2. **Check the confidence.** Anything 80% or above is very likely a real listing. Lower scores may be false positives.
3. **Check the markets.** KRW pairs tend to see the most volume on Upbit.
4. **Act according to your own strategy.** The system tells you *what* is being listed — the trading decision is yours.

---

## FAQ

**Q: How fast are the alerts?**
Most alerts arrive within 1–5 seconds of the listing appearing on Upbit. Notice board alerts arrive within 2–5 seconds of the announcement being published.

**Q: Can I get false alerts?**
Rarely. The system filters out maintenance notices, delistings, and unrelated announcements. The confidence score helps you judge — anything below 70% deserves extra caution.

**Q: I haven't received any alerts in days. Is it broken?**
Probably not. Upbit doesn't list new coins every day. As long as you're receiving the daily health report at 9:00 AM KST, the system is working normally.

**Q: What if I stop receiving the daily report?**
Contact support. The server may need to be restarted.

**Q: Can I receive alerts on Discord too?**
Yes, Discord alerts can be enabled as an additional channel. Contact support to set this up.

**Q: What timezone are the timestamps in?**
All times are in **KST** (Korea Standard Time, UTC+9), which is the same timezone Upbit operates in.
