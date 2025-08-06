# 🚀 Multi-Crypto Discord Price Bot

A high-performance Discord bot system built in Rust that tracks cryptocurrency prices in real-time. Each cryptocurrency gets its own dedicated bot instance with live price updates, Discord status displays, and interactive slash commands.

## ✨ Features

### 🎯 **Core Functionality**
- **Real-time Price Tracking**: Updates every 12 seconds from Pyth Network
- **Multi-Bot Architecture**: Dedicated bot instance per cryptocurrency
- **Live Discord Integration**: Dynamic nicknames and status messages
- **Interactive Commands**: Slash commands for instant price queries
- **Historical Data**: SQLite database with price history and trends

### � **PriceC Display**
- **Dynamic Nicknames**: Shows current price (e.g., "BTC $67,234")
- **Rotating Status**: Cycles through percentage changes and cross-rates
- **Trend Indicators**: Visual arrows (📈📉) for price movements
- **Multi-Timeframe**: 1h, 12h, 24h, 7d percentage changes
- **Cross-Rate Conversion**: BTC, ETH, SOL equivalent values

### 🛠️ **Technical Features**
- **Alpine Linux**: Optimized containers (~115MB each)
- **Health Monitoring**: Built-in health check endpoints
- **Auto-Recovery**: Comprehensive error handling and retry logic
- **Rate Limiting**: Discord API protection with exponential backoff
- **Data Persistence**: SQLite with automatic cleanup
- **Resource Efficient**: ~3-6MB RAM per bot instance

## 🏗️ **Architecture**

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Pyth Network  │───▶│  Price Service   │───▶│ Shared Database │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                                │                        │
                                ▼                        ▼
                       ┌─────────────────┐    ┌─────────────────┐
                       │ JSON Price Cache│    │   SQLite DB     │
                       └─────────────────┘    └─────────────────┘
                                │                        │
                                ▼                        ▼
                    ┌─────────────────────────────────────────────┐
                    │           Discord Bots                      │
                    │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐   │
                    │  │ BTC │ │ ETH │ │ SOL │ │ ... │ │ ... │   │
                    │  └─────┘ └─────┘ └─────┘ └─────┘ └─────┘   │
                    └─────────────────────────────────────────────┘
```

### **Components**

1. **Price Service**: Fetches and aggregates price data from Pyth Network
2. **Discord Bots**: Individual bot instances for each cryptocurrency
3. **Shared Database**: SQLite database for price history and calculations
4. **Health Monitoring**: HTTP endpoints for service health checks
5. **Database Cleanup**: Automated data retention management

## 🚀 **Quick Start**

### **Prerequisites**

- Docker and Docker Compose
- Discord Developer Account
- Basic knowledge of environment variables

### **1. Clone Repository**

```bash
git clone <repository-url>
cd crypto-discord-bot
```

### **2. Create Environment File**

```bash
cp .env.example .env
```

Edit `.env` with your configuration:

```bash
# Update interval (seconds)
UPDATE_INTERVAL_SECONDS=12

# Discord Bot Tokens (create one bot per crypto)
DISCORD_TOKEN_BTC=your_btc_bot_token_here
DISCORD_TOKEN_ETH=your_eth_bot_token_here
DISCORD_TOKEN_SOL=your_sol_bot_token_here
# Add more tokens as needed...

# Optional: Pyth Network Feed IDs for direct API access
CRYPTO_FEEDS=BTC:feed_id_here,ETH:feed_id_here,SOL:feed_id_here
```

### **3. Build and Deploy**

```bash
# Build optimized containers
./build.sh

# Start all services
docker-compose up -d

# Verify deployment
docker-compose ps
```

### **4. Check Health**

```bash
# Test health endpoints
curl http://localhost:9081/health | jq .

