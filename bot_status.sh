#!/bin/bash

# Script to show status of all bots

BOTS=(
    "silver:9096"
    "sui:9091"
    "sol:9081"
    "btc:9082"
    "eth:9083"
    "fartcoin:9084"
    "avax:9085"
    "bnb:9086"
    "doge:9087"
    "jlp:9088"
    "pump:9089"
    "sei:9090"
    "mstr:9092"
    "hood:9093"
    "sbet:9094"
    "gold:9095"
)

echo "🤖 Bot Status Report - $(date)"
echo "=================================="
printf "%-12s %-8s %-12s %-12s %-8s %-8s\n" "BOT" "HEALTHY" "DISCORD" "FAILURES" "GATEWAY" "CONTAINER"
echo "------------------------------------------------------------------"

for bot_info in "${BOTS[@]}"; do
    IFS=':' read -r bot_name port <<< "$bot_info"
    container_name="rustymcpriceface-${bot_name}-bot-1"
    
    # Get container status
    container_status=$(docker ps --format "table {{.Names}}\t{{.Status}}" | grep "$container_name" | awk '{print $2}')
    if [ -z "$container_status" ]; then
        container_status="Down"
    fi
    
    # Get health info
    health_response=$(curl -s -f "http://localhost:${port}/health" 2>/dev/null)
    if [ $? -eq 0 ]; then
        healthy=$(echo "$health_response" | jq -r '.healthy // false')
        discord_age=$(echo "$health_response" | jq -r '.seconds_since_discord_update // 999')
        failures=$(echo "$health_response" | jq -r '.consecutive_failures // 0')
        gateway_failures=$(echo "$health_response" | jq -r '.gateway_failures // 0')
        
        # Format discord age
        if [ "$discord_age" -lt 60 ]; then
            discord_display="${discord_age}s"
        elif [ "$discord_age" -lt 3600 ]; then
            discord_display="$((discord_age/60))m"
        else
            discord_display="$((discord_age/3600))h"
        fi
        
        # Color coding
        if [ "$healthy" = "true" ]; then
            health_display="✅ Yes"
        else
            health_display="❌ No"
        fi
        
        if [ "$discord_age" -gt 300 ]; then
            discord_display="🔴 $discord_display"
        elif [ "$discord_age" -gt 120 ]; then
            discord_display="🟡 $discord_display"
        else
            discord_display="🟢 $discord_display"
        fi
        
    else
        health_display="❌ No"
        discord_display="🔴 N/A"
        failures="N/A"
        gateway_failures="N/A"
    fi
    
    printf "%-12s %-8s %-12s %-12s %-8s %-8s\n" "$bot_name" "$health_display" "$discord_display" "$failures" "$gateway_failures" "$container_status"
done

echo ""
echo "Legend:"
echo "🟢 = Recent update (< 2min)  🟡 = Moderate delay (2-5min)  🔴 = Stale (> 5min)"
echo ""
echo "To restart a bot: ./restart_bot.sh <bot-name>"
echo "To check logs: docker logs rustymcpriceface-<bot-name>-bot-1"