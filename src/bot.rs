use crate::config::BotConfig;
use crate::database::PriceDatabase;
use crate::discord_api::DiscordApi;
use crate::errors::{BotError, BotResult};
use crate::health::HealthState;
use crate::health_server;
use crate::price_service::PricesFile;
use crate::utils::{
    format_price, get_current_timestamp, validate_crypto_name, validate_price,
};
use serenity::{
    all::{
        ActivityData, Command, CommandDataOptionValue, CommandOptionType, CreateCommand,
        CreateCommandOption, GatewayIntents,
    },
    async_trait,
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    http::Http,
    model::{application::CommandInteraction, gateway::Ready},
    prelude::*,
    Client,
};
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const RECONNECT_DELAY_SECONDS: u64 = 30;

/// Discord bot for tracking cryptocurrency prices
#[derive(Debug)]
pub struct Bot {
    config: BotConfig,
    health: HealthState,
    database: Arc<PriceDatabase>,
}

impl Bot {
    /// Create a new bot instance with configuration
    pub fn new(config: BotConfig) -> Self {
        let health = HealthState::new(config.crypto_name.clone());
        let database = Arc::new(PriceDatabase::new("shared/prices.db"));
        
        Self {
            config,
            health,
            database,
        }
    }

    /// Register slash commands with Discord
    async fn register_commands(&self, http: &Http) -> BotResult<()> {
        info!("Registering slash commands...");
        
        let current_crypto = &self.config.crypto_name;
        let price_command = CreateCommand::new("price")
            .description(format!(
                "Get current price for a cryptocurrency (defaults to {})",
                current_crypto
            ))
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "crypto",
                    format!("Cryptocurrency symbol (defaults to {})", current_crypto),
                )
                .required(false),
            );

        info!("Creating global command...");

        Command::create_global_command(http, price_command)
            .await
            .map_err(|e| BotError::Discord(format!("Failed to register /price command: {}", e)))?;

        info!("Successfully registered /price command globally");
        info!("Note: Global commands can take up to 1 hour to appear in Discord");

        Ok(())
    }

    /// Handle the /price slash command
    async fn handle_price_command(&self, interaction: &CommandInteraction) -> BotResult<String> {
        // Get crypto name from command option, or default to current bot's crypto
        let crypto_name = if let Some(crypto_option) = interaction
            .data
            .options
            .iter()
            .find(|opt| opt.name == "crypto")
        {
            match &crypto_option.value {
                CommandDataOptionValue::String(s) => {
                    let name = s.clone();
                    validate_crypto_name(&name)?;
                    name
                }
                _ => return Err(BotError::InvalidInput("Invalid crypto option".into())),
            }
        } else {
            // No crypto specified, use the current bot's crypto
            self.config.crypto_name.clone()
        };

        debug!("Price command called for: {}", crypto_name);

        // Get current price from shared prices file
        let prices = read_prices_from_file().await?;
        debug!(
            "Available cryptos: {:?}",
            prices.prices.keys().collect::<Vec<_>>()
        );

        let price_data = prices
            .prices
            .get(&crypto_name)
            .ok_or_else(|| BotError::PriceNotFound(crypto_name.clone()))?;

        validate_price(price_data.price)?;

        let formatted_price = format_price(price_data.price);

        info!("{} price: ${}", crypto_name, price_data.price);

        // Calculate price changes over different time periods using database
        let change_info = self
            .database
            .get_price_changes(&crypto_name, price_data.price)
            .unwrap_or_else(|_| " 🔄 Building history".to_string());

        // Build the main response
        let mut response = format!(
            "{}: {} {}",
            crypto_name, formatted_price, change_info
        );

        // Add prices in terms of BTC, ETH, and SOL (excluding the crypto's own price)
        let mut conversion_prices = Vec::new();

        if crypto_name != "BTC" {
            if let Some(btc_price) = prices.prices.get("BTC") {
                let btc_conversion = price_data.price / btc_price.price;
                conversion_prices.push(format!("{:.8} BTC", btc_conversion));
                debug!("BTC conversion: {:.8} BTC", btc_conversion);
            } else {
                warn!("BTC price not found in shared data");
            }
        }

        if crypto_name != "ETH" {
            if let Some(eth_price) = prices.prices.get("ETH") {
                let eth_conversion = price_data.price / eth_price.price;
                conversion_prices.push(format!("{:.6} ETH", eth_conversion));
                debug!("ETH conversion: {:.6} ETH", eth_conversion);
            } else {
                warn!("ETH price not found in shared data");
            }
        }

        if crypto_name != "SOL" {
            if let Some(sol_price) = prices.prices.get("SOL") {
                let sol_conversion = price_data.price / sol_price.price;
                conversion_prices.push(format!("{:.4} SOL", sol_conversion));
                debug!("SOL conversion: {:.4} SOL", sol_conversion);
            } else {
                warn!("SOL price not found in shared data");
            }
        }

        // Add conversion prices to response if available
        if !conversion_prices.is_empty() {
            response.push_str(&format!(
                "\n💱 Also: {}",
                conversion_prices.join(" | ")
            ));
            debug!("Final response with conversions: {}", response);
        } else {
            warn!("No conversion prices available");
        }

        Ok(response)
    }

    /// Start the price update loop
    pub async fn start_price_loop(&self, http: Arc<Http>, ctx: Arc<Context>) {
        let config = self.config.clone();
        let health = Arc::new(self.health.clone());
        let database = self.database.clone();

        // Start health check server
        let health_clone = health.clone();
        tokio::spawn(async move {
            health_server::start_health_server(health_clone, 8080).await;
        });

        tokio::spawn(async move {
            price_update_loop(http, ctx, config, health, database).await;
        });
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Bot is ready! Logged in as: {}", ready.user.name);
        info!("Bot ID: {}", ready.user.id);
        info!("Connected to {} guilds", ready.guilds.len());

        info!("Starting command registration...");

        // Register slash commands with retry logic
        for attempt in 1..=3 {
            match self.register_commands(&ctx.http).await {
                Ok(_) => {
                    info!("Command registration completed successfully");
                    break;
                }
                Err(e) => {
                    error!("Command registration failed (attempt {}): {}", attempt, e);
                    if attempt < 3 {
                        sleep(Duration::from_secs(5)).await;
                    } else {
                        error!("Failed to register commands after 3 attempts");
                        return;
                    }
                }
            }
        }

        info!("Starting price update loop...");

        let http = ctx.http.clone();
        let ctx_arc = Arc::new(ctx);

        self.start_price_loop(http, ctx_arc).await;

        info!("Bot initialization complete!");
    }

    async fn interaction_create(
        &self,
        ctx: Context,
        interaction: serenity::model::application::Interaction,
    ) {
        debug!("Interaction received: {:?}", interaction.kind());

        if let serenity::model::application::Interaction::Command(command_interaction) = interaction
        {
            debug!("Command interaction: {}", command_interaction.data.name);

            let response = match command_interaction.data.name.as_str() {
                "price" => {
                    debug!("Handling /price command");
                    match self.handle_price_command(&command_interaction).await {
                        Ok(message) => {
                            debug!("Price command successful, responding with: {}", message);
                            let data = CreateInteractionResponseMessage::new().content(message);
                            let builder = CreateInteractionResponse::Message(data);
                            command_interaction
                                .create_response(&ctx.http, builder)
                                .await
                        }
                        Err(e) => {
                            error!("Price command failed: {}", e);
                            let data = CreateInteractionResponseMessage::new()
                                .content(format!("❌ Error: {}", e));
                            let builder = CreateInteractionResponse::Message(data);
                            command_interaction
                                .create_response(&ctx.http, builder)
                                .await
                        }
                    }
                }
                _ => {
                    warn!("Unknown command: {}", command_interaction.data.name);
                    let data =
                        CreateInteractionResponseMessage::new().content("❌ Unknown command");
                    let builder = CreateInteractionResponse::Message(data);
                    command_interaction
                        .create_response(&ctx.http, builder)
                        .await
                }
            };

            if let Err(e) = response {
                error!("Failed to respond to interaction: {}", e);
            }
        }
    }
}

