//! `sc-core`: Configuration models, domain enums, and scanner status definitions.

#![deny(unsafe_code)]

pub mod config;
pub mod enums;
pub mod error;
pub mod status;

pub use config::{
    AppConfig, AudioLoopbackConfig, ScanScribeFeederConfig, ScannerConfig, SyncConfig,
};
pub use enums::{
    AudioChannelMode, ConnectionMode, ExclusionMode, FeederMode, LockoutMethod, PriorityMode,
    ScannerModel, ALLOWED_SAMPLE_RATES,
};
pub use error::ConfigError;
pub use status::{normalize_frequency, ScannerStatus};

/// Returns the version of the `sc-core` crate.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unreadable_literal)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(version(), "0.1.0");
    }

    #[test]
    fn test_default_config_validates() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_load_real_production_config_json() {
        // Read the actual config.json from repository root
        let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../config.json");
        let content =
            std::fs::read_to_string(config_path).expect("Failed to read root config.json");

        let config =
            AppConfig::from_json_str(&content).expect("Failed to deserialize root config.json");

        assert_eq!(config.scanner_a.id, "scanner_a");
        assert_eq!(config.scanner_a.model, ScannerModel::BCD996P2);
        assert_eq!(config.scanner_a.port, "COM5");
        assert_eq!(config.scanner_a.connection_mode, ConnectionMode::Serial);
        assert_eq!(config.scanner_a.audio_channel, AudioChannelMode::Mono);

        assert_eq!(config.scanner_b.id, "scanner_b");
        assert_eq!(config.scanner_b.model, ScannerModel::BCD996P2);
        assert_eq!(config.scanner_b.port, "NONE");
        assert!(!config.scanner_b.enabled);

        assert_eq!(config.sync.exclusion_mode, ExclusionMode::TgidAndFreq);
        assert_eq!(config.sync.priority_mode, PriorityMode::EqualPeers);

        assert_eq!(config.feeder.feeder_mode, FeederMode::DirectoryDrop);
        assert_eq!(config.feeder.sample_rate, 16000);
        assert_eq!(config.feeder.audio_format, "MP3");

        // Test roundtrip: serialize back to JSON and re-parse
        let serialized = config.to_json_string().expect("Serialization failed");
        let roundtrip = AppConfig::from_json_str(&serialized).expect("Roundtrip parsing failed");
        assert_eq!(config, roundtrip);
    }

    #[test]
    fn test_scanner_baud_rate_validation() {
        let mut cfg = ScannerConfig {
            id: "test".to_string(),
            name: "Test Radio".to_string(),
            model: ScannerModel::BC125AT,
            connection_mode: ConnectionMode::Serial,
            port: "COM1".to_string(),
            baud_rate: 115_200,
            ip_host: "127.0.0.1".to_string(),
            ip_port: 5000,
            enabled: true,
            poll_interval_ms: 150,
            lockout_method: LockoutMethod::Keypress,
            audio_device_index: None,
            audio_device_name: None,
            audio_channel: AudioChannelMode::Mono,
            audio_enabled: true,
            filename_template: None,
            tit2_template: None,
        };
        assert!(cfg.validate().is_ok());

        // BC125AT only supports 115200; 9600 must fail
        cfg.baud_rate = 9600;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_feeder_sample_rate_validation() {
        let mut feeder = ScanScribeFeederConfig::default();
        assert!(feeder.validate().is_ok());

        // Test all 5 allowed rates
        for rate in [8000, 11025, 16000, 22050, 44100] {
            feeder.sample_rate = rate;
            assert!(feeder.validate().is_ok());
        }

        // Invalid rates must fail
        for invalid in [0, 4000, 12000, 48000, 96000] {
            feeder.sample_rate = invalid;
            assert!(feeder.validate().is_err());
        }
    }

    #[test]
    fn test_frequency_normalization() {
        assert_eq!(normalize_frequency(Some(155.7975)), Some(155.7975));
        assert_eq!(normalize_frequency(Some(851.25)), Some(851.25));
        // 10Hz units
        assert_eq!(normalize_frequency(Some(15_579_750.0)), Some(155.7975));
        // 100Hz units
        assert_eq!(normalize_frequency(Some(1_557_975.0)), Some(155.7975));
        // Non-positive or None
        assert_eq!(normalize_frequency(Some(0.0)), None);
        assert_eq!(normalize_frequency(None), None);
    }
}
