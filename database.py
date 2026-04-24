"""
PostgreSQL database operations using asyncpg.
"""

import asyncio
import logging
import os
import time
from typing import Optional

import asyncpg

logger = logging.getLogger(__name__)

ONE_HOUR = 3600
ONE_DAY = 86400
ONE_WEEK = 604800


class Database:
    def __init__(self):
        self.pool: Optional[asyncpg.Pool] = None
        self.dsn = os.environ.get("DATABASE_URL")
        if not self.dsn:
            raise ValueError("DATABASE_URL environment variable is required")
        self._last_cleanup = 0
        self._last_aggregate = 0

    async def connect(self):
        """Connect to PostgreSQL and create tables."""
        try:
            self.pool = await asyncpg.create_pool(
                self.dsn,
                min_size=1,
                max_size=2,
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
                        price DOUBLE PRECISION NOT NULL,
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

    async def _should_run_task(self, last_run: float, interval: int) -> bool:
        """Check if a task should run based on interval."""
        return (time.time() - last_run) >= interval

    async def _aggregate_prices(self):
        """Aggregate raw prices into time buckets."""
        now = int(time.time())

        buckets = [
            (300, 7 * ONE_DAY),  # 5-min aggregates for 7 days
            (ONE_HOUR, 30 * ONE_DAY),  # hourly aggregates for 30 days
            (ONE_DAY, 365 * ONE_DAY),  # daily aggregates for 1 year
            (ONE_WEEK, 5 * 365 * ONE_DAY),  # weekly for 5 years
        ]

        async with self.pool.acquire() as conn:
            for duration, max_age in buckets:
                bucket_start = (now // duration) * duration
                cutoff = now - max_age

                await conn.execute(
                    """
                    INSERT INTO price_aggregates 
                    (crypto_name, bucket_start, bucket_duration, open_price, high_price, 
                     low_price, close_price, avg_price, sample_count)
                    SELECT 
                        crypto_name,
                        $1 as bucket_start,
                        $2 as bucket_duration,
                        (ARRAY_AGG(price ORDER BY timestamp ASC))[1] as open_price,
                        MAX(price) as high_price,
                        MIN(price) as low_price,
                        (ARRAY_AGG(price ORDER BY timestamp DESC))[1] as close_price,
                        AVG(price) as avg_price,
                        COUNT(*) as sample_count
                    FROM prices
                    WHERE timestamp >= $3 AND timestamp < $1
                    AND NOT EXISTS (
                        SELECT 1 FROM price_aggregates 
                        WHERE crypto_name = prices.crypto_name 
                        AND bucket_start = $1 
                        AND bucket_duration = $2
                    )
                    GROUP BY crypto_name
                    HAVING COUNT(*) > 0
                """,
                    bucket_start,
                    duration,
                    cutoff,
                )

    async def _cleanup_old_data(self):
        """Delete price data older than retention period."""
        now = int(time.time())
        retention = 5 * 365 * ONE_DAY  # 5 years

        async with self.pool.acquire() as conn:
            await conn.execute(
                """
                DELETE FROM prices WHERE timestamp < $1
            """,
                now - retention,
            )

            await conn.execute(
                """
                DELETE FROM price_aggregates WHERE bucket_start < $1
            """,
                now - retention,
            )

            logger.info("Cleanup: deleted old price data (retention: 5 years)")

    async def _run_maintenance(self):
        """Run periodic maintenance tasks."""
        now = time.time()

        if self._should_run_task(self._last_aggregate, ONE_HOUR):
            await self._aggregate_prices()
            self._last_aggregate = now

        if self._should_run_task(self._last_cleanup, ONE_DAY):
            await self._cleanup_old_data()
            self._last_cleanup = now

    async def save_price(self, crypto_name: str, price: float) -> bool:
        """Save a price to the database."""
        if price <= 0:
            return False

        timestamp = int(time.time())

        async with self.pool.acquire() as conn:
            await conn.execute(
                """
                INSERT INTO prices (crypto_name, price, timestamp)
                VALUES ($1, $2, $3)
            """,
                crypto_name.upper(),
                price,
                timestamp,
            )

        asyncio.create_task(self._run_maintenance())
        return True

    async def get_latest_price(self, crypto_name: str) -> Optional[float]:
        """Get the latest price for a cryptocurrency."""
        async with self.pool.acquire() as conn:
            row = await conn.fetchrow(
                """
                SELECT price FROM prices 
                WHERE crypto_name = $1 
                ORDER BY timestamp DESC 
                LIMIT 1
            """,
                crypto_name.upper(),
            )

            if row:
                return float(row["price"])
            return None

    def _get_bucket_for_hours(self, hours: int) -> tuple:
        """Get appropriate bucket duration and SQL for given timeframe."""
        if hours <= 24:
            return (
                "raw",
                """
                SELECT timestamp, price FROM prices
                WHERE crypto_name = $1 AND timestamp > $2
                ORDER BY timestamp ASC
                LIMIT $3
            """,
            )
        elif hours <= 168:
            return (
                "5min",
                """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 300
                ORDER BY bucket_start ASC
                LIMIT $3
            """,
            )
        elif hours <= 720:
            return (
                "hourly",
                """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 3600
                ORDER BY bucket_start ASC
                LIMIT $3
            """,
            )
        elif hours <= 8760:
            return (
                "daily",
                """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 86400
                ORDER BY bucket_start ASC
                LIMIT $3
            """,
            )
        elif hours <= 43800:
            return (
                "weekly",
                """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 604800
                ORDER BY bucket_start ASC
                LIMIT $3
            """,
            )
        else:
            return (
                "monthly",
                """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 2592000
                ORDER BY bucket_start ASC
                LIMIT $3
            """,
            )

    async def get_price_history(
        self, crypto_name: str, hours: int = 24, limit: int = 2000
    ) -> list:
        """Get price history for a cryptocurrency using appropriate aggregation."""
        cutoff = int(time.time()) - (hours * 3600)
        bucket_type, query = self._get_bucket_for_hours(hours)

        async with self.pool.acquire() as conn:
            rows = await conn.fetch(query, crypto_name.upper(), cutoff, limit)

            if not rows and bucket_type != "raw":
                query = """
                    SELECT timestamp, price FROM prices
                    WHERE crypto_name = $1 AND timestamp > $2
                    ORDER BY timestamp ASC
                    LIMIT $3
                """
                rows = await conn.fetch(query, crypto_name.upper(), cutoff, limit)

            return [(r["timestamp"], float(r["price"])) for r in rows]

    async def get_all_latest_prices(self) -> dict:
        """Get latest price for all cryptocurrencies."""
        async with self.pool.acquire() as conn:
            rows = await conn.fetch("""
                SELECT DISTINCT ON (crypto_name) 
                    crypto_name, price, timestamp
                FROM prices
                ORDER BY crypto_name, timestamp DESC
            """)

            return {r["crypto_name"]: float(r["price"]) for r in rows}
