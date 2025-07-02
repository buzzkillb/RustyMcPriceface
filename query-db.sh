#!/bin/bash

# Database query script for Docker containers
# Usage: ./query-db.sh <command> [args...]

if [ $# -eq 0 ]; then
    echo "Usage: $0 <command> [args...]"
    echo ""
    echo "Commands:"
    echo "  stats                    - Show database statistics"
    echo "  latest [crypto]          - Show latest prices for all or specific crypto"
    echo "  history [crypto] [limit] - Show price history (default: 10 records)"
    echo "  cleanup                  - Manually trigger cleanup of old records"
    echo ""
    echo "Examples:"
    echo "  $0 stats"
    echo "  $0 latest"
    echo "  $0 latest BTC"
    echo "  $0 history BTC 20"
    echo "  $0 cleanup"
    exit 1
fi

# Run the database query command
docker exec -it pbot-price-service-1 /app/db-query "$@" 