"""
Chart generation service using matplotlib.
"""
import io
import logging
import os
from datetime import datetime
from typing import Optional

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
import numpy as np
from matplotlib.patches import FancyBboxPatch

logger = logging.getLogger(__name__)


class ChartService:
    
    def _format_price(self, price: float) -> str:
        """Format price nicely."""
        if price >= 10000:
            return f"${price:,.0f}"
        elif price >= 100:
            return f"${price:,.2f}"
        elif price >= 1:
            return f"${price:.4f}"
        else:
            return f"${price:.6f}"
    
    def generate_price_chart(
        self,
        timestamps: list,
        prices: list,
        crypto_name: str,
        timeframe: str = "24h"
    ) -> Optional[bytes]:
        """Generate a price chart and return as PNG bytes."""
        if not timestamps or not prices or len(timestamps) < 2:
            return None
        
        try:
            fig = plt.figure(figsize=(14, 8), facecolor='#0d1117')
            ax = fig.add_subplot(111, facecolor='#0d1117')
            
            dates = [datetime.fromtimestamp(ts) for ts in timestamps]
            prices_arr = np.array(prices)
            
            start_price = prices[0]
            end_price = prices[-1]
            change = ((end_price - start_price) / start_price) * 100
            change_color = '#00d26a' if change >= 0 else '#ff4757'
            line_color = '#00d26a' if change >= 0 else '#ff4757'
            
            ax.plot(dates, prices_arr, color=line_color, linewidth=2.5, zorder=3)
            
            ax.fill_between(dates, prices_arr, 
                           alpha=0.15, color=line_color, zorder=2)
            
            gradient = np.linspace(0, 1, 256).reshape(256, 1)
            gradient_plot = np.vstack([prices_arr]*256)
            
            ax.scatter(dates, prices_arr, color=line_color, s=30, zorder=4, 
                      edgecolors='#0d1117', linewidths=0.5)
            
            min_idx = np.argmin(prices_arr)
            max_idx = np.argmax(prices_arr)
            
            ax.scatter(dates[min_idx], prices_arr[min_idx], color='#ff4757', 
                      s=100, zorder=5, marker='v', edgecolors='white', linewidths=1)
            ax.scatter(dates[max_idx], prices_arr[max_idx], color='#00d26a', 
                      s=100, zorder=5, marker='^', edgecolors='white', linewidths=1)
            
            ax.annotate(f'LOW\n{self._format_price(prices_arr[min_idx])}',
                        xy=(dates[min_idx], prices_arr[min_idx]),
                        xytext=(10, -30), textcoords='offset points',
                        fontsize=8, color='#888888',
                        bbox=dict(boxstyle='round,pad=0.3', facecolor='#161b22', 
                                 edgecolor='#30363d', pad=0.3),
                        arrowprops=dict(arrowstyle='->', color='#ff4757', lw=1))
            
            ax.annotate(f'HIGH\n{self._format_price(prices_arr[max_idx])}',
                        xy=(dates[max_idx], prices_arr[max_idx]),
                        xytext=(10, 20), textcoords='offset points',
                        fontsize=8, color='#888888',
                        bbox=dict(boxstyle='round,pad=0.3', facecolor='#161b22', 
                                 edgecolor='#30363d', pad=0.3),
                        arrowprops=dict(arrowstyle='->', color='#00d26a', lw=1))
            
            ax.annotate(f'{self._format_price(end_price)}',
                       xy=(dates[-1], prices_arr[-1]),
                       xytext=(10, 0), textcoords='offset points',
                       fontsize=11, color=line_color, fontweight='bold',
                       bbox=dict(boxstyle='round,pad=0.4', facecolor='#161b22', 
                                edgecolor=line_color, pad=0.4),
                       arrowprops=dict(arrowstyle='->', color=line_color, lw=1))
            
            ax.set_title(
                f'{crypto_name}  |  {timeframe}  |  {change:+.2f}%',
                fontsize=16, fontweight='bold', color='white',
                pad=20, loc='center'
            )
            
            ax.set_ylabel('Price (USD)', fontsize=11, color='#888888', labelpad=10)
            ax.set_xlabel('Time', fontsize=11, color='#888888', labelpad=10)
            
            ax.tick_params(colors='#888888', labelsize=9)
            ax.spines['bottom'].set_color('#30363d')
            ax.spines['left'].set_color('#30363d')
            ax.spines['top'].set_visible(False)
            ax.spines['right'].set_visible(False)
            
            if hours <= 24:
                ax.xaxis.set_major_formatter(mdates.DateFormatter('%H:%M'))
            elif hours <= 168:
                ax.xaxis.set_major_formatter(mdates.DateFormatter('%b %d %H:%M'))
            else:
                ax.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))
            ax.xaxis.set_major_locator(mdates.AutoDateLocator())
            
            price_min = min(prices_arr)
            price_max = max(prices_arr)
            price_range = price_max - price_min
            ax.set_ylim(price_min - price_range * 0.1, price_max + price_range * 0.15)
            
            fig.autofmt_xdate()
            
            ax.grid(True, alpha=0.1, color='#30363d', linestyle='--', zorder=1)
            
            for spine in ax.spines.values():
                spine.set_zorder(0)
            
            buf = io.BytesIO()
            plt.savefig(
                buf,
                format='png',
                bbox_inches='tight',
                facecolor='#0d1117',
                edgecolor='none',
                dpi=100
            )
            buf.seek(0)
            plt.close(fig)
            
            return buf.read()
            
        except Exception as e:
            logger.error(f"Failed to generate chart for {crypto_name}: {e}")
            return None
    
    async def get_chart_bytes(
        self,
        db,
        crypto: str,
        hours: int = 24,
        timeframe_str: str = None
    ) -> Optional[bytes]:
        """Get price history from DB and generate chart."""
        history = await db.get_price_history(crypto, hours=hours)
        
        if not history or len(history) < 2:
            return None
        
        timestamps = [h[0] for h in history]
        prices = [float(h[1]) for h in history]
        
        if not timeframe_str:
            timeframe_str = f"{hours}h" if hours <= 24 else f"{hours//24}d"
        return self.generate_price_chart(timestamps, prices, crypto.upper(), timeframe_str)