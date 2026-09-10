"""
Price fetching service using Pyth Network API.
"""
import logging
import os
import re
import time
from typing import Optional

import aiohttp

logger = logging.getLogger(__name__)

HERMES_API_URL = "https://hermes.pyth.network/api/latest_price_feeds"
GOLDSILVER_AI_URL = "https://goldsilver.ai/metal-prices/shanghai-silver-price"
DEXSCREENER_API_URL = "https://api.dexscreener.com/latest/dex/pairs"


# Fallback price sources for feeds not covered by our Pyth Pro grant.
# Format: TICKER -> ("yahoo", symbol) | ("coingecko", coin_id) | ("dexscreener", chain/pair)
# Tried in order when the Pyth fetch fails (401/403/404/network).
FALLBACK_SOURCES = {
    # Equities (equity feeds are a separate paid Pyth tier)
    "MSTR": [("yahoo", "MSTR")],
    "HOOD": [("yahoo", "HOOD")],
    "SBET": [("yahoo", "SBET")],
    # Crypto not in our grant
    "AVAX": [("yahoo", "AVAX-USD"), ("coingecko", "avalanche-2")],
    "SEI": [("yahoo", "SEI-USD"), ("coingecko", "sei-network")],
    "XPL": [("yahoo", "XPL-USD"), ("coingecko", "plasma")],
    "SUI": [("coingecko", "sui")],  # yahoo SUI-USD is a different token (Salmonation)
    "ASTER": [("coingecko", "aster-2"), ("dexscreener", "solana/CQPBBre8Xuhp3yq2cTak3zdBxHjGC4foHhnw4Z6QMcBh")],
    "FARTCOIN": [("dexscreener", "solana/Bzc9NZfMqkXR6fz1DBph7BDf9BroyEf6pnzESP7v5iiw"), ("coingecko", "fartcoin")],
    "PUMP": [("dexscreener", "solana/2uF4Xh61rDwxnG9woyxsVQP7zuA6kLFpb3NvnRQeoiSd"), ("coingecko", "pump")],
    "JLP": [("dexscreener", "solana/5SHjDACvwtox5nY8kpWNYyaceWjtTG8C6L821D9Gtpjf")],
    "2Z": [("dexscreener", "solana/5Guq7ooZFtNju48kVNRzCVJmE9erW4DPcTQyrAk3z4UE")],
    # Pyth grant no longer covers crypto spot (2026-09: 403 "no grant accepts
    # this feed" even for BTC/ETH/SOL). Give every Pyth-fed ticker a free
    # source so Pyth is never a single point of failure. Pyth is still tried
    # first when it works, via the get_price_for_crypto fall-through.
    "BTC": [("yahoo", "BTC-USD"), ("coingecko", "bitcoin")],
    "ETH": [("yahoo", "ETH-USD"), ("coingecko", "ethereum")],
    "SOL": [("yahoo", "SOL-USD"), ("coingecko", "solana")],
    "DOGE": [("yahoo", "DOGE-USD"), ("coingecko", "dogecoin")],
    "BNB": [("yahoo", "BNB-USD"), ("coingecko", "binancecoin")],
    "WIF": [("coingecko", "dogwifcoin")],
    "GOLD": [("yahoo", "GC=F"), ("coingecko", "pax-gold")],
    "SILVER": [("yahoo", "SI=F")],
    "EURO": [("yahoo", "EURUSD=X")],
    "VOO": [("yahoo", "VOO")],
}


