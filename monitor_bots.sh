#!/bin/bash

# Bot monitoring script
# This script checks the health of all Discord bots and provides status information

echo "🤖 Discord Bot Health Monitor"
echo "=============================="
echo ""

# Define bot services and their health check ports
declare -A BOTS=(
    ["sol-bot"]="9081"
    ["btc-bot"]="9082" 
    ["eth-bot"]="9083"
    ["fartcoin-bot"]="9084"
    ["avax-bot"]="9085"
    ["bnb-bot"]="9086"
    ["doge-bot"]="9087"
    ["jlp-bot"]="9088"
    ["pump-bot"]="9089"
    ["sei-bot"]="9090"
    ["sui-bot"]="9091"
    ["mstr-bot"]="9092"
    ["hood-bot"]="9093"
    ["sbet-bot"]="9094"
    ["gold-bot"]="9095"
    ["silver-bot"]="9096"
)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check Docker Compose status
echo "📊 Docker Compose Status:"
docker-compose ps --format "table {{.Name}}\t{{.State}}\t{{.Status}}"
echo ""

# Check individual bot health
echo "🏥 Bot Health Status:"
echo "Bot Name          | Status | Last Price Update | Last DB Write | Failures"
echo "------------------|--------|-------------------|---------------|----------"

for bot in "${!BOTS[@]}"; do
    port=${BOTS[$bot]}
    
    # Check if container is running
    if ! docker-compose ps -q "$bot" > /dev/null 2>&1; then
        printf "%-17s | ${RED}DOWN${NC}   | N/A               | N/A           | N/A\n" "$bot"
        continue
    fi
    
    # Check health endpoint
    health_response=$(curl -s -w "%{http_code}" "http://localhost:$port/health" 2>/dev/null)
    http_code="${health_response: -3}"
    health_data="${health_response%???}"
    
    if [ "$http_code" = "200" ]; then
        # Parse health data
        last_price=$(echo "$health_data" | jq -r '.seconds_since_price_update // "N/A"' 2>/dev/null)
        last_db=$(echo "$health_data" | jq -r '.seconds_since_db_write // "N/A"' 2>/dev/null)
        failures=$(echo "$health_data" | jq -r '.consecutive_failures // "N/A"' 2>/dev/null)
        
        printf "%-17s | ${GREEN}OK${NC}     | %-17s | %-13s | %s\n" "$bot" "${last_price}s ago" "${last_db}s ago" "$failures"
    elif [ "$http_code" = "503" ]; then
        printf "%-17s | ${YELLOW}WARN${NC}   | Health check failed | Health check failed | Unknown\n" "$bot"
    else
        printf "%-17s | ${RED}ERROR${NC}  | No response       | No response   | Unknown\n" "$bot"
    fi
done

echo ""

# Check logs for recent errors
echo "🔍 Recent Error Summary (last 50 lines):"
echo "========================================="
for bot in "${!BOTS[@]}"; do
    error_count=$(docker-compose logs --tail=50 "$bot" 2>/dev/null | grep -i "error\|failed\|panic" | wc -l)
    if [ "$error_count" -gt 0 ]; then
        echo "❌ $bot: $error_count recent errors"
        docker-compose logs --tail=5 "$bot" 2>/dev/null | grep -i "error\|failed\|panic" | tail -2
        echo ""
    fi
done

# Check price service status
echo "💰 Price Service Status:"
echo "========================"
if [ -f "shared/prices.json" ]; then
    last_update=$(stat -c %Y "shared/prices.json" 2>/dev/null || echo "0")
    current_time=$(date +%s)
    age=$((current_time - last_update))
    
    if [ "$age" -lt 60 ]; then
        echo "✅ Price data is fresh (updated ${age}s ago)"
    elif [ "$age" -lt 300 ]; then
        echo "⚠️  Price data is getting stale (updated ${age}s ago)"
    else
        echo "❌ Price data is stale (updated ${age}s ago)"
    fi
    
    # Show available cryptocurrencies
    echo "📈 Available cryptocurrencies:"
    if command -v jq >/dev/null 2>&1; then
        jq -r '.prices | keys[]' "shared/prices.json" 2>/dev/null | sort | tr '\n' ' '
        echo ""
    else
        echo "   (install jq to see cryptocurrency list)"
    fi
else
    echo "❌ Price data file not found"
fi

echo ""
echo "🔧 Troubleshooting Commands:"
echo "============================"
echo "View logs:           docker-compose logs -f [bot-name]"
echo "Restart bot:         docker-compose restart [bot-name]"
echo "Restart all:         docker-compose restart"
echo "Check containers:    docker-compose ps"
echo "Check resources:     docker stats"
echo ""