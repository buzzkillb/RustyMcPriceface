#!/bin/bash

# Bot restart script with health verification

echo "🔄 Discord Bot Restart Script"
echo "============================="
echo ""

# Function to check bot health
check_health() {
    local bot_name=$1
    local port=$2
    local max_attempts=10
    local attempt=1
    
    echo "⏳ Waiting for $bot_name to become healthy..."
    
    while [ $attempt -le $max_attempts ]; do
        if curl -s -f "http://localhost:$port/health" > /dev/null 2>&1; then
            echo "✅ $bot_name is healthy"
            return 0
        fi
        
        echo "   Attempt $attempt/$max_attempts - waiting..."
        sleep 3
        ((attempt++))
    done
    
    echo "❌ $bot_name failed to become healthy after $max_attempts attempts"
    return 1
}

# Parse command line arguments
if [ "$1" = "all" ] || [ -z "$1" ]; then
    echo "🛑 Stopping all services..."
    docker-compose down
    
    echo "🚀 Starting all services..."
    docker-compose up -d
    
    echo "⏳ Waiting for services to start..."
    sleep 10
    
    # Check health of key bots
    declare -A KEY_BOTS=(
        ["sol-bot"]="9081"
        ["btc-bot"]="9082"
        ["eth-bot"]="9083"
    )
    
    for bot in "${!KEY_BOTS[@]}"; do
        check_health "$bot" "${KEY_BOTS[$bot]}"
    done
    
elif [ "$1" = "price-service" ]; then
    echo "🛑 Restarting price service..."
    docker-compose restart price-service
    
    echo "⏳ Waiting for price service to start..."
    sleep 5
    
    echo "✅ Price service restarted"
    
else
    # Restart specific bot
    BOT_NAME=$1
    
    echo "🛑 Restarting $BOT_NAME..."
    docker-compose restart "$BOT_NAME"
    
    echo "⏳ Waiting for $BOT_NAME to start..."
    sleep 5
    
    # Try to determine health check port (simplified mapping)
    case $BOT_NAME in
        "sol-bot") PORT=9081 ;;
        "btc-bot") PORT=9082 ;;
        "eth-bot") PORT=9083 ;;
        "fartcoin-bot") PORT=9084 ;;
        "avax-bot") PORT=9085 ;;
        "bnb-bot") PORT=9086 ;;
        "doge-bot") PORT=9087 ;;
        "jlp-bot") PORT=9088 ;;
        "pump-bot") PORT=9089 ;;
        "sei-bot") PORT=9090 ;;
        "sui-bot") PORT=9091 ;;
        "mstr-bot") PORT=9092 ;;
        "hood-bot") PORT=9093 ;;
        "sbet-bot") PORT=9094 ;;
        "gold-bot") PORT=9095 ;;
        "silver-bot") PORT=9096 ;;
        "db-cleanup") PORT=9097 ;;
        *) PORT="" ;;
    esac
    
    if [ -n "$PORT" ]; then
        check_health "$BOT_NAME" "$PORT"
    else
        echo "✅ $BOT_NAME restarted (no health check configured)"
    fi
fi

echo ""
echo "📊 Current Status:"
docker-compose ps --format "table {{.Name}}\t{{.State}}\t{{.Status}}"

echo ""
echo "💡 Tips:"
echo "   - Run './monitor_bots.sh' to check detailed health status"
echo "   - Use 'docker-compose logs -f [bot-name]' to view logs"
echo "   - Check 'shared/prices.json' for price data freshness"
echo ""