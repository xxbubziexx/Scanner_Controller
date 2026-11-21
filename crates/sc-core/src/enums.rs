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
    /// All 34 supported `ScannerModel` variants in canonical order.
    pub const ALL: [Self; 34] = [
        Self::BCD436HP,
        Self::BCD996P2,
        Self::BCD536HP,
        Self::SDS100,
        Self::SDS200,
        Self::BCD325P2,
        Self::BCD996XT,
        Self::BCD396XT,
        Self::BCT15X,
        Self::BCT15,
        Self::BC346XT,
        Self::BC346XTC,
        Self::BR330T,
        Self::BCD396T,
        Self::BCD996T,
        Self::BC250D,
        Self::BC296D,
        Self::BC780XLT,
        Self::BC785D,
        Self::BC796D,
        Self::SDS150,
        Self::SDS100E,
        Self::SDS200E,
        Self::USDS100,
        Self::UBCD3600XLT,
        Self::BCD160DN,
        Self::BCD260DN,
        Self::BC125AT,
        Self::UBCD160DN,
        Self::UBCD260DN,
        Self::UBCD436PT,
        Self::UBCD536PT,
        Self::UBC125XLT,
        Self::UBC126AT,
    ];

    /// Returns the exact Uniden `MDL` string representation for this model.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::BCD436HP => "BCD436HP",
            Self::BCD996P2 => "BCD996P2",
            Self::BCD536HP => "BCD536HP",
            Self::SDS100 => "SDS100",
            Self::SDS200 => "SDS200",
            Self::BCD325P2 => "BCD325P2",
            Self::BCD996XT => "BCD996XT",
            Self::BCD396XT => "BCD396XT",
            Self::BCT15X => "BCT15X",
            Self::BCT15 => "BCT15",
            Self::BC346XT => "BC346XT",
            Self::BC346XTC => "BC346XTC",
            Self::BR330T => "BR330T",
            Self::BCD396T => "BCD396T",
            Self::BCD996T => "BCD996T",
            Self::BC250D => "BC250D",
            Self::BC296D => "BC296D",
            Self::BC780XLT => "BC780XLT",
            Self::BC785D => "BC785D",
            Self::BC796D => "BC796D",
            Self::SDS150 => "SDS150",
            Self::SDS100E => "SDS100E",
            Self::SDS200E => "SDS200E",
            Self::USDS100 => "USDS100",
            Self::UBCD3600XLT => "UBCD3600XLT",
            Self::BCD160DN => "BCD160DN",
            Self::BCD260DN => "BCD260DN",
            Self::BC125AT => "BC125AT",
            Self::UBCD160DN => "UBCD160DN",
            Self::UBCD260DN => "UBCD260DN",
            Self::UBCD436PT => "UBCD436PT",
            Self::UBCD536PT => "UBCD536PT",
            Self::UBC125XLT => "UBC125XLT",
            Self::UBC126AT => "UBC126AT",
        }
    }

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

    /// Parses a `ScannerModel` from a raw `MDL` / `SI` response token (case-insensitive).
    #[must_use]
    pub fn from_mdl_str(raw: &str) -> Option<Self> {
        let token = raw
            .trim()
            .split([',', ' ', '\t', '\r', '\n'])
            .next()?
            .trim();
        if token.is_empty() {
            return None;
        }
        Self::ALL
            .iter()
            .copied()
            .find(|m| m.as_str().eq_ignore_ascii_case(token))
    }

    /// Matches a `ScannerModel` from a USB serial port product/description string
    /// (checks longest model names first so e.g. `USDS100` / `SDS100E` match before `SDS100`).
    #[must_use]
    pub fn from_usb_description(desc: &str) -> Option<Self> {
        let upper = desc.to_ascii_uppercase();
        if upper.trim().is_empty() {
            return None;
        }
        let mut best: Option<(usize, Self)> = None;
        for m in Self::ALL {
            let name = m.as_str();
            if upper.contains(name) {
                let len = name.len();
                if best.is_none_or(|(best_len, _)| len > best_len) {
                    best = Some((len, m));
                }
            }
        }
        best.map(|(_, m)| m)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_mdl_str_and_usb_description() {
        assert_eq!(
            ScannerModel::from_mdl_str("BCD436HP"),
            Some(ScannerModel::BCD436HP)
        );
        assert_eq!(
            ScannerModel::from_mdl_str(" bcd996p2,V1.0\r"),
            Some(ScannerModel::BCD996P2)
        );
        assert_eq!(
            ScannerModel::from_mdl_str("SDS100"),
            Some(ScannerModel::SDS100)
        );
        assert_eq!(ScannerModel::from_mdl_str("invalid"), None);
        assert_eq!(ScannerModel::from_mdl_str(""), None);

        assert_eq!(
            ScannerModel::from_usb_description("BCD436HP Serial Port"),
            Some(ScannerModel::BCD436HP)
        );
        assert_eq!(
            ScannerModel::from_usb_description("Uniden USDS100 Serial Port"),
            Some(ScannerModel::USDS100)
        );
        assert_eq!(
            ScannerModel::from_usb_description("Uniden BCT15X"),
            Some(ScannerModel::BCT15X)
        );
        assert_eq!(
            ScannerModel::from_usb_description("USB Serial Device"),
            None
        );
    }
}
