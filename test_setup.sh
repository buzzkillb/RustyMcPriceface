#!/bin/bash

echo "🧪 Testing Multi-Crypto Price Bot Setup"
echo "========================================"

# Check if .env exists
if [ ! -f ".env" ]; then
    echo "❌ .env file not found. Please copy env.example to .env and configure your tokens."
    exit 1
fi

echo "✅ .env file found"

# Test price service compilation
echo "🔨 Testing price service compilation..."
if cargo build --bin price-service --quiet; then
    echo "✅ Price service compiles successfully"
else
    echo "❌ Price service compilation failed"
    exit 1
fi

# Test discord bot compilation
echo "🔨 Testing discord bot compilation..."
if cargo build --bin discord-bot --quiet; then
    echo "✅ Discord bot compiles successfully"
else
    echo "❌ Discord bot compilation failed"
    exit 1
fi

# Test Docker Compose syntax (optional)
echo "🐳 Testing Docker Compose syntax..."
if command -v docker-compose &> /dev/null; then
    if docker-compose config --quiet; then
        echo "✅ Docker Compose syntax is valid"
    else
        echo "❌ Docker Compose syntax error"
        exit 1
    fi
else
    echo "⚠️  Docker Compose not installed (optional for manual setup)"
fi

echo ""
echo "🎉 All tests passed! Your setup is ready."
echo ""
echo "Next steps:"
echo "1. Configure your Discord bot tokens in .env"
echo "2. Run: docker-compose up -d"
echo "3. Or run manually:"
echo "   Terminal 1: cargo run --bin price-service"
echo "   Terminal 2: CRYPTO_NAME=BTC DISCORD_TOKEN=your_token cargo run --bin discord-bot"
echo ""
echo "Check shared/prices.json for current prices once the price service is running." 