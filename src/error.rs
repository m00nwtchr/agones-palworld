use std::result::Result as StdResult;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("config: {0}")]
    Config(String),

    #[error("agones: {0}")]
    Agones(#[from] agones::errors::Error),

    #[error("palworld http {0}: {1}")]
    PalworldHttp(reqwest::StatusCode, String),

    #[error("palworld timeout after {0}s")]
    PalworldTimeout(u64),

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("url: {0}")]
    Url(#[from] url::ParseError),

    #[error("otel: {0}")]
    Otel(#[from] opentelemetry_sdk::error::Error),

    #[error("signal: {0}")]
    Signal(String),
}

pub type AppResult<T> = StdResult<T, AppError>;
