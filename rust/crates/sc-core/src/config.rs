//! Application and hardware configuration structs and serialization logic.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::enums::{
    AudioChannelMode, ConnectionMode, ExclusionMode, FeederMode, LockoutMethod, PriorityMode,
    ScannerModel, ALLOWED_SAMPLE_RATES,
};
use crate::error::ConfigError;

/// Configuration for an individual physical scanner channel (Scanner A or B).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub id: String,
    pub name: String,
    pub model: ScannerModel,
    #[serde(default)]
    pub connection_mode: ConnectionMode,
    #[serde(default = "default_com_port")]
    pub port: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_ip_host")]
    pub ip_host: String,
    #[serde(default = "default_ip_port")]
    pub ip_port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub lockout_method: LockoutMethod,
    #[serde(default)]
    pub audio_device_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_device_name: Option<String>,
    #[serde(default)]
    pub audio_channel: AudioChannelMode,
    #[serde(default = "default_true")]
    pub audio_enabled: bool,
    #[serde(default)]
    pub filename_template: Option<String>,
    #[serde(default)]
    pub tit2_template: Option<String>,
}

fn default_com_port() -> String {
    "COM5".to_string()
}

const fn default_baud_rate() -> u32 {
    115_200
}

fn default_ip_host() -> String {
    "127.0.0.1".to_string()
}

const fn default_ip_port() -> u16 {
    5000
}

const fn default_true() -> bool {
    true
}

const fn default_poll_interval_ms() -> u64 {
    150
}

impl ScannerConfig {
    /// Validates scanner configuration parameters (e.g. allowed baud rate for model).
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidBaudRate`] if `baud_rate` is not supported by `model`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.model.is_baud_rate_allowed(self.baud_rate) {
            return Err(ConfigError::InvalidBaudRate {
                model: format!("{:?}", self.model),
                baud_rate: self.baud_rate,
                allowed: self.model.allowed_baud_rates().to_vec(),
            });
        }
        Ok(())
    }
}

/// Configuration for the `ScanScribe` AI transcription dispatcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanScribeFeederConfig {
    #[serde(default = "default_true")]
    pub enable_feeder: bool,
    #[serde(default)]
    pub feeder_mode: FeederMode,
    #[serde(default = "default_inbox_directory")]
    pub inbox_directory: String,
    #[serde(default = "default_filename_template")]
    pub filename_template: String,
    #[serde(default = "default_tit2_template")]
    pub tit2_template: String,
    #[serde(default = "default_webhook_url")]
    pub webhook_url: String,
    #[serde(default = "default_audio_format")]
    pub audio_format: String,
    #[serde(default)]
    pub audio_device_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_device_name: Option<String>,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_true")]
    pub enable_sidecar_json: bool,
    #[serde(default = "default_true")]
    pub enable_proscan_tags: bool,
}

fn default_inbox_directory() -> String {
    "A:\\Scanscribe_Projects\\scanscribe\\inbox".to_string()
}

fn default_filename_template() -> String {
    "%DT - %S - %C (%TG)".to_string()
}

fn default_tit2_template() -> String {
    "%C (%TG)".to_string()
}

fn default_webhook_url() -> String {
    "http://127.0.0.1:5000/api/ingest".to_string()
}

fn default_audio_format() -> String {
    "WAV".to_string()
}

const fn default_sample_rate() -> u32 {
    16000
}

impl Default for ScanScribeFeederConfig {
    fn default() -> Self {
        Self {
            enable_feeder: true,
            feeder_mode: FeederMode::DirectoryDrop,
            inbox_directory: default_inbox_directory(),
            filename_template: default_filename_template(),
            tit2_template: default_tit2_template(),
            webhook_url: default_webhook_url(),
            audio_format: default_audio_format(),
            audio_device_index: None,
            audio_device_name: None,
            sample_rate: default_sample_rate(),
            enable_sidecar_json: true,
            enable_proscan_tags: true,
        }
    }
}

impl ScanScribeFeederConfig {
    /// Validates sample rate and audio container formats.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidSampleRate`] if `sample_rate` is not in [`ALLOWED_SAMPLE_RATES`].
    /// Returns [`ConfigError::InvalidAudioFormat`] if `audio_format` is neither "WAV" nor "MP3".
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !ALLOWED_SAMPLE_RATES.contains(&self.sample_rate) {
            return Err(ConfigError::InvalidSampleRate(self.sample_rate));
        }
        let fmt = self.audio_format.to_uppercase();
        if fmt != "WAV" && fmt != "MP3" {
            return Err(ConfigError::InvalidAudioFormat(self.audio_format.clone()));
        }
        Ok(())
    }
}