/// Read prices from the shared JSON file with retry logic
async fn read_prices_from_file() -> BotResult<PricesFile> {
    let file_path = "shared/prices.json";
    const MAX_RETRIES: u32 = 3;

    for attempt in 1..=MAX_RETRIES {
        // Check if file exists
        if !std::path::Path::new(file_path).exists() {
            if attempt < MAX_RETRIES {
                warn!(
                    "Prices file not found (attempt {}), retrying...",
                    attempt
                );
                sleep(Duration::from_millis(1000 * attempt as u64)).await;
                continue;
            }
            return Err(BotError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Prices file not found. Make sure price-service is running.",
            )));
        }

        match fs::read_to_string(file_path) {
            Ok(content) => match serde_json::from_str::<PricesFile>(&content) {
                Ok(prices) => return Ok(prices),
                Err(e) => {
                    error!("Failed to parse prices file (attempt {}): {}", attempt, e);
                    if attempt < MAX_RETRIES {
                        sleep(Duration::from_millis(1000 * attempt as u64)).await;
                        continue;
                    }
                    return Err(BotError::Json(e));
                }
            },
            Err(e) => {
                error!("Failed to read prices file (attempt {}): {}", attempt, e);
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(BotError::Io(e));
            }
        }
    }

    unreachable!()
}