# View logs
docker-compose logs -f sol-bot
```

## 🤖 **Discord Bot Setup**

### **Creating Discord Bots**

For each cryptocurrency, you need a separate Discord bot:

1. **Go to Discord Developer Portal**: https://discord.com/developers/applications
2. **Create New Application**: Give it a name (e.g., "BTC Price Bot")
3. **Create Bot**: Go to "Bot" section and create a bot
4. **Copy Token**: Save the bot token securely
5. **Enable Intents**: Enable "Server Members Intent" if needed
6. **Generate Invite**: Create invite link with appropriate permissions

### **Required Permissions**

Your bots need these Discord permissions:
- `Change Nickname` - To update price in nickname
- `Use Slash Commands` - For interactive commands
- `Send Messages` - For command responses
- `Read Message History` - For command context

### **Invite URL Example**

```
https://discord.com/api/oauth2/authorize?client_id=YOUR_BOT_ID&permissions=67584&scope=bot%20applications.commands
```

## 📈 **Supported Cryptocurrencies**

The system currently supports these cryptocurrencies:

| Symbol | Name | Status |
|--------|------|--------|
| BTC | Bitcoin | ✅ Active |
| ETH | Ethereum | ✅ Active |
| SOL | Solana | ✅ Active |
| AVAX | Avalanche | ✅ Active |
| BNB | Binance Coin | ✅ Active |
| DOGE | Dogecoin | ✅ Active |
| SUI | Sui | ✅ Active |
| SEI | Sei | ✅ Active |
| JLP | Jupiter LP | ✅ Active |
| PUMP | Pump.fun | ✅ Active |
| MSTR | MicroStrategy | ✅ Active |
| HOOD | Robinhood | ✅ Active |
| SBET | SportsBet | ✅ Active |
| GOLD | Gold | ✅ Active |
| SILVER | Silver | ✅ Active |

## ➕ **Adding New Cryptocurrencies**

### **Step 1: Create Discord Bot**

Follow the Discord Bot Setup section to create a new bot for your cryptocurrency.

### **Step 2: Update Environment**

Add the new bot token to your `.env` file:

```bash
DISCORD_TOKEN_NEWCOIN=your_new_bot_token_here
```

### **Step 3: Add to Docker Compose**

Add a new service to `docker-compose.yml`:

```yaml
newcoin-bot:
  build: .
  volumes:
    - ./shared:/app/shared
  environment:
    - DISCORD_TOKEN=${DISCORD_TOKEN_NEWCOIN}
    - CRYPTO_NAME=NEWCOIN
    - UPDATE_INTERVAL_SECONDS=${UPDATE_INTERVAL_SECONDS:-12}
  command: ["./discord-bot"]
  depends_on:
    - price-service
  restart: unless-stopped
  ports:
    - "9098:8080"  # Use next available port
  healthcheck:
    test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
    interval: 30s
    timeout: 10s
    retries: 3
```

### **Step 4: Find Pyth Feed ID**

1. Visit https://pyth.network/price-feeds
2. Search for your cryptocurrency
3. Copy the feed ID (hexadecimal string)
4. Add to `CRYPTO_FEEDS` in your environment

### **Step 5: Deploy**

```bash
# Rebuild and restart
docker-compose down
docker-compose up -d --build
```

## 💬 **Bot Commands**

### **Slash Commands**

| Command | Description | Example |
|---------|-------------|---------|
| `/price [crypto]` | Get current price info | `/price BTC` |

### **Command Response Format**

```
📊 BTC: $67,234.56 📈 +2.15% (1h) | 📈 +5.23% (12h) | 📉 -1.45% (24h)
💱 Cross-rates: 1.000 BTC | 18.45 ETH | 705.2 SOL
```

### **Status Display Rotation**

Each bot cycles through these status messages:
1. **1-hour change**: "📈 +2.15% (1h)"
2. **BTC equivalent**: "≈ 0.0234 BTC"
3. **ETH equivalent**: "≈ 1.456 ETH"
4. **SOL equivalent**: "≈ 234.5 SOL"

## 🔧 **Configuration**

### **Environment Variables**

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `UPDATE_INTERVAL_SECONDS` | Price update frequency | `12` | No |
| `DISCORD_TOKEN_*` | Discord bot tokens | - | Yes |
| `CRYPTO_FEEDS` | Pyth Network feed IDs | - | No |
| `CRYPTO_NAME` | Cryptocurrency symbol | - | Yes |
| `CLEANUP_INTERVAL_HOURS` | Database cleanup frequency | `24` | No |

### **Update Intervals**

Choose based on your needs:
- **12 seconds**: Real-time updates (default)
- **30 seconds**: Balanced performance
- **60 seconds**: Lower resource usage
- **300 seconds**: Minimal resource usage

### **Health Check Ports**

Each service exposes a health endpoint:
- Price Service: No external port
- SOL Bot: `http://localhost:9081/health`
- BTC Bot: `http://localhost:9082/health`
- ETH Bot: `http://localhost:9083/health`
- (Additional bots on ports 9084-9096)

