# RustyMcPriceface

Discord bot that tracks cryptocurrency and asset prices. Each bot displays its ticker's price and cycles through BTC/ETH/SOL conversions plus 1-hour percentage change.

## Architecture

```
Pyth Network / Yahoo Finance / GoldSilver.ai
                    |
              Price Service
                    |
              PostgreSQL Database
                    |
    +-------------+-------------+-------------+
    |             |             |             |
  Bot BTC      Bot ETH       Bot SOL    Bot TICKER
```

## Features

- Multiple bot instances run in parallel, one per ticker
- Prices fetched from Pyth Network (crypto), Yahoo Finance (indices), GoldSilver.ai (metals)
- Discord nicknames show ticker and current price
- Status cycles through: BTC value, ETH value, SOL value, 1-hour change
- All prices stored in PostgreSQL for historical tracking
- Each bot is independent - add more by adding tokens to .env
- Slash commands for charts and detailed price info
- Beautiful dark-themed charts with high/low markers

## Prerequisites

- Docker and Docker Compose
- Discord applications with bot tokens

## Setup

1. Copy the example environment file:
   ```
   cp .env.example .env
   ```

2. Edit `.env` and add your Discord bot tokens. Get tokens from https://discord.com/developers/applications

3. In the Discord developer portal for each bot:
   - Enable "Public Bot"
   - Enable necessary intents for your bot features

4. Invite each bot to your server using the OAuth2 URL in the Discord developer portal

## Environment Variables

### Bot Tokens
```
DISCORD_TOKEN_BTC=your_btc_bot_token
DISCORD_TOKEN_ETH=your_eth_bot_token
DISCORD_TOKEN_SOL=your_sol_bot_token
```

### Price Feed Configuration
```
CRYPTO_FEEDS=BTC:feed_id,ETH:feed_id,SOL:feed_id,...
```
Get Pyth Network feed IDs from https://insights.pyth.network/price-feeds

### Special Tickers
- DXY: Fetched via Yahoo Finance (DX-Y.NYB)
- SSILVER: Fetched via GoldSilver.ai scraping
- GOLD, SILVER: Fetched via Pyth Network

### Optional Settings
```
UPDATE_INTERVAL_SECONDS=12
DATABASE_URL=postgresql://postgres:postgres@postgres:5432/pricebot
```

## Running

### Start
```
docker-compose up -d --build
```

### Check Status
```
docker-compose ps
docker-compose logs -f
```

### Stop
```
docker-compose down
```

## Bot Status Display

Each bot cycles its status every update interval:
- Nickname: `BTC $67,432`
- Status cycles: `0.030582 BTC` -> `3.421 ETH` -> `285.67 SOL` -> `+1.24% (1h)` -> repeat

The bot skips showing its own ticker in the conversion (BTC bot shows ETH/SOL/1h%, not BTC).

## Slash Commands

Each bot supports the following slash commands:

### /chart price
Generates a price chart from historical data.

**Usage:** `/chart price hours:24`

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| hours  | int  | 24      | Number of hours of data to display (24, 48, 168, 720, etc.) |

**Available for:** All bots (shows chart for the bot's specific ticker)

### /price current
Shows the current price with conversions and percentage changes.

**Usage:** `/price current` or `/price current crypto:BTC`

When called without a crypto argument, defaults to the bot's own ticker (e.g., ETH bot shows ETH price by default).

Displays:
- Current USD price
- 24h, 7d, 30d percentage changes (green/red indicators)
- Conversions to BTC, ETH, SOL (if applicable)

**Available for:** All bots

## Adding New Bots

Add more tokens to `.env`:
```
DISCORD_TOKEN_NEWTICKER=your_new_token
```

The bot will automatically spawn a new instance for each token.

## Tech Stack

- Docker with Docker Compose
- Python 3.12
- PostgreSQL (asyncpg)
- discord.py (Discord API)
- aiohttp (HTTP client)
- matplotlib (chart generation)
- Pyth Network (price feeds)
- Yahoo Finance (DXY index)
- GoldSilver.ai (Shanghai Silver)

## Project Structure

```
.
├── bot.py              # Main bot with Discord integration
├── database.py         # PostgreSQL operations
├── price_service.py    # Price fetching from various sources
├── chart_service.py     # Chart generation with matplotlib
├── docker-compose.yml   # Container orchestration
├── Dockerfile          # Python container image
├── requirements.txt    # Python dependencies
└── .env.example        # Environment variable template
```

## Database

Prices are stored in PostgreSQL with the following schema:

- `prices`: ticker, price, timestamp
- `price_aggregates`: aggregated price data for historical queries

The database container stores data in a named volume to persist across restarts.
