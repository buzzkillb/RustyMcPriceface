"""
Discord Bot for cryptocurrency price tracking.
Uses discord.py, asyncpg, and Pyth Network API.
"""
import asyncio
import logging
import os
import sys
from dataclasses import dataclass
from typing import Optional

import discord
from discord import app_commands
from dotenv import load_dotenv

from database import Database
from price_service import PriceService

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)
logger = logging.getLogger(__name__)

load_dotenv()


@dataclass
class BotConfig:
    name: str
    token: str
    crypto: str
    feed_id: str
    pyth_feed_id: Optional[str] = None


def load_bot_configs() -> list[BotConfig]:
    """Load bot configurations from environment variables."""
    configs = []
    
    # Check for DISCORD_TOKEN_BTC, DISCORD_TOKEN_ETH, etc.
    for key, value in os.environ.items():
        if key.startswith("DISCORD_TOKEN_") and value:
            name = key.replace("DISCORD_TOKEN_", "")
            crypto = os.environ.get(f"CRYPTO_{name}", name.lower())
            feed_id = os.environ.get(f"FEED_ID_{name}", "")
            configs.append(BotConfig(
                name=name,
                token=value,
                crypto=crypto,
                feed_id=feed_id,
            ))
    
    # Also check for generic DISCORD_TOKEN
    if not configs:
        token = os.environ.get("DISCORD_TOKEN", "")
        if token:
            crypto = os.environ.get("CRYPTO_NAME", "BTC").lower()
            feed_id = os.environ.get("PYTH_FEED_ID", "")
            configs.append(BotConfig(
                name="DEFAULT",
                token=token,
                crypto=crypto,
                feed_id=feed_id,
            ))
    
    return configs


class PriceBot(discord.Client):
    def __init__(self, config: BotConfig, db: Database, price_service: PriceService):
        super().__init__(intents=discord.Intents.default())
        self.config = config
        self.db = db
        self.price_service = price_service
        self.tree = app_commands.CommandTree(self)
        
    async def setup_hook(self):
        await self.tree.sync()
        logger.info(f"Synced commands for {self.config.name}")

    async def on_ready(self):
        logger.info(f"Logged in as {self.user} ({self.user.id}) for {self.config.name}")
        await self.start_price_updates()

    async def start_price_updates(self):
        """Background task to update price periodically."""
        async def update_loop():
            interval = int(os.environ.get("UPDATE_INTERVAL_SECONDS", "12"))
            while True:
                try:
                    price = await self.price_service.get_price(self.config.crypto)
                    if price:
                        await self.db.save_price(self.config.crypto, price)
                        logger.debug(f"Saved {self.config.crypto} price: ${price}")
                except Exception as e:
                    logger.error(f"Failed to update price: {e}")
                await asyncio.sleep(interval)
        
        asyncio.create_task(update_loop())


class PriceGroup(app_commands.Group):
    def __init__(self, db: Database, price_service: PriceService):
        super().__init__(name="price", description="Crypto price commands")
        self.db = db
        self.price_service = price_service
    
    @app_commands.command()
    async def current(self, interaction: discord.Interaction, crypto: str = None):
        """Get current price of a cryptocurrency."""
        crypto = crypto or os.environ.get("DEFAULT_CRYPTO", "BTC")
        crypto = crypto.upper()
        
        try:
            price = await self.db.get_latest_price(crypto)
            if price:
                await interaction.response.send_message(f"{crypto}: ${price:,.2f}")
            else:
                # Try to fetch fresh price
                fresh_price = await self.price_service.get_price(crypto)
                if fresh_price:
                    await self.db.save_price(crypto, fresh_price)
                    await interaction.response.send_message(f"{crypto}: ${fresh_price:,.2f}")
                else:
                    await interaction.response.send_message(f"No price data for {crypto}")
        except Exception as e:
            logger.error(f"Price command failed: {e}")
            await interaction.response.send_message(f"Error: {e}")


async def main(config: BotConfig):
    db = Database()
    await db.connect()
    
    price_service = PriceService()
    
    client = PriceBot(config, db, price_service)
    
    try:
        await client.start(config.token)
    except discord.LoginFailure:
        logger.error(f"Failed to login for {config.name} - invalid token?")
    finally:
        await db.disconnect()


if __name__ == "__main__":
    configs = load_bot_configs()
    
    if not configs:
        logger.error("No bot configurations found!")
        logger.error("Set DISCORD_TOKEN or DISCORD_TOKEN_BTC, etc.")
        sys.exit(1)
    
    logger.info(f"Found {len(configs)} bot configuration(s)")
    
    # Run all bots concurrently
    tasks = [main(cfg) for cfg in configs]
    asyncio.gather(*tasks)