class PriceService:
    # Short TTL cache so 23 bots polling overlapping tickers (own price +
    # BTC/ETH/SOL conversions) don't hammer Yahoo/CoinGecko/DexScreener with
    # duplicate requests every cycle. Also collapses Pyth 403 retry noise.
    CACHE_TTL = int(os.environ.get("PRICE_CACHE_TTL", "25"))

    def __init__(self):
        self.feeds = self._load_feeds()
        self.dex_feeds = self._load_dex_feeds()
        self.session: Optional[aiohttp.ClientSession] = None
        self._cache: dict = {}  # ticker -> (monotonic_ts, price)
    
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
    
    def _load_dex_feeds(self) -> dict:
        """Load DexScreener pair feeds from environment.
        Format: CYB:solana/<pair_address>,FOO:ethereum/<pair_address>
        """
        feeds = {}
        feeds_str = os.environ.get("DEXSCREENER_FEEDS", "")
        
        for pair in feeds_str.split(","):
            pair = pair.strip()
            if not pair:
                continue
            if ":" in pair:
                name, chain_pair = pair.split(":", 1)
                feeds[name.strip().upper()] = chain_pair.strip()
        
        if feeds:
            logger.info(f"Loaded {len(feeds)} DexScreener pair feeds")
        return feeds
    
    async def _get_session(self) -> aiohttp.ClientSession:
        if self.session is None or self.session.closed:
            timeout = aiohttp.ClientTimeout(total=15)
            self.session = aiohttp.ClientSession(timeout=timeout)
        return self.session
    
    @staticmethod
    def _pyth_auth_headers() -> dict:
        """Build auth headers for Pyth API using PYTH_API_KEY if set."""
        api_key = os.environ.get("PYTH_API_KEY", "").strip()
        if api_key:
            return {"Authorization": f"Bearer {api_key}"}
        return {}
    
    async def get_shanghai_silver_price(self) -> Optional[float]:
        """Fetch Shanghai Silver price from goldsilver.ai."""
        try:
            session = await self._get_session()
            headers = {
                "User-Agent": "RustyMcPriceface/1.0 (crypto price bot)",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                "Accept-Language": "en-US,en;q=0.9",
            }
            async with session.get(GOLDSILVER_AI_URL, headers=headers) as resp:
                if resp.status != 200:
                    logger.warning(f"goldsilver.ai returned {resp.status}")
                    return None
                
                text = await resp.text()
                
                # Extract number after "Shanghai Spot" and "$"
                shanghai_price = self._extract_price_after(text, "Shanghai Spot")
                if shanghai_price and shanghai_price > 10:
                    logger.info(f"Shanghai Silver: ${shanghai_price}")
                    return shanghai_price
                
                logger.warning("Could not extract valid Shanghai price")
                return None
                
        except Exception as e:
            logger.error(f"Failed to fetch Shanghai Silver: {e}")
            return None
    
    def _extract_price_after(self, html: str, prefix: str) -> Optional[float]:
        """Extract dollar amount after a prefix."""
        pos = html.find(prefix)
        if pos == -1:
            return None
        
        after = html[pos:pos+200]
        
        # Find $ followed by number
        match = re.search(r'\$([0-9,]+\.?[0-9]*)', after)
        if match:
            price_str = match.group(1).replace(",", "")
            try:
                return float(price_str)
            except ValueError:
                return None
        return None
    
    async def get_yahoo_price(self, ticker: str) -> Optional[float]:
        """Fetch price from Yahoo Finance API."""
        try:
            session = await self._get_session()
            url = f"https://query1.finance.yahoo.com/v8/finance/chart/{ticker}?interval=1d&range=1d"
            headers = {
                "User-Agent": "Mozilla/5.0 (compatible; RustyMcPriceface/1.0)",
            }
            async with session.get(url, headers=headers) as resp:
                if resp.status != 200:
                    logger.warning(f"Yahoo returned {resp.status} for {ticker}")
                    return None
                
                data = await resp.json()
                
                # Extract price from Yahoo Finance JSON structure
                result = data.get("chart", {}).get("result", [])
                if not result:
                    logger.warning(f"No result from Yahoo for {ticker}")
                    return None
                
                meta = result[0].get("meta", {})
                price = meta.get("regularMarketPrice")
                
                if price:
                    logger.info(f"Yahoo {ticker}: {price}")
                    return float(price)
                
                logger.warning(f"No price in Yahoo response for {ticker}")
                return None
                
        except Exception as e:
            logger.error(f"Failed to fetch {ticker} from Yahoo: {e}")
            return None
    
    async def get_dexscreener_price(self, chain_pair: str) -> Optional[float]:
        """Fetch price for a DexScreener pair (e.g. 'solana/<pair_address>')."""
        try:
            session = await self._get_session()
            url = f"{DEXSCREENER_API_URL}/{chain_pair}"
            headers = {
                "User-Agent": "Mozilla/5.0 (compatible; RustyMcPriceface/1.0)",
            }
            async with session.get(url, headers=headers) as resp:
                if resp.status != 200:
                    logger.warning(f"DexScreener returned {resp.status} for {chain_pair}")
                    return None
                
                data = await resp.json()
                
                pair = data.get("pair") or {}
                price_str = pair.get("priceUsd")
                
                if not price_str:
                    logger.warning(f"No priceUsd in DexScreener response for {chain_pair}")
                    return None
                
                price = float(price_str)
                if price <= 0:
                    logger.warning(f"Invalid DexScreener price {price} for {chain_pair}")
                    return None
                
                logger.info(f"DexScreener {chain_pair}: ${price}")
                return price
                
        except Exception as e:
            logger.error(f"Failed to fetch {chain_pair} from DexScreener: {e}")
            return None
    
    async def get_price(self, crypto: str) -> Optional[float]:
        """Get price for a single cryptocurrency (with a short TTL cache)."""
        crypto = crypto.upper()
        now = time.monotonic()
        cached = self._cache.get(crypto)
        if cached is not None:
            ts, price = cached
            if now - ts < self.CACHE_TTL:
                return price
        price = await self._get_price_uncached(crypto)
        if price is not None and price > 0:
            self._cache[crypto] = (now, price)
        return price

    async def _get_price_uncached(self, crypto: str) -> Optional[float]:
        """Get price for a single cryptocurrency (no cache)."""
        crypto = crypto.upper()
        
        # Special handling for Shanghai Silver (not in Pyth feeds)
        if crypto == "SSILVER":
            return await self.get_shanghai_silver_price()
        
        # Special handling for DXY (Yahoo Finance)
        if crypto == "DXY":
            return await self.get_yahoo_price("DX-Y.NYB")

        # Special handling for OIL (Yahoo Finance - commodity feeds are not
        # part of our Pyth Pro grant)
        if crypto == "OIL":
            return await self.get_yahoo_price("CL=F")
        
        # DexScreener pairs (e.g. CYB)
        if crypto in self.dex_feeds:
            return await self.get_dexscreener_price(self.dex_feeds[crypto])
        
        # Free fallback sources for feeds outside our Pyth grant
        if crypto in FALLBACK_SOURCES:
            price = await self.get_fallback_price(crypto)
            if price:
                return price
            # fall through to Pyth attempt as last resort
        
        if crypto not in self.feeds:
            logger.warning(f"No feed ID for {crypto}")
            return None
        
        feed_id = self.feeds[crypto]
        url = f"{HERMES_API_URL}?ids[]={feed_id}"
        
        try:
            session = await self._get_session()
            headers = self._pyth_auth_headers()
            async with session.get(url, headers=headers) as resp:
                if resp.status != 200:
                    if resp.status == 401:
                        logger.error(
                            f"Pyth API returned 401 for {crypto} - PYTH_API_KEY missing or invalid"
                        )
                    else:
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
    
    async def get_coingecko_price(self, coin_id: str) -> Optional[float]:
        """Fetch USD price from CoinGecko free API (no key required)."""
        try:
            session = await self._get_session()
            url = f"https://api.coingecko.com/api/v3/simple/price?ids={coin_id}&vs_currencies=usd"
            async with session.get(url) as resp:
                if resp.status != 200:
                    logger.warning(f"CoinGecko returned {resp.status} for {coin_id}")
                    return None
                data = await resp.json()
                price = data.get(coin_id, {}).get("usd")
                return float(price) if price else None
        except Exception as e:
            logger.error(f"Failed to fetch {coin_id} from CoinGecko: {e}")
            return None

    async def get_fallback_price(self, crypto: str) -> Optional[float]:
        """Try each configured fallback source in order for a ticker."""
        for source, symbol in FALLBACK_SOURCES.get(crypto, []):
            try:
                if source == "yahoo":
                    price = await self.get_yahoo_price(symbol)
                elif source == "coingecko":
                    price = await self.get_coingecko_price(symbol)
                elif source == "dexscreener":
                    price = await self.get_dexscreener_price(symbol)
                else:
                    price = None
                if price and price > 0:
                    logger.info(f"Fetched {crypto} via {source}: ${price}")
                    return price
            except Exception as e:
                logger.warning(f"Fallback {source} failed for {crypto}: {e}")
        return None

    async def close(self):
        """Close the HTTP session."""
        if self.session and not self.session.closed:
            await self.session.close()