## 📊 **Monitoring & Management**

### **Health Monitoring**

```bash
# Check all services
docker-compose ps

# Test specific health endpoint
curl http://localhost:9081/health | jq .

# Monitor resource usage
docker stats --no-stream
```

### **Log Management**

```bash
# View all logs
docker-compose logs -f

# View specific service
docker-compose logs -f btc-bot

# View recent logs only
docker-compose logs --tail=50 price-service
```

### **Database Management**

```bash
# Query database directly
docker-compose exec price-service ./db-query

# Manual cleanup
docker-compose exec db-cleanup ./db-cleanup

# Backup database
cp shared/prices.db backup/prices-$(date +%Y%m%d).db
```

## 🛠️ **Development**

### **Local Development**

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build project
cargo build --release

# Run price service locally
cargo run --bin price-service

# Run specific bot locally
CRYPTO_NAME=BTC DISCORD_TOKEN=your_token cargo run --bin discord-bot
```

### **Testing**

```bash
# Run tests
cargo test

# Check code formatting
cargo fmt --check

# Run linter
cargo clippy
```

### **Building Containers**

```bash
# Build all containers
./build.sh

# Build specific service
docker-compose build btc-bot

# Check image sizes
docker images | grep rustymcpriceface
```

## 🚨 **Troubleshooting**

### **Common Issues**

**Bot not responding to commands:**
- Verify bot token is correct
- Check bot permissions in Discord server
- Ensure slash commands are registered
- Check bot logs: `docker-compose logs bot-name`

**No price updates:**
- Check price-service logs: `docker-compose logs price-service`
- Verify network connectivity
- Check Pyth Network status
- Ensure shared directory permissions are correct

**Database errors:**
- Check disk space: `df -h`
- Verify shared directory permissions: `ls -la shared/`
- Check database file: `file shared/prices.db`
- Review database logs: `docker-compose logs db-cleanup`

**High resource usage:**
- Monitor with: `docker stats`
- Check update interval settings
- Review log levels
- Consider increasing update intervals

### **Debug Commands**

```bash
# Access container shell
docker-compose exec btc-bot /bin/sh

# Check container health
docker-compose exec btc-bot curl localhost:8080/health

# View database schema
docker-compose exec price-service sqlite3 shared/prices.db ".schema"

# Check file permissions
docker-compose exec price-service ls -la shared/

# Test network connectivity
docker-compose exec price-service ping pyth.network
```

### **Performance Tuning**

```bash
# Reduce update frequency
UPDATE_INTERVAL_SECONDS=30

# Lower log levels
RUST_LOG=warn

# Optimize database
docker-compose exec price-service sqlite3 shared/prices.db "VACUUM;"
```

## 📝 **License**

This project is licensed under the MIT License - see the LICENSE file for details.

## 🤝 **Contributing**

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📞 **Support**

- **Issues**: Create an issue on GitHub
- **Documentation**: Check this README and inline code comments
- **Logs**: Always include relevant logs when reporting issues
- **Health Checks**: Use health endpoints to diagnose problems

---

**Built with ❤️ in Rust** | **Optimized for Alpine Linux** | **Production Ready** 🚀 