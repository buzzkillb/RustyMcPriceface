# Multi-Crypto Discord Price Bot (Shared Setup)

This setup allows you to run multiple Discord bots for different cryptocurrencies while using a single price service to minimize API calls.

## Architecture

- **1 Price Service**: Fetches all crypto prices and writes to `shared/prices.json`
- **4 Discord Bots**: Each bot reads its assigned crypto price from the shared file
- **Shared Data**: All bots use the same price data, no duplicate API calls

## Setup

### 1. Create Discord Bots

Create 4 Discord applications at https://discord.com/developers/applications:
- BTC Bot
- ETH Bot  
- SOL Bot
- USDC Bot

### 2. Configure Environment

Copy `env.example` to `.env` and fill in your bot tokens:

```env
# Price update interval (shared by all services)
UPDATE_INTERVAL_SECONDS=12

# Discord Bot Tokens (one for each bot)
DISCORD_TOKEN_BTC=your_btc_bot_token_here
DISCORD_TOKEN_ETH=your_eth_bot_token_here
DISCORD_TOKEN_SOL=your_sol_bot_token_here
DISCORD_TOKEN_USDC=your_usdc_bot_token_here
```

### 3. Run with Docker Compose

```bash
# Start all services
docker-compose up -d

# Check logs
docker-compose logs -f price-service
docker-compose logs -f btc-bot

# Stop all services
docker-compose down
```

### 4. Run from Command Line

**Terminal 1 - Price Service:**
```bash
cargo run --bin price-service
```

**Terminal 2-5 - Discord Bots:**
```bash
CRYPTO_NAME=BTC DISCORD_TOKEN=your_btc_token cargo run --bin discord-bot
CRYPTO_NAME=ETH DISCORD_TOKEN=your_eth_token cargo run --bin discord-bot
CRYPTO_NAME=SOL DISCORD_TOKEN=your_sol_token cargo run --bin discord-bot
CRYPTO_NAME=USDC DISCORD_TOKEN=your_usdc_token cargo run --bin discord-bot
```

## Benefits

- ✅ **1 API call** instead of 4 API calls
- ✅ **Real-time updates** for all bots
- ✅ **Easy scaling** - add more bots without increasing API load
- ✅ **Fault tolerance** - if one bot fails, others keep working
- ✅ **Simple debugging** - check `shared/prices.json` for current prices

## File Structure

```
pbot/
├── docker-compose.yml
├── .env
├── shared/
│   └── prices.json (created by price-service)
├── src/
│   ├── main.rs (discord bot code)
│   └── price_service.rs (price fetching service)
└── Cargo.toml
```

## Adding More Cryptos

1. Add the crypto to `price_service.rs` in the `PricesFile` struct
2. Add the feed ID to `get_feed_ids()` function
3. Add a new bot service to `docker-compose.yml`
4. Add the bot token to `.env`

## Monitoring

- Check `shared/prices.json` for current prices
- Use `docker-compose logs` to monitor each service
- Price service logs show API fetch status
- Bot logs show Discord update status 