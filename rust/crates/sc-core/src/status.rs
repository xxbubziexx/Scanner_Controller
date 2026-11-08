//! Live scanner status structures and frequency normalization.

use serde::{Deserialize, Serialize};

/// Normalizes raw scanner frequency values to standard MHz.
///
/// Handles:
/// - Floats already in MHz (e.g. 155.7975, 851.25)
/// - 100Hz integer units (e.g. 1557975 -> 155.7975, 8512500 -> 851.25)
/// - 10Hz integer units (e.g. 15579750 -> 155.7975, 85350000 -> 853.5)
/// - Hz integer units (e.g. 155797500 -> 155.7975)
/// - Low-band 100Hz scaling (e.g. 296000 -> 29.6)
#[must_use]
pub fn normalize_frequency(val: Option<f64>) -> Option<f64> {
    let raw = val?;
    if raw <= 0.0 {
        return None;
    }

    // Direct match in valid scanner range (25.0 to 1300.0 MHz)
    if (25.0..=1300.0).contains(&raw) {
        return Some(round_freq(raw));
    }

    // 100Hz scaling (/ 10000.0)
    let s_hundred_hz = raw / 10_000.0;
    if (25.0..=1300.0).contains(&s_hundred_hz) {
        return Some(round_freq(s_hundred_hz));
    }

    // 10Hz scaling (/ 100000.0)
    let s_ten_hz = raw / 100_000.0;
    if (25.0..=1300.0).contains(&s_ten_hz) {
        return Some(round_freq(s_ten_hz));
    }

    // Hz scaling (/ 1000000.0)
    let s_one_hz = raw / 1_000_000.0;
    if (25.0..=1300.0).contains(&s_one_hz) {
        return Some(round_freq(s_one_hz));
    }

    // Iterative fallback
    let mut temp = raw;
    while temp > 1300.0 {
        temp /= 10.0;
    }
    if (25.0..=1300.0).contains(&temp) {
        Some(round_freq(temp))
    } else {
        Some(round_freq(raw))
    }
}

fn round_freq(f: f64) -> f64 {
    (f * 100_000.0).round() / 100_000.0
}

/// Represents the instantaneous operational state of a physical scanner radio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ScannerStatus {
    pub scanner_id: String,
    #[serde(default)]
    pub name: String,
    pub model: String,
    pub connected: bool,
    pub receiving: bool,
    pub system_name: String,
    pub dept_name: String,
    pub channel_name: String,
    pub tgid: Option<String>,
    pub frequency: Option<f64>,
    pub rssi: i32,
    pub modulation: String,
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    pub status_line: String,
    #[serde(default)]
    pub avoided: bool,
    #[serde(default)]
    pub last_update: f64,
    #[serde(default, skip_serializing)]
    pub mute_state: bool,
}

impl ScannerStatus {
    /// Creates a new inactive status initialized to default scanning state.
    #[must_use]
    pub fn new(
        scanner_id: impl Into<String>,
        name: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            scanner_id: scanner_id.into(),
            name: name.into(),
            model: model.into(),
            connected: false,
            receiving: false,
            system_name: "Scanning...".to_string(),
            dept_name: String::new(),
            channel_name: String::new(),
            tgid: None,
            frequency: None,
            rssi: 0,
            modulation: "NFM".to_string(),
            unit_id: None,
            tone: None,
            status_line: "SCANNING".to_string(),
            avoided: false,
            last_update: 0.0,
            mute_state: false,
        }
    }
}
