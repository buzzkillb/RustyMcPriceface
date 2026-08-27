# RustyMcPriceface

Discord bot for tracking cryptocurrency and asset prices with beautiful charts.

## Features

- Multiple independent bot instances, one per ticker
- Real-time price updates via Pyth Network, Yahoo Finance, GoldSilver.ai, and DexScreener
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
| Any DexScreener pair (e.g. CYB, Solana altcoins) | DexScreener |

## Adding a DexScreener Token

DexScreener is used for tokens that aren't listed on Pyth Network — Solana altcoins, pump.fun tokens, etc.

**1. Get the pair address**

Open any token's page on DexScreener and copy the pair part of the URL. The URL format is:

```
https://dexscreener.com/<chain>/<pair_address>
        e.g. https://dexscreener.com/solana/chvehkrbncdpdr1od9eya1vp635wwfdzgxdzexxt6v96
```

Here `solana` is the chain and `chvehkrbncdpdr1od9eya1vp635wwfdzgxdzexxt6v96` is the pair address. You need **both**.

**2. Add a bot token**

```bash
DISCORD_TOKEN_CYB=your_cyb_bot_token_here
```

**3. Add the pair to `DEXSCREENER_FEEDS`**

Add an entry in the format `TICKER:<chain>/<pair_address>`:

```bash
DEXSCREENER_FEEDS=CYB:solana/chvehkrbncdpdr1od9eya1vp635wwfdzgxdzexxt6v96
```

Add multiple pairs by separating with commas:

```bash
DEXSCREENER_FEEDS=CYB:solana/<pair_address>,FOO:ethereum/<pair_address>
```

**4. Rebuild**

```bash
docker-compose up -d --build
```

The bot will automatically pick up the new ticker and start showing its price (including BTC/ETH/SOL conversions and charts, once it has collected enough history).

## Environment Variables

```bash
# Bot tokens - one per ticker
DISCORD_TOKEN_BTC=your_token
DISCORD_TOKEN_CYB=your_token

# Pyth feed IDs
CRYPTO_FEEDS=BTC:feed_id,ETH:feed_id,SOL:feed_id

# Pyth API key (required) - get one from https://pythdata.app
# Keep this secret / only in .env, never commit it
PYTH_API_KEY=your_pyth_api_key_here

# DexScreener pairs (for tokens not on Pyth): TICKER:<chain>/<pair_address>
DEXSCREENER_FEEDS=CYB:solana/chvehkrbncdpdr1od9eya1vp635wwfdzgxdzexxt6v96

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
