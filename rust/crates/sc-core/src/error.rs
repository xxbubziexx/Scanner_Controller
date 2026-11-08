//! Error types for `sc-core`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error while reading or writing configuration: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization or deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid sample rate: {0}. Allowed rates are: 8000, 11025, 16000, 22050, 44100")]
    InvalidSampleRate(u32),

    #[error("Invalid audio format: '{0}'. Must be 'WAV' or 'MP3'")]
    InvalidAudioFormat(String),

    #[error(
        "Invalid baud rate {baud_rate} for scanner model '{model}'. Allowed rates: {allowed:?}"
    )]
    InvalidBaudRate {
        model: String,
        baud_rate: u32,
        allowed: Vec<u32>,
    },
}
