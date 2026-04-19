"""
Price fetching service using Pyth Network API.
"""
import logging
import os
import re
from typing import Optional

import aiohttp

logger = logging.getLogger(__name__)

HERMES_API_URL = "https://hermes.pyth.network/api/latest_price_feeds"
GOLDSILVER_AI_URL = "https://goldsilver.ai/metal-prices/shanghai-silver-price"


class PriceService:
    def __init__(self):
        self.feeds = self._load_feeds()
        self.session: Optional[aiohttp.ClientSession] = None
    
    def _load_feeds(self) -> dict:
        """Load feed IDs from environment."""
        feeds_str = os.environ.get(
            "CRYPTO_FEEDS",
            "BTC:0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43,"
            "ETH:0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace,"
            "SOL:0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d"
        )
        
        feeds = {}
        for pair in feeds_str.split(","):
            if ":" in pair:
                name, feed_id = pair.split(":", 1)
                feeds[name.strip().upper()] = feed_id.strip()
        
        logger.info(f"Loaded {len(feeds)} price feeds")
        return feeds
    
    async def _get_session(self) -> aiohttp.ClientSession:
        if self.session is None or self.session.closed:
            timeout = aiohttp.ClientTimeout(total=15)
            self.session = aiohttp.ClientSession(timeout=timeout)
        return self.session
    
    async def get_shanghai_silver_price(self) -> Optional[float]:
        """Fetch Shanghai Silver price from goldsilver.ai."""
        try:
            session = await self._get_session()
            headers = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            }
            async with session.get(GOLDSILVER_AI_URL, headers=headers) as resp:
                if resp.status != 200:
                    logger.warning(f"goldsilver.ai returned {resp.status}")
                    return None
                
                text = await resp.text()
                return self._extract_shanghai_price(text)
                
        except Exception as e:
            logger.error(f"Failed to fetch Shanghai Silver: {e}")
            return None
    
    def _extract_shanghai_price(self, html: str) -> Optional[float]:
        """Extract Shanghai Silver price from HTML."""
        patterns = [
            r'Shanghai Spot.*?\$?([0-9,]+\.?[0-9]*)',
            r'"price".*?"amount".*?([0-9,]+\.?[0-9]*)',
            r'Silver.*?\$?([0-9,]+\.?[0-9]*)',
        ]
        
        for pattern in patterns:
            match = re.search(pattern, html, re.IGNORECASE | re.DOTALL)
            if match:
                price_str = match.group(1).replace(",", "")
                try:
                    price = float(price_str)
                    if price > 0:
                        logger.debug(f"Extracted Shanghai Silver price: ${price}")
                        return price
                except ValueError:
                    continue
        
        return None
    
    async def get_price(self, crypto: str) -> Optional[float]:
        """Get price for a single cryptocurrency."""
        crypto = crypto.upper()
        
        # Special handling for Shanghai Silver
        if crypto == "SHANGHAISILVER":
            return await self.get_shanghai_silver_price()
        
        if crypto not in self.feeds:
            logger.warning(f"No feed ID for {crypto}")
            return None
        
        feed_id = self.feeds[crypto]
        url = f"{HERMES_API_URL}?ids[]={feed_id}"
        
        try:
            session = await self._get_session()
            async with session.get(url) as resp:
                if resp.status != 200:
                    logger.warning(f"Pyth API returned {resp.status} for {crypto}")
                    return None
                
                data = await resp.json()
                if not data or not isinstance(data, list):
                    return None
                
                price_data = data[0].get("price", {})
                price_str = price_data.get("price")
                expo = price_data.get("expo", 0)
                
                if price_str is None:
                    return None
                
                price = int(price_str) * (10 ** expo)
                
                if price <= 0:
                    logger.warning(f"Invalid price {price} for {crypto}")
                    return None
                
                logger.debug(f"Fetched {crypto} price: ${price}")
                return float(price)
                
        except Exception as e:
            logger.error(f"Failed to fetch {crypto} price: {e}")
            return None
    
    async def get_all_prices(self) -> dict:
        """Get prices for all configured cryptocurrencies."""
        results = {}
        for crypto in self.feeds:
            price = await self.get_price(crypto)
            if price:
                results[crypto] = price
        return results
    
    async def close(self):
        """Close the HTTP session."""
        if self.session and not self.session.closed:
            await self.session.close()
