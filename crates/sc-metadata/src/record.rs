//! `ProScan` Transmission and Scanner Metadata Record.

use serde::{Deserialize, Serialize};

/// Transmission and scanner metadata record reverse-engineered from `ProScan.exe`.
///
/// Maintains exact 22-field payload sequence and fixed-width padding for binary compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProScanMetadata {
    #[serde(alias = "Scanner", default = "default_scanner")]
    pub scanner: String,
    #[serde(alias = "EndingDate", default = "default_ending_date")]
    pub ending_date: String,
    #[serde(alias = "FavoriteName", default)]
    pub favorite_name: String,
    #[serde(alias = "SystemName", default)]
    pub system_name: String,
    #[serde(alias = "SiteName", default)]
    pub site_name: String,
    #[serde(alias = "DepartmentName", default)]
    pub department_name: String,
    #[serde(alias = "ChannelName", default)]
    pub channel_name: String,
    #[serde(alias = "Frequency", default)]
    pub frequency: String,
    #[serde(alias = "Modulation", default = "default_modulation")]
    pub modulation: String,
    #[serde(alias = "Tone", default)]
    pub tone: String,
    #[serde(alias = "TGID", default)]
    pub tgid: String,
    #[serde(alias = "UID", default)]
    pub uid: String,
    #[serde(alias = "UID#", default)]
    pub uid_number: String,
    #[serde(alias = "FTOA", default)]
    pub ftoa: String,
    #[serde(alias = "FTOB", default)]
    pub ftob: String,
    #[serde(alias = "RSSI", default)]
    pub rssi: String,
    #[serde(alias = "AlertColor", default)]
    pub alert_color: String,
    #[serde(alias = "ServiceType", default)]
    pub service_type: String,
    #[serde(alias = "ServiceColor", default)]
    pub service_color: String,
    #[serde(alias = "DisplayedSystemType", default)]
    pub displayed_system_type: String,
    #[serde(alias = "DigitalStatus", default)]
    pub digital_status: String,
    #[serde(alias = "DMRSlot", default)]
    pub dmr_slot: String,
    #[serde(alias = "ScannerMode", default = "default_scanner_mode")]
    pub scanner_mode: String,
    #[serde(alias = "InfoSCState", default)]
    pub info_sc_state: String,
    #[serde(alias = "CommPort", default = "default_comm_port")]
    pub comm_port: String,
    #[serde(alias = "ProgramPath", default)]
    pub program_path: String,
    #[serde(alias = "ProgramFilename", default)]
    pub program_filename: String,
}

fn default_scanner() -> String {
    "BCD996P2".to_string()
}

fn default_ending_date() -> String {
    "00000000000000".to_string()
}

fn default_modulation() -> String {
    "NFM".to_string()
}

fn default_scanner_mode() -> String {
    "Scan".to_string()
}

fn default_comm_port() -> String {
    "5".to_string()
}