/// Main price update loop with comprehensive error handling
async fn price_update_loop(
    http: Arc<Http>,
    ctx: Arc<Context>,
    config: BotConfig,
    health: Arc<HealthState>,
    database: Arc<PriceDatabase>,
) {
    let crypto_name = &config.crypto_name;
    let mut consecutive_failures = 0;
    let discord_api = DiscordApi::new(http);

    info!("Starting price update loop for {}", crypto_name);

    loop {
        let loop_start = std::time::Instant::now();

        // Wrap the entire update logic in error handling
        let update_result = async {
            // Get current price with error handling
            let current_price = match get_crypto_price(&config).await {
                Ok(price) => {
                    consecutive_failures = 0; // Reset failure counter on success
                    health.reset_failures();
                    health.update_price_timestamp();
                    price
                }
                Err(e) => {
                    consecutive_failures += 1;
                    health.increment_failures();
                    error!(
                        "Failed to get {} price (failure {}/{}): {}",
                        crypto_name, consecutive_failures, MAX_CONSECUTIVE_FAILURES, e
                    );

                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        error!(
                            "Too many consecutive failures for {}. Entering recovery mode.",
                            crypto_name
                        );
                        sleep(Duration::from_secs(RECONNECT_DELAY_SECONDS)).await;
                        consecutive_failures = 0; // Reset after recovery delay
                        health.reset_failures();
                    }
                    return Err(e);
                }
            };

            // Get price change indicator with error handling
            let (arrow, change_percent) = database.get_price_indicator(crypto_name, current_price);

            // Format the nickname
            let nickname = format!("{} {}", crypto_name, format_price(current_price));

            // Format the custom status with rotation
            let update_count = match get_current_timestamp() {
                Ok(time) => (time / 12) % 4,
                Err(_) => 0,
            };

            let custom_status = match read_prices_from_file().await {
                Ok(shared_prices) => {
                    format_custom_status(
                        crypto_name,
                        current_price,
                        &shared_prices,
                        update_count,
                        &arrow,
                        change_percent,
                    )
                }
                Err(e) => {
                    warn!("Failed to read shared prices for status: {}", e);
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
            };

            debug!("Updating nickname to: {}", nickname);
            debug!("Updating custom status to: {}", custom_status);

            // Update custom status (activity)
            ctx.set_activity(Some(ActivityData::playing(custom_status.clone())));
            debug!("Updated activity status");

            // Save current price to database with error handling
            if let Err(e) = database.save_price(crypto_name, current_price) {
                error!("Failed to save price to database: {}", e);
            } else {
                health.update_db_timestamp();
            }

            // Update nickname in guilds with rate limiting and error handling
            let guilds = ctx.cache.guilds();
            let guild_count = guilds.len();

            if guild_count > 0 {
                info!("Updating nickname in {} guilds", guild_count);

                let results = discord_api
                    .update_nicknames_in_guilds(&guilds, &nickname)
                    .await;

                // Count successful updates
                let successful_updates = results.iter().filter(|r| r.is_ok()).count();
                if successful_updates > 0 {
                    health.update_discord_timestamp();
                }

                debug!(
                    "Updated nicknames: {}/{} successful",
                    successful_updates, guild_count
                );
            } else {
                debug!("No guilds found in cache");
            }

            Ok(())
        }
        .await;

        // Handle update result
        match update_result {
            Ok(_) => {
                debug!("Price update completed successfully for {}", crypto_name);
            }
            Err(e) => {
                error!("Price update failed for {}: {}", crypto_name, e);
            }
        }

        // Periodic cleanup of old prices
        database.maybe_cleanup();

        // Calculate how long the update took and adjust sleep time
        let loop_duration = loop_start.elapsed();
        let target_interval = config.update_interval;

        if loop_duration < target_interval {
            let sleep_time = target_interval - loop_duration;
            debug!("Update took {:?}, sleeping for {:?}", loop_duration, sleep_time);
            sleep(sleep_time).await;
        } else {
            warn!(
                "Update took longer than interval: {:?} > {:?}",
                loop_duration, target_interval
            );
            // Still sleep for a minimum time to prevent tight loops
            sleep(Duration::from_secs(1)).await;
        }
    }
}

