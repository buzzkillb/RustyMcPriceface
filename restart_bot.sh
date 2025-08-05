#!/bin/bash

# Script to manually restart a specific bot

if [ $# -eq 0 ]; then
    echo "Usage: $0 <bot-name>"
    echo "Available bots: silver, sui, sol, btc, eth, fartcoin, avax, bnb, doge, jlp, pump, sei, mstr, hood, sbet, gold"
    exit 1
fi

BOT_NAME=$1
CONTAINER_NAME="rustymcpriceface-${BOT_NAME}-bot-1"

echo "Restarting $BOT_NAME bot..."
docker restart "$CONTAINER_NAME"

if [ $? -eq 0 ]; then
    echo "✅ Successfully restarted $BOT_NAME bot"
    echo "Waiting 10 seconds for startup..."
    sleep 10
    
    # Try to get the port for this bot
    case $BOT_NAME in
        "silver") PORT=9096 ;;
        "sui") PORT=9091 ;;
        "sol") PORT=9081 ;;
        "btc") PORT=9082 ;;
        "eth") PORT=9083 ;;
        "fartcoin") PORT=9084 ;;
        "avax") PORT=9085 ;;
        "bnb") PORT=9086 ;;
        "doge") PORT=9087 ;;
        "jlp") PORT=9088 ;;
        "pump") PORT=9089 ;;
        "sei") PORT=9090 ;;
        "mstr") PORT=9092 ;;
        "hood") PORT=9093 ;;
        "sbet") PORT=9094 ;;
        "gold") PORT=9095 ;;
        *) echo "Unknown bot: $BOT_NAME"; exit 1 ;;
    esac
    
    echo "Checking health status..."
    HEALTH=$(curl -s "http://localhost:${PORT}/health" | jq -r '.healthy // false')
    if [ "$HEALTH" = "true" ]; then
        echo "✅ $BOT_NAME bot is healthy"
    else
        echo "⚠️  $BOT_NAME bot may still have issues"
    fi
else
    echo "❌ Failed to restart $BOT_NAME bot"
    exit 1
fi