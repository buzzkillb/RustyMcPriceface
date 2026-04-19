"""
Discord Bot for cryptocurrency price tracking.
Uses discord.py, asyncpg, and Pyth Network API.
"""
import asyncio
import io
import logging
import os
import sys
import time
from dataclasses import dataclass
from typing import List, Optional

import discord
from discord import app_commands
from dotenv import load_dotenv

from database import Database
from price_service import PriceService
from chart_service import ChartService

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
    def __init__(self, config: BotConfig, db: Database, price_service: PriceService, chart_service: ChartService):
        intents = discord.Intents.default()
        super().__init__(intents=intents)
        self.config = config
        self.db = db
        self.price_service = price_service
        self.chart_service = chart_service
        self.tree = app_commands.CommandTree(self)
        
    async def setup_hook(self):
        price_group = PriceGroup(self.db, self.price_service, self.config.crypto)
        self.tree.add_command(price_group)
        
        chart_group = ChartGroup(self.db, self.chart_service, self.config.crypto)
        self.tree.add_command(chart_group)
        
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
    
    async def get_price_for_crypto(self, crypto: str) -> Optional[float]:
        """Get price, using database fallback for SSILVER."""
        price = await self.price_service.get_price(crypto)
        
        # SSILVER: only use if it looks valid (silver is ~$30+, so > 10)
        if crypto == "SSILVER" and (price is None or price < 10):
            db_price = await self.db.get_latest_price(crypto)
            if db_price and db_price > 10:
                logger.debug(f"SSILVER: Using cached price ${db_price}")
                return db_price
            logger.warning(f"SSILVER: No valid price (got {price}), skipping update")
            return None
        
        if price is None or price <= 0:
            db_price = await self.db.get_latest_price(crypto)
            if db_price and db_price > 0:
                logger.debug(f"Using cached {crypto} price: ${db_price}")
                return db_price
            return None
        return price

    async def get_conversion_prices(self) -> dict:
        """Get BTC, ETH, SOL prices for conversion."""
        prices = {}
        for ticker in ["BTC", "ETH", "SOL"]:
            try:
                p = await self.price_service.get_price(ticker)
                if p and p > 0:
                    prices[ticker] = p
                else:
                    db_p = await self.db.get_latest_price(ticker)
                    if db_p and db_p > 0:
                        prices[ticker] = db_p
            except Exception as e:
                logger.debug(f"Could not get {ticker} price: {e}")
        return prices

    async def update_discord_presence(self, price: float, change_percent: float, display_crypto: str, conversions: dict, show_index: int):
        """Update nickname and custom status."""
        try:
            guilds = self.guilds
            if not guilds:
                return
            
            formatted_price = format_price(price)
            nickname = f"{display_crypto.upper()} {formatted_price}"
            
            # Cycle through: BTC value, ETH value, SOL value, 1h%
            tickers = ["BTC", "ETH", "SOL"]
            ticker = tickers[show_index % 3]
            
            if ticker in conversions and conversions[ticker] > 0 and display_crypto.upper() != ticker:
                converted = price / conversions[ticker]
                status_text = f"{converted:.6f} {ticker}"
            else:
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
            conversions = {}
            show_index = 0  # Cycles: 0=BTC, 1=ETH, 2=SOL, 3=1h%, then repeats
            
            while True:
                try:
                    price = await self.get_price_for_crypto(self.config.crypto)
                    if price and price > 0:
                        await self.db.save_price(self.config.crypto, price)
                        current_price = price
                        current_change = await self.get_1h_change(self.config.crypto)
                        conversions = await self.get_conversion_prices()
                    
                    if current_price:
                        await self.update_discord_presence(
                            current_price, 
                            current_change, 
                            self.config.crypto,
                            conversions,
                            show_index
                        )
                        show_index += 1
                        logger.debug(f"Updated {self.config.name}: {self.config.crypto} ${current_price} {current_change:+.2f}%")
                        
                except Exception as e:
                    logger.error(f"Failed to update for {self.config.name}: {e}")
                
                await asyncio.sleep(interval)
        
        asyncio.create_task(update_loop())


