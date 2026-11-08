//! Error types for the `sc-audio` crate.

use thiserror::Error;

/// Errors returned by audio capture, playback, resampling, or encoding operations.
#[derive(Debug, Error)]
pub enum AudioError {
    /// Invalid PCM byte buffer alignment (16-bit PCM requires even byte length).
    #[error("invalid 16-bit PCM byte length: {0} (must be a multiple of 2)")]
    InvalidPcmByteLength(usize),

    /// Unsupported sample rate.
    #[error("unsupported sample rate: {0} Hz")]
    UnsupportedSampleRate(u32),

    /// Resampling error.
    #[error("resampling error: {0}")]
    Resample(String),

    /// WAV encoding or validation error.
    #[error("WAV error: {0}")]
    Wav(String),

    /// MP3 encoding error.
    #[error("MP3 encoder error: {0}")]
    Mp3Encode(String),

    /// Audio hardware device or stream error.
    #[error("audio device error: {0}")]
    Device(String),
}
