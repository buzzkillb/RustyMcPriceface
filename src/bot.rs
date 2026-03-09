use crate::config::BotConfig;
use crate::database::PriceDatabase;
use crate::discord_api::DiscordApi;
use crate::errors::{BotError, BotResult};
use crate::health::{HealthState, HealthAggregator};

use crate::price_service::PricesFile;
use crate::utils::{
    format_price, get_current_timestamp, validate_crypto_name, validate_price,
};
use crate::charting::{generate_shanghai_chart, generate_price_chart};
use crate::price_service::fetch_shanghai_history;
use serenity::{
    all::{
        ActivityData, Command, CommandDataOptionValue, CommandOptionType, CreateCommand,
        CreateCommandOption, GatewayIntents, CreateAttachment,
    },
    async_trait,
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    http::Http,
    model::{application::CommandInteraction, gateway::Ready, channel::Message},
    prelude::*,
    Client,
};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const RECONNECT_DELAY_SECONDS: u64 = 30;

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    if days > 0 {
        format!("{}d{}h", days, hours)
    } else if hours > 0 {
        format!("{}h{}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

/// Discord bot for tracking cryptocurrency prices
#[derive(Debug, Clone)]
pub struct Bot {
    config: BotConfig,
    health: Arc<HealthState>,
    health_aggregator: Arc<HealthAggregator>,
    database: Arc<PriceDatabase>,
}

impl Bot {
    /// Create a new bot instance with configuration, shared database, and health state
    pub fn new(config: BotConfig, database: Arc<PriceDatabase>, health: Arc<HealthState>, health_aggregator: Arc<HealthAggregator>) -> BotResult<Self> {
        Ok(Self {
            config,
            health,
            health_aggregator,
            database,
        })
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

        let chart_command = CreateCommand::new("silverchart")
            .description("Get a 1-year historical chart for the current crypto");

        let status_command = CreateCommand::new("status")
            .description("Get bot system status (BTC bot only)");

        info!("Creating global command...");

        Command::create_global_command(http, price_command)
            .await
            .map_err(|e| BotError::Discord(format!("Failed to register /price command: {}", e)))?;

        Command::create_global_command(http, chart_command)
            .await
            .map_err(|e| BotError::Discord(format!("Failed to register /silverchart command: {}", e)))?;

        Command::create_global_command(http, status_command)
            .await
            .map_err(|e| BotError::Discord(format!("Failed to register /status command: {}", e)))?;

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

        // Get current price from database
        let current_price = self.database.get_latest_price(&crypto_name)?;
        validate_price(current_price)?;

        // Get all prices from database for conversions
        let all_prices = self.database.get_all_latest_prices()?;

        // Build response using helper
        let response = self.build_price_response(&crypto_name, current_price, &all_prices)?;

        Ok(response)
    }

    async fn handle_chart_command(&self, interaction: &CommandInteraction, ctx: &Context) -> BotResult<()> {
        // Defer response as charting might take a moment
        interaction.defer(&ctx.http).await
            .map_err(|e| BotError::Discord(format!("Failed to defer interaction: {}", e)))?;

        // silverchart command always uses SILVER
        let crypto_name = "SILVER";
        
        // SILVER uses database history, GOLD uses Shanghai API
        if crypto_name == "SILVER" || crypto_name == "XAG" {
            let history = self.database.get_price_history(crypto_name, 30)
                .map_err(|e| BotError::Discord(format!("Failed to fetch history: {}", e)))?;

            if history.is_empty() {
                interaction.edit_response(&ctx.http, serenity::builder::EditInteractionResponse::new().content("❌ No historical data available yet (waiting for data to be collected)")).await
                    .map_err(|e| BotError::Discord(format!("Failed to send empty response: {}", e)))?;
                return Ok(());
            }

            let image_data = generate_price_chart(&history, crypto_name).map_err(|e| BotError::Discord(format!("Failed to generate chart: {}", e)))?;
            let attachment = CreateAttachment::bytes(image_data, "chart.png");
            interaction.edit_response(&ctx.http, serenity::builder::EditInteractionResponse::new()
                .content(format!("📊 30-Day Chart for {}", crypto_name))
                .new_attachment(attachment)
            ).await
            .map_err(|e| BotError::Discord(format!("Failed to send chart response: {}", e)))?;
            return Ok(());
        }

        // GOLD still uses Shanghai API
        let api_symbol = match crypto_name {
            "GOLD" | "XAU" => Some("XAU"),
            _ => None,
        };

        let history = fetch_shanghai_history("1Y", api_symbol).await
            .map_err(|e| BotError::Discord(format!("Failed to fetch history: {}", e)))?;

        if history.is_empty() {
             interaction.edit_response(&ctx.http, serenity::builder::EditInteractionResponse::new().content("❌ No historical data available")).await
                .map_err(|e| BotError::Discord(format!("Failed to send empty response: {}", e)))?;
             return Ok(());
        }

        // Generate Chart
        let image_data = generate_shanghai_chart(&history, crypto_name).map_err(|e| BotError::Discord(format!("Failed to generate chart: {}", e)))?;

        // Send Response
        let attachment = CreateAttachment::bytes(image_data, "chart.png");
        interaction.edit_response(&ctx.http, serenity::builder::EditInteractionResponse::new()
            .content(format!("📊 1-Year Chart for {}", crypto_name))
            .new_attachment(attachment)
        ).await
        .map_err(|e| BotError::Discord(format!("Failed to send chart response: {}", e)))?;

        Ok(())
    }

    async fn send_chart_to_channel(
        &self,
        ctx: &Context,
        channel_id: &serenity::model::id::ChannelId,
        crypto_name: &str,
        title: &str,
    ) -> BotResult<()> {
        match self.database.get_price_history(crypto_name, 30) {
            Ok(history) => {
                if history.is_empty() {
                    let _ = channel_id.say(&ctx.http, "❌ No historical data available yet (waiting for data to be collected)").await;
                    return Ok(());
                }
                match generate_price_chart(&history, crypto_name) {
                    Ok(image_data) => {
                        let attachment = CreateAttachment::bytes(image_data, "chart.png");
                        let _ = channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                            .content(title)
                            .add_file(attachment)
                        ).await;
                        self.health.update_discord_timestamp();
                    },
                    Err(e) => {
                        error!("Failed to generate chart: {}", e);
                        let _ = channel_id.say(&ctx.http, format!("❌ Failed to generate chart: {}", e)).await;
                    }
                }
            },
            Err(e) => {
                error!("Failed to fetch history: {}", e);
                let _ = channel_id.say(&ctx.http, format!("❌ Failed to fetch history: {}", e)).await;
            }
        }
        Ok(())
    }

    /// Build a price response string with conversions and additional info
    /// This is the core logic shared between slash commands and message commands
    fn build_price_response(
        &self,
        crypto_name: &str,
        current_price: f64,
        all_prices: &HashMap<String, f64>,
    ) -> BotResult<String> {
        let formatted_price = format_price(current_price);

        info!("{} price: ${}", crypto_name, current_price);

        // Calculate price changes over different time periods using database
        let change_info = self
            .database
            .get_price_changes(crypto_name, current_price)
            .unwrap_or_else(|e| {
                error!("Failed to get price changes for {}: {}", crypto_name, e);
                " 🔄 Building history".to_string()
            });

        // Build the main response
        let mut response = format!(
            "{}: {} {}",
            crypto_name, formatted_price, change_info
        );

        // Add prices in terms of BTC, ETH, and SOL (excluding the crypto's own price)
        let mut conversion_prices = Vec::new();

        if crypto_name != "BTC" {
            if let Some(btc_price) = all_prices.get("BTC") {
                let btc_conversion = current_price / btc_price;
                conversion_prices.push(format!("{:.8} BTC", btc_conversion));
            }
        }

        if crypto_name != "ETH" {
            if let Some(eth_price) = all_prices.get("ETH") {
                let eth_conversion = current_price / eth_price;
                conversion_prices.push(format!("{:.6} ETH", eth_conversion));
            }
        }

        if crypto_name != "SOL" {
            if let Some(sol_price) = all_prices.get("SOL") {
                let sol_conversion = current_price / sol_price;
                conversion_prices.push(format!("{:.4} SOL", sol_conversion));
            }
        }

        // Add Gold/Silver ratio if this is Silver
        if crypto_name == "SILVER" || crypto_name == "XAG" {
            let gold_price = all_prices.get("GOLD")
                .or_else(|| all_prices.get("XAU"))
                .or_else(|| all_prices.get("PAXG"));

            if let Some(gold) = gold_price {
                let ratio = gold / current_price;
                conversion_prices.push(format!("Ratio: {:.2} (Au/Ag)", ratio));
            }
        }

        // Add Shanghai Premium info
        if crypto_name == "SHANGHAI" || crypto_name == "SHANGHAISILVER" {
            // For SHANGHAISILVER, calculate premium from SILVER price
            let (premium, premium_percent) = if crypto_name == "SHANGHAISILVER" {
                if let Some(silver_price) = all_prices.get("SILVER") {
                    let prem = current_price - silver_price;
                    let prem_pct = (prem / silver_price) * 100.0;
                    (prem, prem_pct)
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0) // For SHANGHAI, we'd need premium data from elsewhere
            };
            
            response.push_str(&format!(
                 "\n🇨🇳 Shanghai Premium: ${:.2} (+{:.2}%)",
                 premium, premium_percent
            ));
        }

        // Add conversion prices to response if available
        if !conversion_prices.is_empty() {
            response.push_str(&format!(
                "\n💱 Also: {}",
                conversion_prices.join(" | ")
            ));
        }

        Ok(response)
    }

    /// Handle price command for message-based commands (like !btc, !sol)
    async fn handle_price_command_for_message(&self, channel_id: &serenity::model::id::ChannelId, ctx: &Context) -> BotResult<()> {
        let crypto_name = self.config.crypto_name.clone();

        debug!("Message price command called for: {}", crypto_name);

        // Get current price from database
        let current_price = self.database.get_latest_price(&crypto_name)?;
        validate_price(current_price)?;

        // Get all prices from database for conversions
        let all_prices = self.database.get_all_latest_prices()?;

        // Build response using helper
        let response = self.build_price_response(&crypto_name, current_price, &all_prices)?;

        // Send the response to the channel
        channel_id.say(&ctx.http, response).await
            .map_err(|e| BotError::Discord(format!("Failed to send message: {}", e)))?;

        Ok(())
    }
}

/// Helper to start a bot instance (used by main.rs)
pub async fn start_bot(config: BotConfig, database: Arc<PriceDatabase>, health: Arc<HealthState>, health_aggregator: Arc<HealthAggregator>) -> BotResult<()> {
    let token = config.discord_token.clone();
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let bot = Bot::new(config, database, health, health_aggregator)?;

    let mut client = Client::builder(&token, intents)
        .event_handler(bot)
        .await
        .map_err(|e| BotError::Discord(format!("Error creating client: {}", e)))?;

    if let Err(why) = client.start().await {
        return Err(BotError::Discord(format!("Client error: {}", why)));
    }

    Ok(())
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Bot is ready! Logged in as: {}", ready.user.name);
        info!("Bot ID: {}", ready.user.id);
        info!("Connected to {} guilds", ready.guilds.len());
        
        // Update Discord timestamp to indicate successful connection
        self.health.update_discord_timestamp();

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

        // Run the price update loop in a separate task so ready() returns
        // Run the price update loop in a separate task so ready() returns
        // Cloning Bot is expensive if it has deep state, but here it's Config + Health (Arc-like internals) + Arc<DB>
        // Use a wrapper or simply spawn the loop with cloned components
        
        let config = self.config.clone();
        let health = self.health.clone();
        let database = self.database.clone();
        
        tokio::spawn(async move {
            price_update_loop(http, ctx_arc, config, health, database).await;
        });

        info!("Bot initialization complete!");
    }

    async fn resume(&self, _ctx: Context, _resumed: serenity::model::event::ResumedEvent) {
        info!("Bot resumed connection to Discord gateway");
        self.health.update_discord_timestamp();
        self.health.reset_gateway_failures();
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
                "silverchart" => {
                    debug!("Handling /silverchart command");
                    if let Err(e) = self.handle_chart_command(&command_interaction, &ctx).await {
                         error!("Chart command failed: {}", e);
                         let _ = command_interaction.create_response(&ctx.http, 
                            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)))
                         ).await;
                    }
                    Ok(()) // Response handled inside function
                }
                "status" => {
                    debug!("Handling /status command");
                    // Only BTC bot responds to status
                    if self.config.crypto_name == "BTC" {
                        let status = self.health_aggregator.to_json();
                        let total_bots = status.get("total_bots").and_then(|v| v.as_u64()).unwrap_or(0);
                        let healthy_bots = status.get("healthy_bots").and_then(|v| v.as_u64()).unwrap_or(0);
                        
                        let mut lines = vec![
                            "System Status".to_string(),
                            format!("Total Bots: {} | Healthy: {}", total_bots, healthy_bots),
                        ];
                        
                        if let Some(bots) = status.get("bots").and_then(|v| v.as_array()) {
                            for bot in bots {
                                let name = bot.get("bot_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let healthy = bot.get("healthy").and_then(|v| v.as_bool()).unwrap_or(false);
                                let failures = bot.get("consecutive_failures").and_then(|v| v.as_u64()).unwrap_or(0);
                                let gateway_failures = bot.get("gateway_failures").and_then(|v| v.as_u64()).unwrap_or(0);
                                let uptime_secs = bot.get("uptime_seconds").and_then(|v| v.as_u64()).unwrap_or(0);
                                let uptime = format_uptime(uptime_secs);
                                
                                lines.push(format!(
                                    "{}: {} | Uptime: {} | Fails: {} | GW: {}", 
                                    name, 
                                    if healthy { "OK" } else { "DOWN" },
                                    uptime,
                                    failures,
                                    gateway_failures
                                ));
                            }
                        }
                        
                        let message = lines.join("\n");
                        let _ = command_interaction.create_response(&ctx.http,
                            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(message))
                        ).await;
                    }
                    Ok(())
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

            match response {
                Ok(_) => {
                    // Update Discord timestamp on successful interaction response
                    self.health.update_discord_timestamp();
                }
                Err(e) => {
                    error!("Failed to respond to interaction: {}", e);
                }
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        info!("🔔 MESSAGE RECEIVED: '{}' from {} in channel {}", msg.content, msg.author.name, msg.channel_id);
        debug!("Received message: '{}' from {}", msg.content, msg.author.name);
        
        // Ignore messages from bots
        if msg.author.bot {
            debug!("Ignoring message from bot: {}", msg.author.name);
            return;
        }

        // Check if message starts with ! followed by this bot's crypto name OR if bot is mentioned
        let command = format!("!{}", self.config.crypto_name.to_lowercase());
        let content_lower = msg.content.to_lowercase();
        
        // Handle !shanghai as alias for SHANGHAISILVER
        let is_shanghai_alias = self.config.crypto_name == "SHANGHAISILVER" && content_lower == "!shanghai";
        
        let is_command = content_lower == command || content_lower.starts_with(&format!("{} ", command)) || is_shanghai_alias;
        
        // SILVER and SHANGHAISILVER bots respond to their respective chart commands
        let is_chart = self.config.crypto_name == "SILVER" && content_lower == "!silverchart";
        
        let is_shanghai_chart = self.config.crypto_name == "SHANGHAISILVER" && content_lower == "!shanghaichart";
        
        // Generic chart command: !<ticker>chart (e.g., !solchart)
        let generic_chart_cmd = format!("!{}chart", self.config.crypto_name.to_lowercase());
        let is_generic_chart = msg.content.to_lowercase() == generic_chart_cmd;
        
        let is_mentioned = msg.mentions_me(&ctx).await.unwrap_or(false);
        
        debug!("Looking for command: '{}' in message: '{}', is_command: {}, is_mentioned: {}", 
               command, msg.content, is_command, is_mentioned);
        
        if is_command || is_mentioned {
            debug!("Received {} command from {}", command, msg.author.name);

            // Get the same price data as the slash command
            match self.handle_price_command_for_message(&msg.channel_id, &ctx).await {
                Ok(_) => {
                    debug!("Successfully responded to {} command", command);
                    self.health.update_discord_timestamp();
                }
                Err(e) => {
                    error!("Failed to handle {} command: {}", command, e);
                    // Try to send an error message
                    if let Err(send_err) = msg.channel_id.say(&ctx.http, format!("❌ Error: {}", e)).await {
                        error!("Failed to send error message: {}", send_err);
                    }
                }
            }
        } else if is_chart {
             debug!("Received chart command from {}", msg.author.name);
             
             // silverchart always uses SILVER (database history)
             let _ = self.send_chart_to_channel(&ctx, &msg.channel_id, "SILVER", "📊 30-Day Chart for SILVER").await;

        } else if is_shanghai_chart {
             debug!("Received !shanghaichart command from {}", msg.author.name);
             
             // SHANGHAISILVER uses database history
             let _ = self.send_chart_to_channel(&ctx, &msg.channel_id, "SHANGHAISILVER", "📊 30-Day Chart for Shanghai Silver").await;

        } else if is_generic_chart {
             debug!("Received generic chart command from {}", msg.author.name);
             let crypto_name = &self.config.crypto_name;
             let title = format!("📊 30-Day History for {}", crypto_name);
             let _ = self.send_chart_to_channel(&ctx, &msg.channel_id, crypto_name, &title).await;

        } else {
            debug!("Message '{}' does not match command '{}'", msg.content, command);
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
            let current_price = match get_crypto_price(&config, &database).await {
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
            let nickname = if crypto_name == "SHANGHAI" || crypto_name == "SHANGHAISILVER" {
                format!("SILVER {}", format_price(current_price))
            } else {
                format!("{} {}", crypto_name, format_price(current_price))
            };

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

            // Update custom status (activity) - this doesn't return a Result but we can still track attempts
            ctx.set_activity(Some(ActivityData::playing(custom_status.clone())));
            debug!("Updated activity status");
            
            // Note: set_activity doesn't return errors, so we can't directly detect failures here
            // The periodic Discord test will catch connectivity issues

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

                // Count successful updates and track failures more aggressively
                let successful_updates = results.iter().filter(|r| r.is_ok()).count();
                let failed_updates = results.iter().filter(|r| r.is_err()).count();
                
                if successful_updates > 0 {
                    health.update_discord_timestamp();
                    // Only reset gateway failures if most updates succeeded
                    if successful_updates > failed_updates {
                        health.reset_gateway_failures();
                    }
                } else {
                    // All updates failed - increment gateway failures
                    health.increment_gateway_failures();
                    warn!("All {} Discord nickname updates failed", guild_count);
                    
                    // If no Discord updates succeeded, check if we should exit for restart
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let last_discord = health.last_discord_update.load(std::sync::atomic::Ordering::Relaxed);
                    
                    // If Discord communication has been failing for more than 2 minutes, exit for restart
                    if now.saturating_sub(last_discord) > 120 {
                        error!("Discord communication has been failing for over 2 minutes. Exiting for restart.");
                        return Err(BotError::Discord("Gateway connection lost - restarting".into()));
                    }
                }
                
                // Track partial failures
                if failed_updates > 0 {
                    warn!("Some Discord updates failed: {}/{} failed", failed_updates, guild_count);
                }

                debug!(
                    "Updated nicknames: {}/{} successful",
                    successful_updates, guild_count
                );
            } else {
                warn!("No guilds found in cache - Discord connection may be lost!");
                health.increment_gateway_failures();
                
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let last_discord = health.last_discord_update.load(std::sync::atomic::Ordering::Relaxed);
                
                if now.saturating_sub(last_discord) > 120 {
                    error!("No guilds for over 2 minutes. Exiting for restart.");
                    return Err(BotError::Discord("Gateway connection lost - no guilds detected".into()));
                }
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

        // Periodic Discord connectivity test (every 10 update cycles)
        let update_count = match get_current_timestamp() {
            Ok(time) => time / config.update_interval.as_secs(),
            Err(_) => 0,
        };
        
        if update_count % 10 == 0 {
            debug!("Running periodic Discord connectivity test for {}", crypto_name);
            tokio::spawn({
                let health_clone = health.clone();
                async move {
                    test_discord_connectivity(health_clone).await;
                }
            });
        }

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
        "SILVER" | "XAG" => {
            // For Silver bot, show Gold/Silver ratio
            let gold_price = shared_prices.prices.get("GOLD")
                .or_else(|| shared_prices.prices.get("XAU"))
                .or_else(|| shared_prices.prices.get("PAXG"))
                .map(|p| p.price);
                
            let ratio_str = if let Some(gold) = gold_price {
                 format!("Au/Ag: {:.2}", gold / current_price)
            } else {
                 format!("{:.8} ₿", btc_amount) // Fallback
            };

            match update_count {
                0 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                1 => ratio_str,
                2 => format!("{:.8} ₿", btc_amount),
                3 => format!("{:.8} Ξ", eth_amount),
                _ => unreachable!(),
            }
        }
        "SHANGHAI" => {
            // For Shanghai bot, scroll through Premium and Premium Percent
            match update_count {
                0 | 3 => { // Show arrow/building history on 0 and 3 (half the time, or custom cycle)
                     // User asked for "always update price... and then cycle 2 and 3 would be underneath"
                     // Actually user said: "watching area scroll through the price delta 'premium'... and percentage delta"
                     // The default status (update_count 0) usually shows price change. 
                     // Let's make it: 
                     // 0: Price Change (standard)
                     // 1: Premium $
                     // 2: Premium %
                     // 3: Source or back to standard
                    
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                
                1 => {
                    let premium = shared_prices.prices.get("SHANGHAI")
                        .and_then(|p| p.premium)
                        .unwrap_or(0.0);
                    format!("Prem: ${:.2}", premium)
                }
                
                2 => {
                    let premium_pct = shared_prices.prices.get("SHANGHAI")
                        .and_then(|p| p.premium_percent)
                        .unwrap_or(0.0);
                    format!("Prem: {:.2}%", premium_pct)
                }
                _ => unreachable!(),
            }
        }
        "SHANGHAISILVER" => {
            match update_count {
                0 | 3 => {
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
                
                1 => {
                    // Calculate premium: SHANGHAISILVER - SILVER
                    let silver_price = shared_prices.prices.get("SILVER")
                        .map(|p| p.price)
                        .unwrap_or(0.0);
                    let premium = if silver_price > 0.0 {
                        current_price - silver_price
                    } else {
                        0.0
                    };
                    format!("Prem: ${:.2}", premium)
                }
                
                2 => {
                    // Calculate premium percent
                    let silver_price = shared_prices.prices.get("SILVER")
                        .map(|p| p.price)
                        .unwrap_or(0.0);
                    let premium_pct = if silver_price > 0.0 {
                        ((current_price - silver_price) / silver_price) * 100.0
                    } else {
                        0.0
                    };
                    format!("Prem: {:.2}%", premium_pct)
                }
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
async fn get_crypto_price(config: &BotConfig, database: &Arc<PriceDatabase>) -> BotResult<f64> {
    // For SHANGHAISILVER, read directly from database (not in prices.json)
    if config.crypto_name == "SHANGHAISILVER" {
        debug!("Getting SHANGHAISILVER price from database");
        match database.get_latest_price(&config.crypto_name) {
            Ok(price) if price > 0.0 => {
                debug!("Got SHANGHAISILVER price from database: {}", price);
                validate_price(price)?;
                return Ok(price);
            }
            Ok(price) => {
                debug!("Got SHANGHAISILVER price but it's zero or negative: {}", price);
            }
            Err(e) => {
                debug!("Failed to get SHANGHAISILVER from database: {}", e);
            }
        }
    }

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

/// Test Discord connectivity by making a simple API call
async fn test_discord_connectivity(health: Arc<HealthState>) {
    use reqwest::Client;
    
    let client = match Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client for Discord test: {}", e);
            health.increment_discord_test_failures();
            return;
        }
    };
    
    match client
        .get("https://discord.com/api/v10/gateway")
        .header("User-Agent", "Discord-Bot-Health-Check/1.0")
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                debug!("Discord connectivity test passed");
                health.update_discord_test_timestamp();
                health.reset_discord_test_failures();
            } else {
                warn!("Discord connectivity test failed with status: {}", response.status());
                health.increment_discord_test_failures();
            }
        }
        Err(e) => {
            warn!("Discord connectivity test failed with error: {}", e);
            health.increment_discord_test_failures();
        }
    }
}