/// Synchronization and mutual exclusion configuration between dual scanners.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_true")]
    pub enable_sync: bool,
    #[serde(default)]
    pub exclusion_mode: ExclusionMode,
    #[serde(default)]
    pub priority_mode: PriorityMode,
    #[serde(default = "default_primary_id")]
    pub primary_scanner_id: String,
    #[serde(default = "default_hang_time")]
    pub hang_time_seconds: f64,
    #[serde(default = "default_auto_release")]
    pub auto_release_seconds: f64,
}

fn default_primary_id() -> String {
    "scanner_a".to_string()
}

const fn default_hang_time() -> f64 {
    2.0
}

const fn default_auto_release() -> f64 {
    30.0
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enable_sync: true,
            exclusion_mode: ExclusionMode::TgidAndFreq,
            priority_mode: PriorityMode::EqualPeers,
            primary_scanner_id: default_primary_id(),
            hang_time_seconds: default_hang_time(),
            auto_release_seconds: default_auto_release(),
        }
    }
}

/// Real-time audio loopback monitor configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioLoopbackConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub output_device_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_device_name: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default = "default_pan_mode")]
    pub pan_mode: String,
}

const fn default_volume() -> f64 {
    0.8
}

fn default_pan_mode() -> String {
    "STEREO_SPLIT".to_string()
}

impl Default for AudioLoopbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            output_device_index: None,
            output_device_name: None,
            volume: default_volume(),
            pan_mode: default_pan_mode(),
        }
    }
}

/// Top-level application configuration containing dual scanners, sync, feeder, and loopback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub scanner_a: ScannerConfig,
    pub scanner_b: ScannerConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub feeder: ScanScribeFeederConfig,
    #[serde(default)]
    pub audio_loopback: AudioLoopbackConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scanner_a: ScannerConfig {
                id: "scanner_a".to_string(),
                name: "Scanner A".to_string(),
                model: ScannerModel::BCD436HP,
                connection_mode: ConnectionMode::Serial,
                port: "COM5".to_string(),
                baud_rate: 115_200,
                ip_host: "127.0.0.1".to_string(),
                ip_port: 5000,
                enabled: true,
                poll_interval_ms: 150,
                lockout_method: LockoutMethod::Keypress,
                audio_device_index: None,
                audio_device_name: None,
                audio_channel: AudioChannelMode::Left,
                audio_enabled: true,
                filename_template: Some("%DT - %S - %C (%TG)".to_string()),
                tit2_template: Some("%C (%TG)".to_string()),
            },
            scanner_b: ScannerConfig {
                id: "scanner_b".to_string(),
                name: "Scanner B".to_string(),
                model: ScannerModel::BCD996P2,
                connection_mode: ConnectionMode::Serial,
                port: "NONE".to_string(),
                baud_rate: 115_200,
                ip_host: "127.0.0.1".to_string(),
                ip_port: 5000,
                enabled: true,
                poll_interval_ms: 150,
                lockout_method: LockoutMethod::Keypress,
                audio_device_index: None,
                audio_device_name: None,
                audio_channel: AudioChannelMode::Right,
                audio_enabled: true,
                filename_template: Some("%DT - %S - %C (%TG)".to_string()),
                tit2_template: Some("%C (%TG)".to_string()),
            },
            sync: SyncConfig::default(),
            feeder: ScanScribeFeederConfig::default(),
            audio_loopback: AudioLoopbackConfig::default(),
        }
    }
}

impl AppConfig {
    /// Deserializes `AppConfig` from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Json`] on invalid JSON syntax, or [`ConfigError`] if validation fails.
    pub fn from_json_str(json_str: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_json::from_str(json_str)?;
        config.validate()?;
        Ok(config)
    }

    /// Serializes `AppConfig` to a formatted JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Json`] if serialization fails.
    pub fn to_json_string(&self) -> Result<String, ConfigError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Loads `AppConfig` from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] on read failure, or [`ConfigError`] on parse or validation failure.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    /// Saves `AppConfig` as formatted JSON to a file on disk.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if validation, serialization, or file write fails.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        self.validate()?;
        let content = self.to_json_string()?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validates all sub-configurations.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if any scanner or feeder configuration is invalid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.scanner_a.validate()?;
        self.scanner_b.validate()?;
        self.feeder.validate()?;
        Ok(())
    }
}
