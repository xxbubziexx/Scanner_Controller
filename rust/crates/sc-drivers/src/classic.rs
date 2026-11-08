//! Uniden Classic / DN Series driver (`ScannerClass 1 & 5`: BC125AT, BCD160DN, BCD260DN, BC780XLT, etc.).
//!
//! Protocol reference: `DMA-VS-HP-ARCHITECTURE.md` §1.4 & `drivers/classic_driver.py`.

use std::time::{Duration, Instant};

use sc_core::{ScannerConfig, ScannerStatus};
use tracing::{error, info, warn};

use crate::parse_util::now_unix_seconds;
use crate::transport::Transport;

/// Pure deterministic parser for Uniden Classic `STS` / `STC` responses.
pub fn parse_classic_response(res: &str, status: &mut ScannerStatus) {
    if res.trim().is_empty() {
        return;
    }

    let parts: Vec<&str> = if res.contains(',') {
        res.split(',').collect()
    } else {
        res.split('\t').collect()
    };

    if parts.len() >= 3 && matches!(parts[0].trim(), "STS" | "STC") {
        let system_name = parts.get(1).map_or("Scanning...", |s| s.trim());
        let channel_name = parts.get(2).map_or("", |s| s.trim());
        let freq_str = parts.get(3).map_or("", |s| s.trim());

        let freq_val: Option<f64> = if freq_str.is_empty() {
            None
        } else {
            freq_str.parse::<f64>().ok()
        };

        let is_receiving = !channel_name.is_empty()
            && !matches!(channel_name, "Scanning..." | "SCAN")
            && (freq_val.is_some() || parts.len() > 3);
        let rssi_val = if is_receiving { 4 } else { 0 };

        status.system_name = if system_name.is_empty() {
            "Scanning...".to_string()
        } else {
            system_name.to_string()
        };
        status.dept_name = String::new();
        status.channel_name = if is_receiving {
            channel_name.to_string()
        } else {
            "Scanning...".to_string()
        };
        status.tgid = None;
        status.frequency = freq_val;
        status.receiving = is_receiving;
        if !is_receiving {
            status.avoided = false;
        }
        status.rssi = rssi_val;
        status.status_line = if is_receiving {
            format!("RECV: {channel_name}")
        } else {
            "SCANNING".to_string()
        };
        status.last_update = now_unix_seconds();
    }
}

/// Stateful Uniden Classic / DN scanner driver wrapping a [`Transport`].
pub struct ClassicDriver<T: Transport> {
    config: ScannerConfig,
    status: ScannerStatus,
    transport: T,
    last_connect_attempt: Option<Instant>,
    connect_retry_interval: Duration,
    logged_connection_error: bool,
}

impl<T: Transport> ClassicDriver<T> {
    /// Creates a new `ClassicDriver` with the given configuration and transport.
    #[must_use]
    pub fn new(config: ScannerConfig, transport: T) -> Self {
        let mut status = ScannerStatus::new(&config.id, &config.name, config.model.as_str());
        status.connected = transport.is_connected();
        Self {
            config,
            status,
            transport,
            last_connect_attempt: None,
            connect_retry_interval: Duration::from_secs(4),
            logged_connection_error: false,
        }
    }

    #[must_use]
    pub fn status(&self) -> &ScannerStatus {
        &self.status
    }

    pub fn status_mut(&mut self) -> &mut ScannerStatus {
        &mut self.status
    }

    #[must_use]
    pub fn config(&self) -> &ScannerConfig {
        &self.config
    }

    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Checks whether the driver should attempt to connect right now.
    pub fn should_try_connect(&mut self) -> bool {
        if !self.config.enabled {
            self.status.connected = false;
            self.status.status_line = "DISABLED".to_string();
            return false;
        }

        let port_val = self.config.port.trim().to_uppercase();
        if port_val.is_empty() || port_val == "NONE" || port_val == "DISABLED" {
            self.status.connected = false;
            self.status.status_line = "NO PORT (SELECT IN SETTINGS)".to_string();
            return false;
        }

        if let Some(last) = self.last_connect_attempt {
            if last.elapsed() < self.connect_retry_interval {
                return false;
            }
        }

        self.last_connect_attempt = Some(Instant::now());
        true
    }

    /// Connects the underlying transport.
    pub fn connect(&mut self) -> bool {
        if !self.should_try_connect() {
            return false;
        }

        match self.transport.connect() {
            Ok(()) => {
                self.status.connected = true;
                self.logged_connection_error = false;
                info!(
                    scanner_id = %self.config.id,
                    model = %self.config.model.as_str(),
                    port = %self.config.port,
                    "Classic scanner connected"
                );
                true
            }
            Err(e) => {
                if !self.logged_connection_error {
                    warn!(
                        scanner_id = %self.config.id,
                        port = %self.config.port,
                        error = %e,
                        "Could not connect Classic scanner; will retry periodically"
                    );
                    self.logged_connection_error = true;
                }
                self.status.connected = false;
                self.status.status_line = format!("PORT ERROR ({})", self.config.port);
                false
            }
        }
    }

    /// Disconnects the underlying transport.
    pub fn disconnect(&mut self) {
        self.status.connected = false;
        self.transport.disconnect();
    }

    fn send_cmd(&mut self, cmd: &str) -> String {
        if !self.status.connected {
            return String::new();
        }
        match self.transport.send_command(cmd) {
            Ok(resp) => resp,
            Err(e) => {
                error!(scanner_id = %self.config.id, error = %e, "Error communicating with Classic scanner");
                self.status.connected = false;
                String::new()
            }
        }
    }

    /// Queries the scanner for live status (`STS`, fallback `STC`) and updates `self.status`.
    pub fn poll(&mut self) -> &ScannerStatus {
        if !self.status.connected && !self.connect() {
            return &self.status;
        }

        let mut res = self.send_cmd("STS");
        if res.is_empty() || res.contains("ERR") {
            res = self.send_cmd("STC");
        }
        if !res.is_empty() {
            parse_classic_response(&res, &mut self.status);
        }
        &self.status
    }

    /// Issues Classic Lockout/Skip keypress (`KEY,L/O,P`, fallback `KEY,LO,P`).
    pub fn send_skip(&mut self) -> bool {
        info!(scanner_id = %self.config.id, "Sending KEY,L/O,P (Skip/Lockout)");
        let mut res = self.send_cmd("KEY,L/O,P");
        if res.is_empty() || res.contains("ERR") {
            res = self.send_cmd("KEY,LO,P");
        }
        self.status.avoided = true;
        res.contains("OK") || !res.is_empty()
    }

    /// Temporarily locks out a talkgroup or frequency on the Classic scanner.
    pub fn temporary_lockout(&mut self, tgid_or_freq: &str) -> bool {
        let freq_matches = self
            .status
            .frequency
            .is_some_and(|f| f.to_string() == tgid_or_freq);
        if self.status.tgid.as_deref() == Some(tgid_or_freq) || freq_matches {
            return self.send_skip();
        }
        let res = self.send_cmd(&format!("LOUT,{tgid_or_freq},TEMP"));
        res.contains("OK") || !res.is_empty()
    }

    /// Releases a temporary lockout on the Classic scanner.
    pub fn release_lockout(&mut self, tgid_or_freq: &str) -> bool {
        let _ = self.send_cmd(&format!("UNLOCK,{tgid_or_freq}"));
        self.status.avoided = false;
        true
    }
}
