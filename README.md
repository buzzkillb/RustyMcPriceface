# RustyMcPriceface

Discord bot for tracking cryptocurrency and asset prices with beautiful charts.

## Features

- Multiple independent bot instances, one per ticker
- Real-time price updates via Pyth Network, Yahoo Finance, and GoldSilver.ai
- Discord nicknames display ticker + current price
- Status cycles through BTC/ETH/SOL conversions and 1h change
- Historical price charts with high/low markers
- Detailed price embeds with 24h/7d/30d percentage changes
- PostgreSQL for persistent price history
- Lightweight Alpine-based Docker image (~266MB)

## Quick Start

```bash
cp .env.example .env
# Edit .env with your Discord bot tokens
docker-compose up -d --build
```

## Slash Commands

### /chart price
Generate a price chart with high/low markers and percentage change.

```
/chart price timeframe:2w
```

| Option | Default | Examples |
|--------|---------|----------|
| timeframe | 24h | 1h, 6h, 12h, 24h, 48h, 1w, 2w, 30d, 3m |

### /price current
Display current price with conversions and percentage changes.

```
/price current
/price current crypto:ETH
```

Shows USD price, 24h/7d/30d changes, and BTC/ETH/SOL conversions.

## Supported Tickers

| Ticker | Source |
|--------|--------|
| BTC, ETH, SOL, and other Pyth feeds | Pyth Network |
| DXY | Yahoo Finance |
| SSILVER | GoldSilver.ai |

## Environment Variables

```bash
# Bot tokens - one per ticker
DISCORD_TOKEN_BTC=your_token
DISCORD_TOKEN_ETH=your_token

# Pyth feed IDs
CRYPTO_FEEDS=BTC:feed_id,ETH:feed_id,SOL:feed_id

# Optional
UPDATE_INTERVAL_SECONDS=12
```

## Tech Stack

- Python 3.12 (Alpine)
- discord.py
- asyncpg / PostgreSQL
- matplotlib
- aiohttp
- Docker / Docker Compose

## Project Structure

```
├── bot.py            # Main bot, commands, status cycling
├── database.py       # PostgreSQL operations
├── price_service.py  # Price fetching
├── chart_service.py  # Chart generation
├── docker-compose.yml
├── Dockerfile
└── requirements.txt
```