/// Format custom status based on crypto type and rotation
fn format_custom_status(
    crypto_name: &str,
    current_price: f64,
    shared_prices: &PricesFile,
    update_count: u64,
    arrow: &str,
    change_percent: f64,
) -> String {
    // Calculate ticker price in terms of BTC, ETH, SOL
    let btc_amount = current_price
        / shared_prices
            .prices
            .get("BTC")
            .map(|p| p.price)
            .unwrap_or(45000.0);
    let eth_amount = current_price
        / shared_prices
            .prices
            .get("ETH")
            .map(|p| p.price)
            .unwrap_or(2800.0);
    let sol_amount = current_price
        / shared_prices
            .prices
            .get("SOL")
            .map(|p| p.price)
            .unwrap_or(95.0);

    match crypto_name {
        "BTC" => {
            // For BTC bot, show ETH and SOL amounts, skip BTC/BTC
            match update_count {
                0 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                1 => format!("{:.8} Ξ", eth_amount),
                2 => format!("{:.8} ◎", sol_amount),
                3 => format!("{:.8} Ξ", eth_amount),
                _ => unreachable!(),
            }
        }
        "ETH" => {
            // For ETH bot, show BTC and SOL amounts, skip ETH/ETH
            match update_count {
                0 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                1 => format!("{:.8} ₿", btc_amount),
                2 => format!("{:.8} ◎", sol_amount),
                3 => format!("{:.8} ₿", btc_amount),
                _ => unreachable!(),
            }
        }
        "SOL" => {
            // For SOL bot, show BTC and ETH amounts, skip SOL/SOL
            match update_count {
                0 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                1 => format!("{:.8} ₿", btc_amount),
                2 => format!("{:.8} Ξ", eth_amount),
                3 => format!("{:.8} ₿", btc_amount),
                _ => unreachable!(),
            }
        }
        _ => {
            // For other tickers, show all three conversions
            match update_count {
                0 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                1 => format!("{:.8} ₿", btc_amount),
                2 => format!("{:.8} Ξ", eth_amount),
                3 => format!("{:.8} ◎", sol_amount),
                _ => unreachable!(),
            }
        }
    }
}