class ChartGroup(app_commands.Group):
    def __init__(self, db: Database, chart_service: ChartService, crypto_name: str):
        super().__init__(name="chart", description=f"{crypto_name} chart commands")
        self.db = db
        self.chart_service = chart_service
        self.crypto_name = crypto_name
    
    @app_commands.command()
    async def price(self, interaction: discord.Interaction, hours: int = 24):
        """Generate price chart."""
        await self._send_chart(interaction, self.crypto_name, hours)
    
    async def _send_chart(self, interaction: discord.Interaction, crypto: str, hours: int):
        await interaction.response.defer()
        
        try:
            chart_bytes = await self.chart_service.get_chart_bytes(self.db, crypto, hours)
            
            if not chart_bytes:
                await interaction.followup.send(f"No price data available for {crypto} (need at least 2 data points)")
                return
            
            buf = io.BytesIO(chart_bytes)
            buf.name = f"{crypto.lower()}_chart.png"
            file = discord.File(buf, filename=buf.name)
            
            await interaction.followup.send(
                content=f"**{crypto.upper()} - {hours}h Price Chart**",
                file=file
            )
        except Exception as e:
            logger.error(f"Chart command failed for {crypto}: {e}")
            await interaction.followup.send(f"Error generating chart: {e}")





class PriceGroup(app_commands.Group):
    def __init__(self, db: Database, price_service: PriceService, default_crypto: str):
        super().__init__(name="price", description="Crypto price commands")
        self.db = db
        self.price_service = price_service
        self.default_crypto = default_crypto.upper()
    
    def _get_change(self, history: list) -> float:
        if len(history) < 2:
            return 0.0
        oldest = history[0][1]
        newest = history[-1][1]
        if oldest <= 0:
            return 0.0
        return ((newest - oldest) / oldest) * 100
    
    @app_commands.command()
    async def current(self, interaction: discord.Interaction, crypto: str = None):
        """Get current price of a cryptocurrency with conversions."""
        crypto = (crypto or self.default_crypto).upper()
        
        try:
            price = await self.db.get_latest_price(crypto)
            if not price:
                fresh_price = await self.price_service.get_price(crypto)
                if fresh_price:
                    await self.db.save_price(crypto, fresh_price)
                    price = fresh_price
                else:
                    await interaction.response.send_message(f"No price data for {crypto}")
                    return
            
            conversions = {}
            for ticker in ["BTC", "ETH", "SOL"]:
                try:
                    conv_price = await self.price_service.get_price(ticker)
                    if conv_price and conv_price > 0:
                        conversions[ticker] = conv_price
                    else:
                        db_p = await self.db.get_latest_price(ticker)
                        if db_p and db_p > 0:
                            conversions[ticker] = db_p
                except:
                    pass
            
            history_24h = await self.db.get_price_history(crypto, hours=24)
            history_7d = await self.db.get_price_history(crypto, hours=168)
            history_30d = await self.db.get_price_history(crypto, hours=720)
            
            change_24h = self._get_change(history_24h)
            change_7d = self._get_change(history_7d)
            change_30d = self._get_change(history_30d)
            
            def change_block(changes: float, label: str) -> str:
                color = "🟢" if changes >= 0 else "🔴"
                sign = "+" if changes >= 0 else ""
                return f"**{label}**\n{color} {sign}{changes:.2f}%"
            
            embed = discord.Embed(
                title=f"{crypto}",
                color=0x00ff00 if change_24h >= 0 else 0xff0000
            )
            
            embed.add_field(
                name="USD",
                value=f"**${price:,.6f}**" if price < 1 else f"**${price:,.2f}**" if price >= 100 else f"**${price:,.4f}**",
                inline=False
            )
            
            embed.add_field(
                name="24h",
                value=change_block(change_24h, ""),
                inline=True
            )
            
            embed.add_field(
                name="7d",
                value=change_block(change_7d, ""),
                inline=True
            )
            
            embed.add_field(
                name="30d",
                value=change_block(change_30d, ""),
                inline=True
            )
            
            conversions_text = ""
            if "BTC" in conversions and conversions["BTC"] > 0 and crypto != "BTC":
                btc_val = price / conversions["BTC"]
                conversions_text += f"BTC: `{btc_val:.8f}`\n"
            if "ETH" in conversions and conversions["ETH"] > 0 and crypto != "ETH":
                eth_val = price / conversions["ETH"]
                conversions_text += f"ETH: `{eth_val:.8f}`\n"
            if "SOL" in conversions and conversions["SOL"] > 0 and crypto != "SOL":
                sol_val = price / conversions["SOL"]
                conversions_text += f"SOL: `{sol_val:.8f}`\n"
            
            if conversions_text:
                embed.add_field(
                    name="Conversions",
                    value=conversions_text,
                    inline=False
                )
            
            await interaction.response.send_message(embed=embed)
            
        except Exception as e:
            logger.error(f"Price command failed: {e}")
            await interaction.response.send_message(f"Error: {e}")


async def run_bot(cfg: BotConfig):
    """Run a single bot with its own db connection."""
    db = Database()
    await db.connect()
    price_service = PriceService()
    chart_service = ChartService()
    
    client = PriceBot(cfg, db, price_service, chart_service)
    
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
