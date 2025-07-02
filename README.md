# Multi-Crypto Discord Bot System

A real-time cryptocurrency price tracking system that updates Discord bot nicknames and statuses with live price data from Pyth Network. The system supports multiple cryptocurrencies simultaneously with intelligent price formatting and SQLite data storage.

## 🚀 Features

- **Real-time Price Updates**: Fetches live crypto prices every 12 seconds
- **Multi-Crypto Support**: Track BTC, ETH, SOL, WIF, FARTCOIN, and any other Pyth Network feeds
- **Smart Price Formatting**: Automatic decimal places based on price magnitude
- **Discord Integration**: Updates bot nicknames and custom statuses
- **SQLite Database**: 7-day price history with automatic cleanup
- **Docker Deployment**: Easy containerized deployment
- **Generic Configuration**: Add new cryptos without code changes

## 📊 Price Formatting Rules

The system automatically formats prices based on their value:
- **≥$1000**: No decimals (e.g., `BTC $107018`)
- **≥$100**: 2 decimal places (e.g., `SOL $149.70`)
- **≥$1**: 3 decimal places (e.g., `FARTCOIN $1.064`)
- **<$1**: 4 decimal places (e.g., `WIF $0.8097`)

## 🏗️ Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Pyth Network  │───▶│  Price Service   │───▶│  Discord Bots   │
│   Hermes API    │    │                  │    │                 │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                              │
                              ▼
                       ┌──────────────────┐
                       │   SQLite DB      │
                       │   (7-day data)   │
                       └──────────────────┘
```

## 📋 Prerequisites

- **Rust** (latest stable version)
- **Docker** and **Docker Compose**
- **Discord Bot Tokens** (one per crypto you want to track)
- **Pyth Network Feed IDs** for your desired cryptocurrencies

## 🛠️ Installation

### 1. Clone the Repository
```bash
git clone <repository-url>
cd pbot
```

### 2. Build the Project
```bash
cargo build --release
```

### 3. Set Up Environment Variables
Create a `.env` file:
```bash
# Price update interval (shared by all services)
UPDATE_INTERVAL_SECONDS=12

# Discord Bot Tokens (replace with your actual tokens)
DISCORD_TOKEN_SOL=your_sol_bot_token_here
DISCORD_TOKEN_BTC=your_btc_bot_token_here
DISCORD_TOKEN_ETH=your_eth_bot_token_here
DISCORD_TOKEN_WIF=your_wif_bot_token_here

# Crypto Feed IDs (Pyth Network feed IDs)
CRYPTO_FEEDS=BTC:0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43,ETH:0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace,SOL:0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d,WIF:0x4ca4beeca86f0d164160323817a4e42b10010a724c2217c6ee41b54cd4cc61fc,FARTCOIN:0x58cd29ef0e714c5affc44f269b2c1899a52da4169d7acc147b9da692e6953608
```

### 4. Start the Services
```bash
docker-compose up -d --build
```

## 🔧 Configuration

### Adding New Cryptocurrencies

1. **Find the Pyth Network Feed ID**:
   - Visit [Pyth Network Price Feeds](https://www.pyth.network/price-feeds/)
   - Search for your desired cryptocurrency
   - Copy the feed ID (64-character hex string)

2. **Add to Environment**:
   ```bash
   # Add to CRYPTO_FEEDS in .env
   CRYPTO_FEEDS=...,NEWCOIN:0xfeed_id_here
   ```

3. **Create Discord Bot**:
   - Create a new Discord application at [Discord Developer Portal](https://discord.com/developers/applications)
   - Add a bot to your application
   - Copy the bot token

4. **Add Bot Token**:
   ```bash
   # Add to .env
   DISCORD_TOKEN_NEWCOIN=your_newcoin_bot_token_here
   ```

5. **Add Docker Service**:
   ```yaml
   # Add to docker-compose.yml
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
   ```

6. **Restart Services**:
   ```bash
   docker-compose up -d newcoin-bot
   ```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `UPDATE_INTERVAL_SECONDS` | Price update frequency | `12` |
| `CRYPTO_FEEDS` | Pyth Network feed IDs | BTC, ETH, SOL |
| `DISCORD_TOKEN_*` | Discord bot tokens | Required |

## 📊 Database Operations

### Query Database Statistics
```bash
docker exec pbot-price-service-1 ./db-query stats
```

### View Latest Prices
```bash
docker exec pbot-price-service-1 ./db-query latest
```

### View Price History
```bash
docker exec pbot-price-service-1 ./db-query history
```

### Clean Up Old Data
```bash
docker exec pbot-price-service-1 ./db-query cleanup
```

## 🐳 Docker Commands

### Start All Services
```bash
docker-compose up -d
```

### Stop All Services
```bash
docker-compose down
```

### Restart Specific Service
```bash
docker-compose restart price-service
docker-compose restart sol-bot
```

### View Logs
```bash
# All services
docker-compose logs

# Specific service
docker-compose logs price-service
docker-compose logs sol-bot

# Follow logs in real-time
docker-compose logs -f price-service
```

### Rebuild and Restart
```bash
docker-compose down
docker-compose up -d --build
```

## 🔍 Troubleshooting

### Price Service Issues
- **404 Errors**: Check if feed ID exists on Pyth Network
- **API Errors**: Verify network connectivity to Hermes API
- **Database Errors**: Check shared directory permissions

### Discord Bot Issues
- **Token Invalid**: Verify bot token in `.env`
- **Permission Errors**: Ensure bot has "Manage Nicknames" permission
- **Rate Limits**: Increase update interval if hitting Discord limits

### Common Commands
```bash
# Check service status
docker-compose ps

# View recent logs
docker-compose logs --tail=20

# Restart everything
docker-compose down && docker-compose up -d --build

# Check environment variables
docker exec pbot-price-service-1 env | grep CRYPTO_FEEDS
```

## 📈 Monitoring

### Price Service Logs
- ✅ Successful price fetches
- ❌ Failed API requests
- 📝 Database operations
- 🧹 Cleanup activities

### Discord Bot Logs
- Bot login status
- Nickname updates
- Custom status changes
- Guild connection status

## 🔒 Security Notes

- **Never commit Discord tokens** to version control
- **Use environment variables** for all sensitive data
- **Regular token rotation** is recommended
- **Monitor bot permissions** in Discord servers

## 🚀 Performance

- **Update Interval**: Configurable (default: 12 seconds)
- **Database Retention**: 7 days with automatic cleanup
- **Memory Usage**: ~50MB per bot instance
- **Network**: Minimal bandwidth usage

## 📝 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly
5. Submit a pull request

## 📞 Support

For issues and questions:
- Check the troubleshooting section
- Review Docker logs for error messages
- Verify Pyth Network feed availability
- Ensure Discord bot permissions are correct

---

**Note**: This system requires active Pyth Network feeds and valid Discord bot tokens to function properly. 