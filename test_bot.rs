// Minimal test to check what intents work
use serenity::{
    async_trait,
    model::gateway::Ready,
    prelude::*,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        println!("Bot is ready! Logged in as: {}", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_TOKEN_SOL").expect("Expected a token in the environment");
    
    // Try with absolutely no intents
    let intents = GatewayIntents::empty();
    
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}