use crate::errors::{BotError, BotResult};
use serenity::http::Http;
use serenity::model::id::GuildId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tracing::{debug, warn};

const MAX_RETRIES: u32 = 3;
const MAX_CONCURRENT_CALLS: usize = 2;
const RATE_LIMIT_DELAY_MS: u64 = 2000; // 2 seconds between Discord API calls

/// Discord API wrapper with rate limiting and error handling
#[derive(Clone)]
pub struct DiscordApi {
    http: Arc<Http>,
    semaphore: Arc<Semaphore>,
}

impl DiscordApi {
    pub fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS)),
        }
    }

    /// Rate-limited Discord API call helper
    async fn rate_limited_call<F, Fut, T>(&self, mut operation: F) -> Result<T, serenity::Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, serenity::Error>>,
    {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| serenity::Error::Other("Semaphore acquire error"))?;

        // Enforce minimum delay between calls
        sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS)).await;

        // Execute the operation with retry logic
        for attempt in 1..=MAX_RETRIES {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                        let backoff_time = Duration::from_secs(2_u64.pow(attempt));
                        warn!(
                            "Rate limited, backing off for {:?} (attempt {})",
                            backoff_time, attempt
                        );
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

        match self
            .rate_limited_call(|| {
                let http_clone = http_ref.clone();
                let nickname_clone = nickname_owned.clone();
                async move {
                    http_clone
                        .edit_nickname(guild_id, Some(&nickname_clone), None)
                        .await
                }
            })
            .await
        {
            Ok(_) => {
                debug!("Updated nickname in guild {}", guild_id);
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                    warn!(
                        "Rate limited while updating nickname in guild {}: {}",
                        guild_id, e
                    );
                } else {
                    warn!("Failed to update nickname in guild {}: {}", guild_id, e);
                }
                Err(BotError::Discord(e.to_string()))
            }
        }
    }

    /// Update nicknames in multiple guilds in parallel
    pub async fn update_nicknames_in_guilds(
        &self,
        guilds: &[GuildId],
        nickname: &str,
    ) -> Vec<BotResult<()>> {
        use futures::stream::StreamExt;
        use std::sync::Arc;

        let nickname = nickname.to_string();
        let self_arc = Arc::new(self.clone());

        let futures: Vec<_> = guilds
            .iter()
            .map(|guild_id| {
                let api = self_arc.clone();
                let nickname = nickname.clone();
                let guild_id = *guild_id;
                async move { api.update_nickname(guild_id, &nickname).await }
            })
            .collect();

        let mut results = Vec::new();
        let mut stream = futures::stream::iter(futures).buffer_unordered(3);

        while let Some(result) = stream.next().await {
            results.push(result);
        }

        results
    }
}
