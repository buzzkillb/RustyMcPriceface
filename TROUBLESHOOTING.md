# Discord Price Bot Troubleshooting Guide

## Common Issues and Solutions

### 1. Bots Silently Crashing / Leaving Channels

**Symptoms:**
- Bot name disappears from Discord
- Bot leaves voice/text channels
- No response to commands

**Root Causes & Solutions:**

#### A. Discord API Rate Limiting
**Problem:** Too many API calls causing Discord to disconnect the bot
**Solution:** 
- Implemented rate limiting with 2-second delays between API calls
- Added exponential backoff for rate limit errors
- Reduced frequency of nickname updates

#### B. Network/API Failures
**Problem:** Unhandled network errors causing crashes
**Solution:**
- Added retry logic for all HTTP requests (3 attempts)
- Implemented timeout handling (10 seconds)
- Added fallback mechanisms for price data

#### C. Database Connection Issues
**Problem:** SQLite locking or connection failures
**Solution:**
- Added database connection retry logic
- Implemented proper connection cleanup
- Added error handling for all database operations

#### D. Memory/Resource Issues
**Problem:** Docker containers running out of memory
**Solution:**
- Added comprehensive error handling to prevent memory leaks
- Implemented proper resource cleanup
- Added health monitoring

### 2. Monitoring Bot Health

#### Health Check Endpoints
Each bot now exposes a health check endpoint on port 8080:
- SOL bot: http://localhost:9081/health
- BTC bot: http://localhost:9082/health
- ETH bot: http://localhost:9083/health

#### Using the Monitor Script
```bash
./monitor_bots.sh
```

This script provides:
- Container status
- Health check results
- Recent error summary
- Price service status

#### Manual Health Checks
```bash
# Check specific bot health
curl http://localhost:9081/health | jq

# Check all container status
docker-compose ps

# View recent logs
docker-compose logs --tail=50 sol-bot

# Check resource usage
docker stats
```

### 3. Common Error Patterns

#### Rate Limit Errors
```
Rate limited while updating nickname in guild 123456789: 429 Too Many Requests
```
**Solution:** The bot now automatically handles rate limits with exponential backoff.

#### Network Timeout Errors
```
Network request failed (attempt 1): operation timed out
```
**Solution:** Implemented retry logic with increasing delays.

#### Database Lock Errors
```
Failed to save price to database: database is locked
```
**Solution:** Added connection retry logic and proper cleanup.

#### Price Service Connection Errors
```
Prices file not found. Make sure price-service is running.
```
**Solution:** Ensure price-service container is running and healthy.

### 4. Recovery Procedures

#### Automatic Recovery
The bots now include automatic recovery mechanisms:
- Reconnection logic for Discord gateway disconnections
- Retry logic for failed operations
- Recovery mode after consecutive failures
- Health monitoring and alerting

#### Manual Recovery
```bash
# Restart a specific bot
docker-compose restart sol-bot

# Restart all bots
docker-compose restart

# View logs to identify issues
docker-compose logs -f sol-bot

# Check if price service is working
curl http://localhost:8080/health
```

### 5. Performance Optimization

#### Reduced Update Frequency
- Nickname updates now respect rate limits
- Added intelligent sleep timing based on actual update duration
- Implemented staggered updates across multiple guilds

#### Resource Management
- Added memory usage monitoring
- Implemented proper cleanup of resources
- Added health checks to detect resource issues early

### 6. Debugging Commands

```bash
# Check container health
docker-compose ps

# View real-time logs
docker-compose logs -f [bot-name]

# Check resource usage
docker stats

# Test health endpoints
curl http://localhost:9081/health

# Check price data freshness
ls -la shared/prices.json
cat shared/prices.json | jq '.timestamp'

# Check database
sqlite3 shared/prices.db "SELECT COUNT(*) FROM prices;"

# Monitor network connections
ss -tulpn | grep :908
```

### 7. Configuration Tuning

#### Environment Variables
```bash
# Increase update interval to reduce API calls
UPDATE_INTERVAL_SECONDS=30

# Enable debug logging
RUST_LOG=debug
```

#### Docker Compose Adjustments
```yaml
# Add memory limits
mem_limit: 256m

# Add restart policies
restart: unless-stopped

# Add health checks
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
  interval: 30s
  timeout: 10s
  retries: 3
```

### 8. Preventive Measures

#### Regular Monitoring
- Run `./monitor_bots.sh` regularly
- Set up alerts for health check failures
- Monitor Docker container resource usage

#### Log Analysis
- Check logs daily for error patterns
- Monitor rate limit warnings
- Watch for database connection issues

#### Maintenance
- Restart bots weekly to clear any accumulated issues
- Clean up old database records (automated)
- Update Discord tokens if they expire

### 9. Emergency Procedures

#### All Bots Down
```bash
# Stop all services
docker-compose down

# Check for port conflicts
ss -tulpn | grep :908

# Restart everything
docker-compose up -d

# Monitor startup
docker-compose logs -f
```

#### Database Corruption
```bash
# Backup current database
cp shared/prices.db shared/prices.db.backup

# Check database integrity
sqlite3 shared/prices.db "PRAGMA integrity_check;"

# If corrupted, recreate (will lose history)
rm shared/prices.db
docker-compose restart price-service
```

#### Discord API Issues
- Check Discord API status: https://discordstatus.com/
- Verify bot tokens are still valid
- Check bot permissions in Discord servers

### 10. Getting Help

If issues persist:
1. Run `./monitor_bots.sh` and save output
2. Collect recent logs: `docker-compose logs --tail=100 > bot_logs.txt`
3. Check Discord API status
4. Verify network connectivity
5. Review bot permissions in Discord servers

The enhanced error handling and monitoring should prevent most silent crashes, but this guide helps diagnose any remaining issues.