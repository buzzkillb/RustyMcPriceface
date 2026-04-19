"""
PostgreSQL database operations using asyncpg.
"""
import logging
import os
from typing import Optional

import asyncpg

logger = logging.getLogger(__name__)


class Database:
    def __init__(self):
        self.pool: Optional[asyncpg.Pool] = None
        self.dsn = os.environ.get(
            "DATABASE_URL",
            "postgresql://postgres:postgres@postgres:5432/pricebot"
        )
    
    async def connect(self):
        """Connect to PostgreSQL and create tables."""
        try:
            self.pool = await asyncpg.create_pool(
                self.dsn,
                min_size=2,
                max_size=10,
            )
            await self._create_tables()
            logger.info("Connected to PostgreSQL")
        except Exception as e:
            logger.error(f"Failed to connect to database: {e}")
            raise
    
    async def disconnect(self):
        """Close database connection."""
        if self.pool:
            await self.pool.close()
            logger.info("Disconnected from PostgreSQL")
    
    async def _create_tables(self):
        """Create necessary tables if they don't exist."""
        async with self.pool.acquire() as conn:
            try:
                await conn.execute("""
                    CREATE TABLE IF NOT EXISTS prices (
                        id BIGSERIAL PRIMARY KEY,
                        crypto_name TEXT NOT NULL,
                        price REAL NOT NULL,
                        timestamp BIGINT NOT NULL,
                        created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
                    )
                """)
            except Exception as e:
                if "already exists" not in str(e):
                    logger.warning(f"Table creation warning (may be OK): {e}")
            
            try:
                await conn.execute("""
                    CREATE INDEX IF NOT EXISTS idx_prices_crypto_timestamp 
                        ON prices(crypto_name, timestamp DESC)
                """)
            except Exception as e:
                logger.warning(f"Index creation warning (may be OK): {e}")
            
            try:
                await conn.execute("""
                    CREATE TABLE IF NOT EXISTS price_aggregates (
                        id BIGSERIAL PRIMARY KEY,
                        crypto_name TEXT NOT NULL,
                        bucket_start BIGINT NOT NULL,
                        bucket_duration INTEGER NOT NULL,
                        open_price REAL NOT NULL,
                        high_price REAL NOT NULL,
                        low_price REAL NOT NULL,
                        close_price REAL NOT NULL,
                        avg_price REAL NOT NULL,
                        sample_count INTEGER NOT NULL,
                        created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
                    )
                """)
            except Exception as e:
                if "already exists" not in str(e):
                    logger.warning(f"Table creation warning (may be OK): {e}")
            
            try:
                await conn.execute("""
                    CREATE INDEX IF NOT EXISTS idx_aggregates_crypto_bucket 
                        ON price_aggregates(crypto_name, bucket_start, bucket_duration)
                """)
            except Exception as e:
                logger.warning(f"Index creation warning (may be OK): {e}")
            
            logger.info("Database tables initialized")
    
    async def save_price(self, crypto_name: str, price: float) -> bool:
        """Save a price to the database."""
        if price <= 0:
            return False
        
        import time
        timestamp = int(time.time())
        
        async with self.pool.acquire() as conn:
            await conn.execute("""
                INSERT INTO prices (crypto_name, price, timestamp)
                VALUES ($1, $2, $3)
            """, crypto_name.upper(), price, timestamp)
        
        return True
    
    async def get_latest_price(self, crypto_name: str) -> Optional[float]:
        """Get the latest price for a cryptocurrency."""
        async with self.pool.acquire() as conn:
            row = await conn.fetchrow("""
                SELECT price FROM prices 
                WHERE crypto_name = $1 
                ORDER BY timestamp DESC 
                LIMIT 1
            """, crypto_name.upper())
            
            if row:
                return float(row['price'])
            return None
    
    async def get_price_history(self, crypto_name: str, hours: int = 24) -> list:
        """Get price history for a cryptocurrency."""
        import time
        cutoff = int(time.time()) - (hours * 3600)
        
        async with self.pool.acquire() as conn:
            rows = await conn.fetch("""
                SELECT timestamp, price FROM prices
                WHERE crypto_name = $1 AND timestamp > $2
                ORDER BY timestamp ASC
            """, crypto_name.upper(), cutoff)
            
            return [(r['timestamp'], r['price']) for r in rows]
    
    async def get_all_latest_prices(self) -> dict:
        """Get latest price for all cryptocurrencies."""
        async with self.pool.acquire() as conn:
            rows = await conn.fetch("""
                SELECT DISTINCT ON (crypto_name) 
                    crypto_name, price, timestamp
                FROM prices
                ORDER BY crypto_name, timestamp DESC
            """)
            
            return {r['crypto_name']: float(r['price']) for r in rows}
