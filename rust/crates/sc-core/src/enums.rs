//! Domain enums and scanner model variants for `Scanner_Controller`.

use serde::{Deserialize, Serialize};

/// Allowed audio sampling rates per second (`ProScan` standard, NONE ELSE).
pub const ALLOWED_SAMPLE_RATES: [u32; 5] = [8000, 11025, 16000, 22050, 44100];

/// Scanner communication connection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectionMode {
    #[default]
    Serial,
    Ip,
}

/// All 34 supported Uniden scanner models (DMA, `HomePatrol`, and Classic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScannerModel {
    BCD436HP,
    BCD996P2,
    BCD536HP,
    SDS100,
    SDS200,
    BCD325P2,
    BCD996XT,
    BCD396XT,
    BCT15X,
    BCT15,
    BC346XT,
    BC346XTC,
    BR330T,
    BCD396T,
    BCD996T,
    BC250D,
    BC296D,
    BC780XLT,
    BC785D,
    BC796D,
    SDS150,
    SDS100E,
    SDS200E,
    USDS100,
    UBCD3600XLT,
    BCD160DN,
    BCD260DN,
    BC125AT,
    UBCD160DN,
    UBCD260DN,
    UBCD436PT,
    UBCD536PT,
    UBC125XLT,
    UBC126AT,
}

impl ScannerModel {
    /// Returns model-specific allowed baud rates based on `ProScan` specifications.
    #[must_use]
    pub fn allowed_baud_rates(&self) -> &'static [u32] {
        match self {
            Self::BCD160DN
            | Self::BCD260DN
            | Self::BC125AT
            | Self::UBCD160DN
            | Self::UBCD260DN
            | Self::UBC125XLT
            | Self::UBC126AT => &[115_200],
            Self::BC250D | Self::BC780XLT | Self::BC785D => &[19200, 9600, 4800, 2400],
            Self::BC296D | Self::BC796D => &[57600, 38400, 19200, 9600, 4800, 2400],
            _ => &[115_200, 57600, 38400, 19200, 9600, 4800],
        }
    }

    /// Checks if a given baud rate is valid for this model.
    #[must_use]
    pub fn is_baud_rate_allowed(&self, baud: u32) -> bool {
        self.allowed_baud_rates().contains(&baud)
    }

    /// Returns whether this model belongs to the `HomePatrol` / STS family.
    #[must_use]
    pub fn is_homepatrol(&self) -> bool {
        matches!(
            self,
            Self::BCD436HP
                | Self::BCD536HP
                | Self::SDS100
                | Self::SDS200
                | Self::SDS150
                | Self::SDS100E
                | Self::SDS200E
                | Self::USDS100
                | Self::UBCD3600XLT
                | Self::UBCD436PT
                | Self::UBCD536PT
        )
    }
}

/// Mutual exclusion channel lockout method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LockoutMethod {
    #[default]
    Keypress,
    Memory,
}

/// Priority resolution mode when both scanners conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriorityMode {
    #[default]
    EqualPeers,
    PrimarySecondary,
    DepartmentPriority,
}

/// Matching criteria for detecting duplicate radio calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExclusionMode {
    #[default]
    TgidAndFreq,
    TgidOnly,
    FreqOnly,
}

/// Audio input capture channel routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudioChannelMode {
    #[default]
    Mono,
    Left,
    Right,
}

/// Feeder dispatch mode for `ScanScribe` ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FeederMode {
    #[default]
    DirectoryDrop,
    HttpWebhook,
    LiveStream,
}
