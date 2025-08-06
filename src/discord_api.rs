use crate::errors::{BotError, BotResult};
use serenity::http::Http;
use serenity::model::id::GuildId;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};

const MAX_RETRIES: u32 = 3;
const RATE_LIMIT_DELAY_MS: u64 = 2000; // 2 seconds between Discord API calls

/// Discord API wrapper with rate limiting and error handling
pub struct DiscordApi {
    http: Arc<Http>,
}

impl DiscordApi {
    pub fn new(http: Arc<Http>) -> Self {
        Self { http }
    }

    /// Rate-limited Discord API call helper
    async fn rate_limited_call<F, Fut, T>(&self, mut operation: F) -> Result<T, serenity::Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, serenity::Error>>,
    {
        static LAST_CALL: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        
        // Enforce rate limiting
        let sleep_time = {
            let mut last_call = LAST_CALL.lock().map_err(|_| serenity::Error::Other("Mutex lock error"))?;
            let now = std::time::Instant::now();
            
            if let Some(last) = *last_call {
                let elapsed = last.elapsed();
                let min_interval = Duration::from_millis(RATE_LIMIT_DELAY_MS);
                if elapsed < min_interval {
                    let sleep_duration = min_interval - elapsed;
                    *last_call = Some(now);
                    Some(sleep_duration)
                } else {
                    *last_call = Some(now);
                    None
                }
            } else {
                *last_call = Some(now);
                None
            }
        };
        
        // Sleep outside the mutex lock if needed
        if let Some(duration) = sleep_time {
            debug!("Rate limiting: sleeping for {:?}", duration);
            sleep(duration).await;
        }
        
        // Execute the operation with retry logic
        for attempt in 1..=MAX_RETRIES {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                        let backoff_time = Duration::from_secs(2_u64.pow(attempt));
                        warn!("Rate limited, backing off for {:?} (attempt {})", backoff_time, attempt);
                        sleep(backoff_time).await;
                    } else if attempt < MAX_RETRIES {
                        warn!("Discord API call failed (attempt {}): {}", attempt, e);
                        sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        
        unreachable!()
    }

    /// Update bot nickname in a specific guild
    pub async fn update_nickname(&self, guild_id: GuildId, nickname: &str) -> BotResult<()> {
        let http_ref = self.http.clone();
        let nickname_owned = nickname.to_string();
        
        match self.rate_limited_call(|| {
            let http_clone = http_ref.clone();
            let nickname_clone = nickname_owned.clone();
            async move {
                http_clone.edit_nickname(guild_id, Some(&nickname_clone), None).await
            }
        }).await {
            Ok(_) => {
                debug!("Updated nickname in guild {}", guild_id);
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                    warn!("Rate limited while updating nickname in guild {}: {}", guild_id, e);
                } else {
                    warn!("Failed to update nickname in guild {}: {}", guild_id, e);
                }
                Err(BotError::Discord(e.to_string()))
            }
        }
    }

    /// Update nicknames in multiple guilds
    pub async fn update_nicknames_in_guilds(&self, guilds: &[GuildId], nickname: &str) -> Vec<BotResult<()>> {
        let mut results = Vec::new();
        
        for (index, guild_id) in guilds.iter().enumerate() {
            let result = self.update_nickname(*guild_id, nickname).await;
            
            match &result {
                Ok(_) => debug!("Updated nickname in guild {} ({}/{})", guild_id, index + 1, guilds.len()),
                Err(_) => {} // Error already logged in update_nickname
            }
            
            results.push(result);
        }
        
        results
    }
}