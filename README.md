# RustyMcPriceface

Discord bot that tracks cryptocurrency and asset prices using Pyth Network and posts updates to Discord channels.

## Prerequisites

- Docker and Docker Compose
- A Discord application with bot tokens

## Setup

1. Copy the example environment file:
   ```
   cp .env.example .env
   ```

2. Edit `.env` and add your Discord bot tokens. Get tokens from https://discord.com/developers/applications

3. The `CRYPTO_FEEDS` variable controls which assets to track. The defaults are:
   - BTC (Bitcoin)
   - ETH (Ethereum)
   - SOL (Solana)
   - DOGE (Dogecoin)
   - DXY (US Dollar Index)

## Running

### With Docker Compose

```
docker-compose up -d
```

The bot will start and begin posting price updates to your configured Discord channels.

### Check Status

```
docker-compose ps
docker-compose logs -f
```

### Stop

```
docker-compose down
```

## Updating Prices

Edit `.env` and change `UPDATE_INTERVAL_SECONDS` (default 30 seconds).

## Adding New Assets

1. Add a new bot token in `.env`: `DISCORD_TOKEN_ASSETNAME=your_token`
2. Add the Pyth Network feed ID in `CRYPTO_FEEDS`: `ASSETNAME:feed_id`
3. Rebuild: `docker-compose up -d --build`
