# 🚀 Multi-Cryptocurrency Discord Bot System

A powerful Rust-based Discord bot system that tracks real-time cryptocurrency prices from Pyth Network's Hermes API. Features multiple Discord bot instances, SQLite price storage, and dynamic slash commands.

## ✨ Features

### 🔄 Real-Time Price Tracking
- **Live Price Updates**: Fetches prices every 12 seconds from Pyth Network
- **Multi-Crypto Support**: Track any cryptocurrency with Pyth feed IDs
- **Dynamic Configuration**: Add new cryptos via environment variables
- **Cross-Crypto Conversions**: Shows prices in terms of BTC, ETH, and SOL

### 🤖 Discord Bot Features
- **Multiple Bot Instances**: Separate bots for each cryptocurrency
- **Dynamic Nicknames**: Bot nicknames update with current prices
- **Rotating Status**: Custom status shows price trends and conversions
- **Slash Commands**: `/price` command with 1h, 12h, and 24h price changes
- **Smart Defaults**: Commands default to the bot's own cryptocurrency

### 💾 Data Storage
- **SQLite Database**: 30-day price history storage
- **Automatic Cleanup**: Removes old data automatically
- **Query Tools**: Built-in database query utilities
- **Price Statistics**: Track price changes over multiple timeframes

## 🚀 Quick Start

### 1. Clone and Setup
```bash
git clone https://github.com/buzzkillb/RustyMcPriceface.git
cd RustyMcPriceface
```

### 2. Environment Configuration
```bash
cp env.example .env
```

Edit `.env` with your configuration:
```env
# Discord Bot Tokens
DISCORD_TOKEN_SOL=your_sol_bot_token
DISCORD_TOKEN_BTC=your_btc_bot_token
DISCORD_TOKEN_ETH=your_eth_bot_token
DISCORD_TOKEN_FARTCOIN=your_fartcoin_bot_token

# Cryptocurrency Feed IDs
CRYPTO_FEEDS=BTC:0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43,ETH:0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace,SOL:0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d,FARTCOIN:your_feed_id

UPDATE_INTERVAL_SECONDS=12
```

### 3. Build and Deploy
```bash
cargo build --release
docker-compose up -d
```

## 🎮 Usage

### Discord Slash Commands

#### `/price` Command
- **`/price`** - Shows price for the bot's own cryptocurrency
- **`/price crypto:BTC`** - Shows Bitcoin price
- **`/price crypto:FARTCOIN`** - Shows Fartcoin price

**Example Output:**
```
🪙 BTC: $107,803 📈 +1.23% (1h) | 📉 -2.34% (12h) | 📈 +0.56% (24h)
💱 Also: 43.9982 ETH | 721.2803 SOL
```

### Database Queries
```bash
# Check database stats
docker exec pbot-price-service-1 /app/db-query stats

# Get latest price
docker exec pbot-price-service-1 /app/db-query latest BTC
```

## 🔧 Adding New Cryptocurrencies

1. **Get Pyth Feed ID** from [Pyth Network](https://pyth.network/price-feeds)
2. **Add to CRYPTO_FEEDS** environment variable
3. **Create Discord bot** and add token to `.env`
4. **Add service** to `docker-compose.yml`
5. **Restart services**: `docker-compose up -d`

## 🛠️ Development

### Building
```bash
# Build all binaries
cargo build --release

# Rebuild Docker images
docker-compose build --no-cache

# Restart services
docker-compose restart
```

### Logs
```bash
# View all logs
docker-compose logs -f

# Check specific service
docker-compose logs sol-bot
```

## 🔍 Troubleshooting

### Common Issues
- **Bot not responding**: Check logs for command registration
- **Price service down**: Verify API connectivity
- **Database issues**: Check file permissions and disk space

### Health Checks
```bash
# Service status
docker-compose ps

# Database health
docker exec pbot-price-service-1 /app/db-query stats
```

## 📊 Architecture

```
Price Service → SQLite DB → Discord Bots
     ↓              ↓           ↓
  Pyth API    Shared JSON   Slash Commands
```

## 🔒 Security

- Never commit `.env` files
- Use separate Discord tokens per bot
- Restrict database file permissions
- Run behind firewall/proxy

## 📄 License

MIT License - see LICENSE file for details.

---

**Happy Trading! 📈💰** 