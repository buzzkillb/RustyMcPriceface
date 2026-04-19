"""
Discord Bot for cryptocurrency price tracking.
Uses discord.py, asyncpg, and Pyth Network API.
"""
import asyncio
import logging
import os
import sys
import time
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
    
    for key, value in os.environ.items():
        if key.startswith("DISCORD_TOKEN_") and value and key != "DISCORD_TOKEN":
            name = key.replace("DISCORD_TOKEN_", "")
            crypto = os.environ.get(f"CRYPTO_{name}", name.lower())
            feed_id = os.environ.get(f"FEED_ID_{name}", "")
            configs.append(BotConfig(
                name=name,
                token=value,
                crypto=crypto,
                feed_id=feed_id,
            ))
    
    return configs


def format_price(price: float) -> str:
    """Format price for display."""
    if price >= 1000:
        return f"${price:,.0f}"
    elif price >= 1:
        return f"${price:,.2f}"
    else:
        return f"${price:.6f}"


def calculate_change_percent(current: float, previous: float) -> float:
    """Calculate percentage change."""
    if previous <= 0:
        return 0.0
    return ((current - previous) / previous) * 100


class PriceBot(discord.Client):
    def __init__(self, config: BotConfig, db: Database, price_service: PriceService):
        intents = discord.Intents.default()
        super().__init__(intents=intents)
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

    async def get_1h_change(self, crypto: str) -> float:
        """Get 1 hour percentage change from database."""
        try:
            history = await self.db.get_price_history(crypto, hours=1)
            if len(history) >= 2:
                oldest = history[0][1]
                newest = history[-1][1]
                return calculate_change_percent(newest, oldest)
        except Exception as e:
            logger.debug(f"Could not get 1h change for {crypto}: {e}")
        return 0.0

    async def update_discord_presence(self, price: float, change_percent: float, display_crypto: str):
        """Update nickname and custom status."""
        try:
            guilds = self.guilds
            if not guilds:
                return
            
            formatted_price = format_price(price)
            nickname = f"{display_crypto} {formatted_price}"
            
            change_sign = "+" if change_percent >= 0 else ""
            status_text = f"{change_sign}{change_percent:.2f}% (1h)"
            
            activity = discord.Activity(
                type=discord.ActivityType.watching,
                name=status_text
            )
            
            for guild in guilds:
                member = guild.get_member(self.user.id)
                if member:
                    try:
                        await member.edit(nick=nickname)
                    except Exception as e:
                        logger.debug(f"Could not update nickname in {guild.name}: {e}")
            
            await self.change_presence(activity=activity)
            logger.debug(f"Updated {self.config.name}: {nickname} | {status_text}")
            
        except Exception as e:
            logger.error(f"Failed to update Discord presence: {e}")

    async def start_price_updates(self):
        """Background task to update price and Discord presence periodically."""
        async def update_loop():
            interval = int(os.environ.get("UPDATE_INTERVAL_SECONDS", "12"))
            current_price = None
            current_change = 0.0
            
            while True:
                try:
                    price = await self.price_service.get_price(self.config.crypto)
                    if price and price > 0:
                        await self.db.save_price(self.config.crypto, price)
                        current_price = price
                        current_change = await self.get_1h_change(self.config.crypto)
                    
                    if current_price:
                        await self.update_discord_presence(current_price, current_change, self.config.crypto)
                        logger.debug(f"Updated {self.config.name}: {self.config.crypto} ${current_price} {current_change:+.2f}%")
                        
                except Exception as e:
                    logger.error(f"Failed to update for {self.config.name}: {e}")
                
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
        crypto = (crypto or os.environ.get("DEFAULT_CRYPTO", "BTC")).upper()
        
        try:
            price = await self.db.get_latest_price(crypto)
            if price:
                await interaction.response.send_message(f"{crypto}: {format_price(price)}")
            else:
                fresh_price = await self.price_service.get_price(crypto)
                if fresh_price:
                    await self.db.save_price(crypto, fresh_price)
                    await interaction.response.send_message(f"{crypto}: {format_price(fresh_price)}")
                else:
                    await interaction.response.send_message(f"No price data for {crypto}")
        except Exception as e:
            logger.error(f"Price command failed: {e}")
            await interaction.response.send_message(f"Error: {e}")


async def run_bot(cfg: BotConfig):
    """Run a single bot with its own db connection."""
    db = Database()
    await db.connect()
    price_service = PriceService()
    
    client = PriceBot(cfg, db, price_service)
    
    while True:
        try:
            logger.info(f"Starting bot {cfg.name}...")
            await client.start(cfg.token)
            logger.warning(f"Bot {cfg.name} disconnected, reconnecting in 5s...")
        except discord.LoginFailure:
            logger.error(f"Bot {cfg.name} login failed - invalid token")
            break
        except KeyboardInterrupt:
            logger.info(f"Bot {cfg.name} shutting down...")
            break
        except Exception as e:
            logger.error(f"Bot {cfg.name} error: {e}, reconnecting in 5s...")
        
        await asyncio.sleep(5)
    
    await db.disconnect()
    logger.info(f"Bot {cfg.name} stopped")


if __name__ == "__main__":
    configs = load_bot_configs()
    
    if not configs:
        logger.error("No bot configurations found!")
        sys.exit(1)
    
    logger.info(f"Found {len(configs)} bot configuration(s)")
    
    async def run_all():
        tasks = [asyncio.create_task(run_bot(cfg)) for cfg in configs]
        
        try:
            await asyncio.gather(*tasks)
        except KeyboardInterrupt:
            logger.info("Shutting down...")
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
    
    asyncio.run(run_all())
