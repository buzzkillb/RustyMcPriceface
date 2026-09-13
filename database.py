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
        self._maintenance_lock = asyncio.Lock()
        self._maintenance_task: Optional[asyncio.Task] = None
    
    async def connect(self):
        """Connect to PostgreSQL and create tables."""
        try:
            self.pool = await asyncpg.create_pool(
                self.dsn,
                min_size=2,
                max_size=20,
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
                    CREATE INDEX IF NOT EXISTS idx_prices_timestamp 
                        ON prices(timestamp)
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
            
            try:
                await conn.execute("""
                    CREATE UNIQUE INDEX IF NOT EXISTS uq_aggregates_crypto_bucket
                        ON price_aggregates(crypto_name, bucket_start, bucket_duration)
                """)
            except Exception as e:
                # If duplicates already exist in old data, dedupe then retry once
                logger.warning(f"Unique index creation failed ({e}); attempting dedupe")
                try:
                    await conn.execute("""
                        DELETE FROM price_aggregates a
                        USING price_aggregates b
                        WHERE a.id > b.id
                          AND a.crypto_name = b.crypto_name
                          AND a.bucket_start = b.bucket_start
                          AND a.bucket_duration = b.bucket_duration
                    """)
                    await conn.execute("""
                        CREATE UNIQUE INDEX IF NOT EXISTS uq_aggregates_crypto_bucket
                            ON price_aggregates(crypto_name, bucket_start, bucket_duration)
                    """)
                except Exception as e2:
                    logger.warning(f"Aggregate dedupe/index retry failed (may be OK): {e2}")

            logger.info("Database tables initialized")
    
    async def _should_run_task(self, last_run: float, interval: int) -> bool:
        """Check if a task should run based on interval."""
        return (time.time() - last_run) >= interval
    
    async def _aggregate_prices(self):
        """Aggregate raw prices into per-bucket OHLCV rows.

        Groups raw prices into their OWN bucket (timestamp/duration)*duration so
        every bucket gets a row, and upserts so re-runs fill in buckets that
        closed after the previous pass. Uses integer division for the bucket
        math and ON CONFLICT DO UPDATE instead of NOT EXISTS (which prevented
        ever correcting or completing buckets).
        """
        now = int(time.time())

        buckets = [
            (300, 7 * ONE_DAY),      # 5-min aggregates for 7 days
            (ONE_HOUR, 30 * ONE_DAY),  # hourly aggregates for 30 days
            (ONE_DAY, 365 * ONE_DAY),  # daily aggregates for 1 year
            (ONE_WEEK, 5 * 365 * ONE_DAY),  # weekly for 5 years
        ]

        async with self.pool.acquire() as conn:
            for duration, max_age in buckets:
                cutoff = now - max_age

                # Upsert every bucket in the retention window. bucket_start is
                # derived per-row from the raw timestamp, so each 5-min/hour/
                # day/week gets its own row with its own OHLC.
                await conn.execute("""
                    INSERT INTO price_aggregates
                    (crypto_name, bucket_start, bucket_duration, open_price, high_price,
                     low_price, close_price, avg_price, sample_count)
                    SELECT
                        crypto_name,
                        (timestamp / $2) * $2 as bucket_start,
                        $2 as bucket_duration,
                        (ARRAY_AGG(price ORDER BY timestamp ASC))[1] as open_price,
                        MAX(price) as high_price,
                        MIN(price) as low_price,
                        (ARRAY_AGG(price ORDER BY timestamp DESC))[1] as close_price,
                        AVG(price) as avg_price,
                        COUNT(*)::int as sample_count
                    FROM prices
                    WHERE timestamp >= $1
                    GROUP BY crypto_name, bucket_start
                    HAVING COUNT(*) > 0
                    ON CONFLICT (crypto_name, bucket_start, bucket_duration) DO UPDATE SET
                        open_price = EXCLUDED.open_price,
                        high_price = EXCLUDED.high_price,
                        low_price = EXCLUDED.low_price,
                        close_price = EXCLUDED.close_price,
                        avg_price = EXCLUDED.avg_price,
                        sample_count = EXCLUDED.sample_count,
                        created_at = CURRENT_TIMESTAMP
                """, cutoff, duration)
    
    async def _cleanup_old_data(self):
        """Delete price data older than retention period.
        Raw prices are only needed for short-term charts and re-aggregation,
        so keep them for 31 days. Aggregates are stored for 5 years.
        """
        now = int(time.time())
        raw_retention = 31 * ONE_DAY
        agg_retention = 5 * 365 * ONE_DAY
        
        async with self.pool.acquire() as conn:
            await conn.execute("""
                DELETE FROM prices WHERE timestamp < $1
            """, now - raw_retention)
            
            await conn.execute("""
                DELETE FROM price_aggregates WHERE bucket_start < $1
            """, now - agg_retention)
            
            logger.info(
                "Cleanup: kept raw prices for %d days, aggregates for %d years",
                31, 5,
            )
    
    async def _run_maintenance(self):
        """Run periodic maintenance tasks.
        Uses a lock so many concurrent save_price calls (one per bot) can't
        stampede and run multiple expensive aggregation queries at once.
        Timestamps are committed AFTER each subtask succeeds, so a failed or
        slow pass doesn't skip the next hour's work. Exceptions are logged
        instead of vanishing (unreferenced tasks die silently).
        """
        if self._maintenance_lock.locked():
            return
        async with self._maintenance_lock:
            if self._should_run_task(self._last_aggregate, ONE_HOUR):
                try:
                    await self._aggregate_prices()
                    self._last_aggregate = time.time()
                except Exception as e:
                    logger.error(f"Price aggregation failed: {e}")

            if self._should_run_task(self._last_cleanup, ONE_DAY):
                try:
                    await self._cleanup_old_data()
                    self._last_cleanup = time.time()
                except Exception as e:
                    logger.error(f"Price cleanup failed: {e}")
    
    async def save_price(self, crypto_name: str, price: float) -> bool:
        """Save a price to the database."""
        if price <= 0:
            return False
        
        timestamp = int(time.time())
        
        async with self.pool.acquire() as conn:
            await conn.execute("""
                INSERT INTO prices (crypto_name, price, timestamp)
                VALUES ($1, $2, $3)
            """, crypto_name.upper(), price, timestamp)
        
        # Only spawn a maintenance task when something is actually due; keep a
        # reference + done-callback so the exception isn't lost to the GC.
        if (await self._should_run_task(self._last_aggregate, ONE_HOUR)
                or await self._should_run_task(self._last_cleanup, ONE_DAY)):
            self._maintenance_task = asyncio.create_task(self._run_maintenance())
            self._maintenance_task.add_done_callback(self._log_task_exception)
        return True

    @staticmethod
    def _log_task_exception(task: asyncio.Task):
        if not task.cancelled() and task.exception() is not None:
            logger.error(f"Maintenance task failed: {task.exception()}")
    
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
    
    def _get_bucket_for_hours(self, hours: int) -> tuple:
        """Get appropriate bucket duration and SQL for given timeframe."""
        if hours <= 24:
            return ("raw", """
                SELECT timestamp, price FROM prices
                WHERE crypto_name = $1 AND timestamp > $2
                ORDER BY timestamp ASC
                LIMIT $3
            """)
        elif hours <= 168:
            return ("5min", """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 300
                ORDER BY bucket_start ASC
                LIMIT $3
            """)
        elif hours <= 720:
            return ("hourly", """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 3600
                ORDER BY bucket_start ASC
                LIMIT $3
            """)
        elif hours <= 8760:
            return ("daily", """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 86400
                ORDER BY bucket_start ASC
                LIMIT $3
            """)
        elif hours <= 43800:
            return ("weekly", """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 604800
                ORDER BY bucket_start ASC
                LIMIT $3
            """)
        else:
            return ("monthly", """
                SELECT bucket_start as timestamp, avg_price as price 
                FROM price_aggregates
                WHERE crypto_name = $1 AND bucket_start > $2 AND bucket_duration = 2592000
                ORDER BY bucket_start ASC
                LIMIT $3
            """)
    
    async def get_price_history(self, crypto_name: str, hours: int = 24, limit: int = 2000) -> list:
        """Get price history for a cryptocurrency using appropriate aggregation.

        Fetches the NEWEST rows first (DESC) and reverses so callers get
        ascending order without dropping recent data when the window has more
        points than `limit`. Previously ASC + LIMIT kept only the oldest rows,
        silently chopping off the most recent hours/days of every chart.
        """
        cutoff = int(time.time()) - (hours * 3600)
        bucket_type, query = self._get_bucket_for_hours(hours)

        async with self.pool.acquire() as conn:
            rows = await conn.fetch(
                query,
                crypto_name.upper(), cutoff, limit
            )

            if not rows and bucket_type != "raw":
                query = """
                    SELECT timestamp, price FROM prices
                    WHERE crypto_name = $1 AND timestamp > $2
                    ORDER BY timestamp DESC
                    LIMIT $3
                """
                rows = await conn.fetch(query, crypto_name.upper(), cutoff, limit)

            # rows are newest-first from SQL; return oldest-first to callers
            return [
                (r['timestamp'], float(r['price']))
                for r in reversed(rows)
            ]
    
