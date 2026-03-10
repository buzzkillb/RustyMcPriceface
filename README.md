# RustyMcPriceface

Discord bot that tracks cryptocurrency and asset prices using Pyth Network and posts updates to Discord channels.

## Architecture

```
                    Pyth Network
                          |
                    Price Service
                          |
                    shared/prices.json
                          |
         +----------------+----------------+
         |                |                |
      Bot BTC         Bot ETH         Bot SOL
      (token)         (token)         (token)
         |                |                |
    +----+----+     +----+----+     +----+----+
    |         |     |         |     |         |
 Discord   SQLite  Discord   SQLite  Discord   SQLite
```

## How It Works

1. Price Service fetches prices from Pyth Network every 30 seconds
2. Prices are written to shared JSON file and SQLite database
3. Each bot instance reads prices and updates its Discord nickname
4. Multiple bot instances run in parallel, one per token

Each bot is independent - add more by adding tokens to .env.

## Prerequisites

- Docker and Docker Compose
- A Discord application with bot tokens

## Setup

1. Copy the example environment file:
   ```
   cp .env.example .env
   ```

2. Edit `.env` and add your Discord bot tokens. Get tokens from https://discord.com/developers/applications

3. In the Discord developer portal for each bot:
   - Enable "Public Bot" 
   - Enable "Server Members Intent"
   - Enable "Message Content Intent"

4. Invite each bot to your server using the OAuth2 URL in the Discord developer portal

5. The CRYPTO_FEEDS variable controls which assets to track:
   ```
   CRYPTO_FEEDS=BTC:feed_id,ETH:feed_id,...
   ```
   Get feed IDs from https://pyth.network/docs/developers

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

### Check Bot Status in Discord
```
!status
```

### Health Check
The health endpoint is available at localhost:8080/health

## Configuration

### Update Interval
Edit `.env` and change `UPDATE_INTERVAL_SECONDS` (default 12 seconds):
```
UPDATE_INTERVAL_SECONDS=30
```

### Adding New Assets

1. Add a new bot token in `.env`: `DISCORD_TOKEN_ASSETNAME=your_token`
2. Add the Pyth Network feed ID in `CRYPTO_FEEDS`: `ASSETNAME:feed_id`
3. Rebuild: `docker-compose up -d --build`

### Adding More Bots

Simply add more tokens to `.env`:
```
DISCORD_TOKEN_BTC=your_btc_token
DISCORD_TOKEN_ETH=your_eth_token
DISCORD_TOKEN_NEWTICKER=your_new_token
```

The bot will automatically spawn new instances for each token.

## Bot Commands

- `!BTC` - Get BTC price
- `!ETH` - Get ETH price
- `/price` - Slash command for prices
- `!silverchart` - Get silver price chart
- `!status` - Check system status (BTC bot only)

## Tech Stack

- Rust (edition 2021)
- Serenity (Discord bot library)
- SQLite (database)
- Pyth Network (price feeds)
- Axum (HTTP server)
- Plotters (chart generation)
