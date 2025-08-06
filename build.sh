#!/bin/bash

# Build script for optimized Alpine Docker containers
set -e

echo "🚀 Building optimized Alpine containers..."

# Build the Docker image
echo "📦 Building Docker image..."
docker build -t crypto-price-bot:alpine .

# Tag the image
docker tag crypto-price-bot:alpine crypto-price-bot:latest

echo "✅ Build complete!"
echo "📊 Image sizes:"
docker images crypto-price-bot --format "table {{.Repository}}\t{{.Tag}}\t{{.Size}}"

echo ""
echo "🔧 To rebuild and restart all services:"
echo "  docker-compose down"
echo "  docker-compose up -d --build"
echo ""
echo "🔍 To check container sizes:"
echo "  docker system df"