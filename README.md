# Multi-Crypto Discord Price Bot

A Discord bot that tracks cryptocurrency prices in real-time and displays them as custom status messages and nicknames. The bot supports multiple cryptocurrencies simultaneously and provides slash commands for price queries.

## Features

- 🚀 **Real-time price tracking** for multiple cryptocurrencies
- 📊 **Persistent price history** using SQLite database
- 🎯 **Custom Discord status** with rotating price displays
- 📈 **Percentage change tracking** (1h, 12h, 24h, 7d)
- 💱 **Cross-rate conversions** (BTC, ETH, SOL equivalents)
- ⚡ **Slash commands** for instant price queries
- 🔄 **Automatic data cleanup** (7-day retention)
- 🐳 **Docker Compose** for easy deployment

## Supported Cryptocurrencies

- **BTC** (Bitcoin)
- **ETH** (Ethereum) 
- **SOL** (Solana)
- **WIF** (dogwifhat)
- **FARTCOIN** (Custom token)
- *And more...*

## Quick Start

### Prerequisites

- Docker and Docker Compose
- Discord Bot Token(s)
- Pyth Network Feed IDs (for direct API access)

### 1. Clone and Setup

```bash
git clone <your-repo-url>
cd pbot
cp env.example .env
```

### 2. Configure Environment Variables

Edit `.env` file with your Discord bot tokens and settings:

```bash
# Update intervals (in seconds)
UPDATE_INTERVAL_SECONDS=12

# Discord Bot Tokens (one per crypto)
DISCORD_TOKEN_SOL=your_sol_bot_token_here
DISCORD_TOKEN_BTC=your_btc_bot_token_here
DISCORD_TOKEN_ETH=your_eth_bot_token_here
DISCORD_TOKEN_WIF=your_wif_bot_token_here
DISCORD_TOKEN_FARTCOIN=your_fartcoin_bot_token_here

# Pyth Network Feed IDs (optional, for direct API access)
# Get feed IDs from: https://pyth.network/price-feeds
CRYPTO_FEEDS=BTC:your_btc_feed_id_here,ETH:your_eth_feed_id_here,SOL:your_sol_feed_id_here,WIF:your_wif_feed_id_here
```

### 3. Build and Run

```bash
# Build the release binaries
cargo build --release

# Build Docker images
docker-compose build

# Start all services
docker-compose up -d
```

### 4. Verify Installation

Check that all services are running:

```bash
docker-compose ps
```

You should see:
- `pbot-price-service-1` - Price aggregation service
- `pbot-sol-bot-1` - Solana price bot
- `pbot-btc-bot-1` - Bitcoin price bot
- `pbot-eth-bot-1` - Ethereum price bot
- `pbot-wif-bot-1` - WIF price bot
- `pbot-fartcoin-bot-1` - Fartcoin price bot

## Adding New Cryptocurrencies

### Example: Adding WIF (dogwifhat)

#### 1. Create Discord Bot

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Create a new application
3. Go to "Bot" section and create a bot
4. Copy the bot token
5. Enable "Message Content Intent" and "Server Members Intent"
6. Invite the bot to your server with appropriate permissions

#### 2. Update Environment Variables

Add the new bot token to your `.env` file:

```bash
DISCORD_TOKEN_WIF=your_wif_bot_token_here
```

#### 3. Add to Docker Compose

Add a new service to `docker-compose.yml`:

```yaml
services:
  # ... existing services ...
  
  wif-bot:
    build: .
    volumes:
      - ./shared:/app/shared
    environment:
      - DISCORD_TOKEN=${DISCORD_TOKEN_WIF}
      - CRYPTO_NAME=WIF
      - UPDATE_INTERVAL_SECONDS=${UPDATE_INTERVAL_SECONDS:-12}
    command: ["./discord-bot"]
    depends_on:
      - price-service
    restart: unless-stopped
```

#### 4. Update Price Service Feeds

Add the WIF feed ID to the `CRYPTO_FEEDS` environment variable in `docker-compose.yml`:

