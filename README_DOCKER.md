# SOL Price Bot - Docker Setup

This setup runs a SOL price bot using Docker with a shared price service.

## Quick Start

### 1. Create Discord Bot
1. Go to https://discord.com/developers/applications
2. Create a new application
3. Add a bot to your application
4. Copy the bot token

### 2. Configure Environment
```bash
# Copy the example environment file
cp env.example .env

# Edit .env and add your bot token
nano .env
```

Your `.env` file should look like:
```env
UPDATE_INTERVAL_SECONDS=12
DISCORD_TOKEN_SOL=your_actual_bot_token_here
```

### 3. Run with Docker Compose
```bash
# Build and start the services
docker-compose up -d

# Check logs
docker-compose logs -f price-service
docker-compose logs -f sol-bot

# Stop services
docker-compose down
```

## What This Does

**Price Service:**
- Fetches BTC, ETH, SOL prices from Pyth Network
- Writes to `shared/prices.json` every 12 seconds
- Single API call for all 3 cryptos

**SOL Bot:**
- Reads SOL price from `shared/prices.json`
- Updates Discord nickname with SOL price
- Shows status rotation: SOL change %, BTC amount, ETH amount, SOL amount
- No API calls needed (uses shared data)

## File Structure
```
pbot/
├── docker-compose.yml
├── Dockerfile
├── .env
├── shared/
│   └── prices.json (created by price-service)
├── src/
│   ├── main.rs (discord bot)
│   └── price_service.rs (price service)
└── Cargo.toml
```

## Monitoring

**Check if services are running:**
```bash
docker-compose ps
```

**View logs:**
```bash
# All services
docker-compose logs

# Specific service
docker-compose logs -f sol-bot
```

**Check shared prices:**
```bash
cat shared/prices.json
```

## Troubleshooting

**Bot not updating:**
- Check if price service is running: `docker-compose logs price-service`
- Verify bot token in `.env`
- Check if bot has permissions in Discord server

**Price service failing:**
- Check internet connection
- Verify Pyth API is accessible
- Check logs: `docker-compose logs price-service`

## Adding More Bots Later

To add BTC and ETH bots later, just add them to `docker-compose.yml`:

```yaml
btc-bot:
  build: .
  volumes:
    - ./shared:/app/shared
    - ./.env:/app/.env
  environment:
    - DISCORD_TOKEN=${DISCORD_TOKEN_BTC}
    - CRYPTO_NAME=BTC
  depends_on:
    - price-service
  restart: unless-stopped
``` 