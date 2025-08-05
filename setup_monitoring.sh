#!/bin/bash

# Setup automatic bot monitoring

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MONITOR_SCRIPT="$SCRIPT_DIR/monitor_bots.sh"

echo "Setting up automatic bot monitoring..."

# Create log directory
sudo mkdir -p /var/log
sudo touch /var/log/bot_monitor.log
sudo chown $USER:$USER /var/log/bot_monitor.log

# Add cron job to run every 5 minutes
CRON_JOB="*/5 * * * * $MONITOR_SCRIPT >> /var/log/bot_monitor.log 2>&1"

# Check if cron job already exists
if ! crontab -l 2>/dev/null | grep -q "$MONITOR_SCRIPT"; then
    # Add the cron job
    (crontab -l 2>/dev/null; echo "$CRON_JOB") | crontab -
    echo "✅ Added cron job to run monitoring every 5 minutes"
else
    echo "ℹ️  Cron job already exists"
fi

# Test the monitoring script once
echo "🔍 Running initial monitoring check..."
$MONITOR_SCRIPT

echo ""
echo "🎉 Monitoring setup complete!"
echo ""
echo "The monitoring script will now run every 5 minutes and automatically restart any bots that:"
echo "  - Haven't updated Discord in 5+ minutes"
echo "  - Have more than 5 consecutive failures"
echo "  - Have unreachable health endpoints"
echo ""
echo "To check logs: tail -f /var/log/bot_monitor.log"
echo "To remove monitoring: crontab -e (then delete the line with monitor_bots.sh)"