```yaml
services:
  price-service:
    build: .
    volumes:
      - ./shared:/app/shared
    environment:
      - UPDATE_INTERVAL_SECONDS=${UPDATE_INTERVAL_SECONDS:-12}
      - CRYPTO_FEEDS=${CRYPTO_FEEDS:-BTC:your_btc_feed_id_here,ETH:your_eth_feed_id_here,SOL:your_sol_feed_id_here,WIF:your_wif_feed_id_here}
    command: ["./price-service"]
    restart: unless-stopped
```

#### 5. Rebuild and Restart

```bash
# Rebuild with new configuration
docker-compose build

# Restart all services
docker-compose down
docker-compose up -d
```

### Finding Pyth Feed IDs

To find the correct feed ID for a cryptocurrency:

1. Visit [Pyth Network Price Feeds](https://pyth.network/price-feeds)
2. Search for your cryptocurrency
3. Copy the feed ID (the long hexadecimal string)

## Bot Features

### Discord Status Display

Each bot displays:
- **Nickname**: Shows current price (e.g., "SOL $95.42")
- **Custom Status**: Rotates between:
  - Percentage change over 1 hour
  - BTC equivalent amount
  - ETH equivalent amount  
  - SOL equivalent amount

### Slash Commands

Use `/price [crypto]` to get current price information:

```
/price SOL
```

Response example:
```
📊 SOL: $95.42 📈 +2.15% (1h) | 📈 +5.23% (12h) | 📉 -1.45% (24h)
💱 Also: 0.0021 BTC | 0.034 ETH | 1.000 SOL
```

### Price History

The bot maintains persistent price history in SQLite:
- **Automatic cleanup**: Removes data older than 7 days
- **Percentage calculations**: Based on actual historical data
- **Cross-restart persistence**: History survives bot restarts

## Development

### Local Development

```bash
# Install Rust dependencies
cargo build

# Run price service
cargo run --bin price-service

# Run Discord bot (in another terminal)
cargo run --bin discord-bot
```

### Database Queries

Query the SQLite database directly:

```bash
# View latest prices
cargo run --bin db-query

# Or use the provided script
./query-db.sh
```

### Logs and Monitoring

```bash
# View all logs
docker-compose logs -f

# View specific service logs
docker-compose logs -f sol-bot

# Check service status
docker-compose ps
```

## Configuration Options

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `UPDATE_INTERVAL_SECONDS` | How often to update prices | `12` |
| `DISCORD_TOKEN_*` | Discord bot tokens | Required |
| `CRYPTO_FEEDS` | Pyth Network feed IDs | Optional |
| `CRYPTO_NAME` | Cryptocurrency symbol | Required |

### Update Intervals

- **12 seconds**: Real-time updates (default)
- **30 seconds**: Balanced performance
- **60 seconds**: Lower resource usage

## Troubleshooting

### Common Issues

**Bot not responding to slash commands:**
- Ensure bot has proper permissions
- Check that slash commands are registered globally
- Verify bot token is correct

**No price data:**
- Check if price-service is running
- Verify Pyth feed IDs are correct
- Check network connectivity

**Database errors:**
- Ensure `shared/` directory exists and is writable
- Check disk space
- Verify SQLite permissions

### Debug Commands

```bash
# Check container logs
docker-compose logs [service-name]

# Access container shell
docker-compose exec [service-name] /bin/bash

# View database contents
docker-compose exec price-service ./db-query

# Restart specific service
docker-compose restart [service-name]
```

## Architecture

### Components

1. **Price Service** (`price-service`): Aggregates prices from Pyth Network
2. **Discord Bots** (`*-bot`): Individual bots for each cryptocurrency
3. **Shared Database** (`shared/prices.db`): SQLite database for price history
4. **Shared Files** (`shared/prices.json`): JSON cache for current prices

### Data Flow

```
Pyth Network → Price Service → SQLite DB + JSON Cache → Discord Bots → Discord
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly
5. Submit a pull request

## License

[Your License Here]

## Support

For issues and questions:
- Create an issue on GitHub
- Check the troubleshooting section
- Review the logs for error messages 