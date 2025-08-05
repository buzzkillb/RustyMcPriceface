#!/bin/bash

# Bot monitoring script - automatically restarts bots that aren't updating properly
# This checks if bots are actually updating Discord (not just health endpoint)

BOTS=(
    "silver-bot:9096"
    "sui-bot:9091"
    "sol-bot:9081"
    "btc-bot:9082"
    "eth-bot:9083"
    "fartcoin-bot:9084"
    "avax-bot:9085"
    "bnb-bot:9086"
    "doge-bot:9087"
    "jlp-bot:9088"
    "pump-bot:9089"
    "sei-bot:9090"
    "mstr-bot:9092"
    "hood-bot:9093"
    "sbet-bot:9094"
    "gold-bot:9095"
)

LOG_FILE="./bot_monitor.log"
MAX_SECONDS_SINCE_UPDATE=300  # 5 minutes

log() {
    echo "$(date '+%Y-%m-%d %H:%M:%S') - $1" | tee -a "$LOG_FILE"
}

check_and_restart_bot() {
    local bot_name=$1
    local port=$2
    local container_name="rustymcpriceface-${bot_name}-1"
    
    # Check health endpoint
    local health_response=$(curl -s -f "http://localhost:${port}/health" 2>/dev/null)
    
    if [ $? -ne 0 ]; then
        log "ERROR: $bot_name health endpoint unreachable, restarting container"
        docker restart "$container_name"
        return
    fi
    
    # Check for gateway connection errors in recent logs
    local gateway_errors=$(docker logs --tail 20 "$container_name" 2>/dev/null | grep -c "failed to send ShardRunnerMessage to shard: send failed because receiver is gone")
    
    if [ "$gateway_errors" -gt 0 ]; then
        log "WARNING: $bot_name has gateway connection errors ($gateway_errors recent), restarting container"
        docker restart "$container_name"
        return
    fi
    
    # Parse JSON to check if Discord updates are recent and check for gateway failures
    local seconds_since_discord=$(echo "$health_response" | jq -r '.seconds_since_discord_update // 999999')
    local consecutive_failures=$(echo "$health_response" | jq -r '.consecutive_failures // 0')
    local gateway_failures=$(echo "$health_response" | jq -r '.gateway_failures // 0')
    local healthy=$(echo "$health_response" | jq -r '.healthy // false')
    
    if [ "$healthy" = "false" ]; then
        log "WARNING: $bot_name reports unhealthy status, restarting container"
        docker restart "$container_name"
    elif [ "$seconds_since_discord" -gt "$MAX_SECONDS_SINCE_UPDATE" ]; then
        log "WARNING: $bot_name hasn't updated Discord in ${seconds_since_discord}s, restarting container"
        docker restart "$container_name"
    elif [ "$consecutive_failures" -gt 5 ]; then
        log "WARNING: $bot_name has ${consecutive_failures} consecutive failures, restarting container"
        docker restart "$container_name"
    elif [ "$gateway_failures" -gt 5 ]; then
        log "WARNING: $bot_name has ${gateway_failures} gateway failures, restarting container"
        docker restart "$container_name"
    else
        log "INFO: $bot_name is healthy (Discord: ${seconds_since_discord}s ago, failures: ${consecutive_failures}, gateway: ${gateway_failures})"
    fi
}

main() {
    log "Starting bot monitoring check"
    
    for bot_info in "${BOTS[@]}"; do
        IFS=':' read -r bot_name port <<< "$bot_info"
        check_and_restart_bot "$bot_name" "$port"
        sleep 2  # Small delay between checks
    done
    
    log "Bot monitoring check completed"
}

main "$@"