/// Get current cryptocurrency price
async fn get_crypto_price(config: &BotConfig) -> BotResult<f64> {
    // First try to get from shared prices file
    match read_prices_from_file().await {
        Ok(prices) => {
            if let Some(price_data) = prices.prices.get(&config.crypto_name) {
                validate_price(price_data.price)?;
                return Ok(price_data.price);
            }
        }
        Err(_) => {
            // If shared file doesn't exist or doesn't have our crypto, try direct API call
        }
    }

    // Fallback to direct API call if we have a feed ID
    if let Some(feed_id) = &config.pyth_feed_id {
        return get_individual_crypto_price(feed_id).await;
    }

    Err(BotError::PriceNotFound(config.crypto_name.clone()))
}

/// Fetch individual cryptocurrency price from Pyth Network with retry logic
async fn get_individual_crypto_price(feed_id: &str) -> BotResult<f64> {
    let url = format!(
        "https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}",
        feed_id
    );
    const MAX_RETRIES: u32 = 3;

    for attempt in 1..=MAX_RETRIES {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| BotError::Http(e.to_string()))?;

        match client
            .get(&url)
            .header("User-Agent", "Crypto-Price-Bot/1.0")
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() {
                    error!(
                        "HTTP request failed (attempt {}): {}",
                        attempt,
                        response.status()
                    );
                    if attempt < MAX_RETRIES {
                        sleep(Duration::from_millis(1000 * attempt as u64)).await;
                        continue;
                    }
                    return Err(BotError::Http(format!(
                        "HTTP request failed: {}",
                        response.status()
                    )));
                }

                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        // Parse the price from the parsed array
                        let parsed_data = json
                            .get("parsed")
                            .and_then(|p| p.as_array())
                            .ok_or_else(|| BotError::Parse("No parsed data found".into()))?;

                        let first_feed = parsed_data
                            .first()
                            .ok_or_else(|| BotError::Parse("No feed data found".into()))?;

                        let price_data = first_feed
                            .get("price")
                            .ok_or_else(|| BotError::Parse("No price data found".into()))?;

                        let price_str = price_data
                            .get("price")
                            .and_then(|p| p.as_str())
                            .ok_or_else(|| BotError::Parse("No price string found".into()))?;

                        let price = price_str
                            .parse::<i64>()
                            .map_err(|_| BotError::Parse("Invalid price format".into()))?;

                        let expo = price_data.get("expo").and_then(|e| e.as_i64()).unwrap_or(0);
                        let real_price = price as f64 * 10f64.powi(expo as i32);

                        validate_price(real_price)?;
                        return Ok(real_price);
                    }
                    Err(e) => {
                        error!("JSON parsing failed (attempt {}): {}", attempt, e);
                        if attempt < MAX_RETRIES {
                            sleep(Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                        return Err(BotError::Http(e.to_string()));
                    }
                }
            }
            Err(e) => {
                error!("Network request failed (attempt {}): {}", attempt, e);
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(BotError::Http(e.to_string()));
            }
        }
    }

    unreachable!()
}

/// Start the bot with proper error handling and reconnection capability
pub async fn start_bot_with_reconnection(config: &BotConfig) -> BotResult<()> {
    let intents = GatewayIntents::GUILDS;

    let bot = Bot::new(config.clone());

    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(bot)
        .await
        .map_err(|e| BotError::Discord(format!("Failed to create client: {}", e)))?;

    info!("Starting Discord client...");

    // Start the client with error handling
    match client.start().await {
        Ok(_) => {
            info!("Discord client started successfully");
            Ok(())
        }
        Err(e) => {
            error!("Discord client error: {}", e);
            Err(BotError::Discord(format!("Client error: {}", e)))
        }
    }
}