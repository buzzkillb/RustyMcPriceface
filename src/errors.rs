use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP request error: {0}")]
    Http(String),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Environment variable not set: {0}")]
    EnvVar(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("System time error: {0}")]
    SystemTime(String),

    #[error("Price data not available for {0}")]
    PriceNotFound(String),

    #[error("Discord API error: {0}")]
    Discord(String),

    #[error("File I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),
}

impl From<std::env::VarError> for BotError {
    fn from(err: std::env::VarError) -> Self {
        BotError::EnvVar(err.to_string())
    }
}

impl From<std::num::ParseIntError> for BotError {
    fn from(err: std::num::ParseIntError) -> Self {
        BotError::Parse(err.to_string())
    }
}

impl From<std::num::ParseFloatError> for BotError {
    fn from(err: std::num::ParseFloatError) -> Self {
        BotError::Parse(err.to_string())
    }
}

pub type BotResult<T> = Result<T, BotError>; 