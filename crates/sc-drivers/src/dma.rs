//! Uniden DMA Series driver (`ScannerClass 2`: BCD996P2, BCD325P2, BCD996XT, BCD396XT, BCT15X, etc.).
//!
//! Protocol reference: `DMA-VS-HP-ARCHITECTURE.md` §2 (`RadioSpecific.cs:1042`) and `drivers/bcd996p2.py`.

use std::time::{Duration, Instant};

use sc_core::{normalize_frequency, ScannerConfig, ScannerStatus};
use tracing::{error, info, warn};

use crate::parse_util::{
    extract_section, now_unix_seconds, set_inactive_status, update_sts_display_lines,
};
use crate::transport::Transport;

/// Pipelined multi-command poll string for DMA series scanners (`RadioSpecific.cs:1042`).
pub const DMA_POLL_COMMAND: &str = "MDL\rSTS\rGLG\rVOL\rSQL\rPWR";

/// Pure deterministic parser for a DMA pipelined response (`MDL`, `PWR`, `GLG`, `STS`).
#[allow(clippy::too_many_lines)]
pub fn parse_dma_response(res: &str, status: &mut ScannerStatus) {
    if res.trim().is_empty() {
        return;
    }

    update_sts_display_lines(res, status);

    // 1. Model identification (MDL,)
    if let Some(mdl) = extract_section(res, "MDL,") {
        let trimmed = mdl.trim();
        if !trimmed.is_empty() {
            status.model = trimmed.to_string();
        }
    }

    // 2. Parse PWR block (PWR,rssi,freq)
    let mut freq_val: Option<f64> = None;
    let mut rssi_val: i32 = 0;

    if let Some(pwr_str) = extract_section(res, "PWR,") {
        let pwr_tokens: Vec<&str> = pwr_str.split(',').map(str::trim).collect();
        if pwr_tokens.len() >= 2 {
            if let Ok(r) = pwr_tokens[0].parse::<i32>() {
                rssi_val = r;
            }
            if let Ok(raw_f) = pwr_tokens[1].parse::<f64>() {
                freq_val = normalize_frequency(Some(raw_f));
            }
        }
    }

    // 3. Parse GLG block (GLG,freq/tgid,mod,att,tone,system,group,channel,activity,...)
    let glg_opt = extract_section(res, "GLG,").or_else(|| {
        res.split_once("GLG,")
            .and_then(|(_, rest)| rest.split('\r').next())
            .map(|s| s.trim().to_string())
    });

    if let Some(glg_str) = glg_opt {
        let glg: Vec<&str> = glg_str.split(',').map(str::trim).collect();
        if glg.len() > 7 {
            let mut is_receiving = glg[7] == "1";

            // Index 0: Polymorphic Frequency or Talkgroup
            let item0 = glg[0].replace("ID:", "").trim().to_string();
            let mut tgid_val: Option<String> = None;

            if item0.contains('.') {
                if let Ok(raw_f) = item0.parse::<f64>() {
                    freq_val = normalize_frequency(Some(raw_f));
                }
            } else if (item0.len() == 7 || item0.len() == 8)
                && item0.chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(raw_f) = item0.parse::<f64>() {
                    if let Some(norm_f) = normalize_frequency(Some(raw_f)) {
                        freq_val = Some(norm_f);
                    } else {
                        tgid_val = Some(item0);
                    }
                } else if !item0.is_empty() {
                    tgid_val = Some(item0);
                }
            } else if !item0.is_empty() {
                tgid_val = Some(item0);
            }

            // Modulation at index 1
            if glg.len() > 1 && !glg[1].is_empty() {
                status.modulation = glg[1].to_string();
            }

            // Hierarchy names
            let sys_name = if glg.len() > 4 { glg[4] } else { "" };
            let dept_name = if glg.len() > 5 { glg[5] } else { "" };
            let mut chan_name = if glg.len() > 6 {
                glg[6].to_string()
            } else {
                String::new()
            };

            // Digital code / color code at index 11, or CTCSS/DCS tone at index 3
            if glg.len() > 11 && !glg[11].is_empty() {
                let code = glg[11].replace("NONE", "");
                let code = code.trim();
                if code.starts_with("10") && code.len() == 4 {
                    if let Ok(cc) = u32::from_str_radix(&code[2..], 16) {
                        status.tone = Some(format!("CC {cc}"));
                    }
                } else if code.starts_with('8') {
                    if let Ok(val) = code.parse::<i32>() {
                        let ran = val - 8192;
                        if (0..=63).contains(&ran) {
                            status.tone = Some(format!("R {ran}"));
                        }
                    }
                }
            } else if glg.len() > 3 && !glg[3].is_empty() && glg[3] != "0" {
                let t = glg[3];
                if t.contains('.') {
                    status.tone = Some(format!("C {t}"));
                } else {
                    status.tone = Some(format!("D {t}"));
                }
            }

            // Invalidate activity if frequency and TGID are both blank
            if freq_val.is_none() && tgid_val.is_none() {
                is_receiving = false;
            }

            if is_receiving && chan_name.is_empty() {
                if let Some(f) = freq_val {
                    chan_name = format!("{f:.4} MHz");
                } else if let Some(ref tg) = tgid_val {
                    chan_name = format!("TGID {tg}");
                } else {
                    chan_name = "Active Channel".to_string();
                }
            }

            status.system_name = if sys_name.is_empty() {
                "Scanning...".to_string()
            } else {
                sys_name.to_string()
            };
            status.dept_name = dept_name.to_string();
            status.channel_name = if is_receiving {
                chan_name.clone()
            } else {
                "Scanning...".to_string()
            };
            status.frequency = freq_val;
            status.tgid = tgid_val;
            status.receiving = is_receiving;
            if !is_receiving {
                status.avoided = false;
            }
            status.rssi = if is_receiving { rssi_val } else { 0 };
            status.status_line = if is_receiving {
                format!("RECV: {chan_name}")
            } else {
                "SCANNING".to_string()
            };
            status.last_update = now_unix_seconds();
            return;
        }
    }

    // Fallback: if GLG was not returned, check if STS was returned
    if let Some(sts_str) = extract_section(res, "STS,") {
        let sts_parts: Vec<&str> = sts_str.split(',').map(str::trim).collect();
        if sts_parts.len() > 6 {
            status.system_name = if sts_parts[6].is_empty() {
                "Scanning...".to_string()
            } else {
                sts_parts[6].to_string()
            };
        }
        status.receiving = false;
        status.status_line = "SCANNING".to_string();
        status.last_update = now_unix_seconds();
    }
}

/// Stateful DMA scanner driver wrapping a [`Transport`].
pub struct DmaDriver<T: Transport> {
    config: ScannerConfig,
    status: ScannerStatus,
    transport: T,
    last_connect_attempt: Option<Instant>,
    connect_retry_interval: Duration,
    logged_connection_error: bool,
}

impl<T: Transport> DmaDriver<T> {
    /// Creates a new `DmaDriver` with the given configuration and transport.
    #[must_use]
    pub fn new(config: ScannerConfig, transport: T) -> Self {
        let mut status = ScannerStatus::new(&config.id, &config.name, config.model.as_str());
        if config.enabled {
            status.connected = transport.is_connected();
        } else {
            set_inactive_status(&mut status, "DISABLED");
        }
        Self {
            config,
            status,
            transport,
            last_connect_attempt: None,
            connect_retry_interval: Duration::from_secs(4),
            logged_connection_error: false,
        }
    }

    /// Reconstructs a `DmaDriver` from parts while preserving an open transport during family switching.
    #[must_use]
    pub fn from_parts(config: ScannerConfig, mut status: ScannerStatus, transport: T) -> Self {
        if !config.enabled {
            set_inactive_status(&mut status, "DISABLED");
        }
        Self {
            config,
            status,
            transport,
            last_connect_attempt: None,
            connect_retry_interval: Duration::from_secs(4),
            logged_connection_error: false,
        }
    }

    /// Consumes this driver, returning `(config, status, transport)` for zero-downtime family switching.
    #[must_use]
    pub fn into_parts(self) -> (ScannerConfig, ScannerStatus, T) {
        (self.config, self.status, self.transport)
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

    pub fn config_mut(&mut self) -> &mut ScannerConfig {
        &mut self.config
    }

    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Checks whether the driver should attempt to connect right now (port configured + retry backoff elapsed).
    pub fn should_try_connect(&mut self) -> bool {
        if !self.config.enabled {
            set_inactive_status(&mut self.status, "DISABLED");
            return false;
        }

        let port_val = self.config.port.trim().to_uppercase();
        if port_val.is_empty() || port_val == "NONE" || port_val == "DISABLED" {
            set_inactive_status(&mut self.status, "NO PORT (SELECT IN SETTINGS)");
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
                    port = %self.config.port,
                    "DMA scanner connected"
                );
                true
            }
            Err(e) => {
                if !self.logged_connection_error {
                    warn!(
                        scanner_id = %self.config.id,
                        port = %self.config.port,
                        error = %e,
                        "Could not connect DMA scanner; will retry periodically"
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
        self.status.display_lines.clear();
        self.transport.disconnect();
    }

    fn send_cmd(&mut self, cmd: &str) -> String {
        if !self.status.connected {
            return String::new();
        }
        match self.transport.send_command(cmd) {
            Ok(resp) => resp,
            Err(e) => {
                error!(scanner_id = %self.config.id, error = %e, "Error communicating with DMA scanner");
                self.status.connected = false;
                self.status.display_lines.clear();
                String::new()
            }
        }
    }

    /// Queries the scanner for live status and updates `self.status`.
    pub fn poll(&mut self) -> &ScannerStatus {
        if !self.status.connected && !self.connect() {
            return &self.status;
        }

        let res = self.send_cmd(DMA_POLL_COMMAND);
        if !res.is_empty() {
            parse_dma_response(&res, &mut self.status);
        }
        &self.status
    }

    /// Issues the verified DMA resume scan / skip command (`KEY,SCAN,P` -> `KEY,S,P` -> `KEY,>,P`).
    pub fn send_skip(&mut self) -> bool {
        info!(scanner_id = %self.config.id, "Sending KEY,SCAN,P (Resume Scan / Skip)");
        let mut res = self.send_cmd("KEY,SCAN,P");
        if res.is_empty() || res.contains("ERR") || res.contains("NG") {
            res = self.send_cmd("KEY,S,P");
        }
        if res.is_empty() || res.contains("ERR") || res.contains("NG") {
            res = self.send_cmd("KEY,>,P");
        }
        res.contains("OK") || !res.is_empty()
    }

    /// Temporarily locks out a talkgroup or frequency on the DMA scanner.
    pub fn temporary_lockout(&mut self, tgid_or_freq: &str) -> bool {
        info!(scanner_id = %self.config.id, target = %tgid_or_freq, "Temporarily locking out target");
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

    /// Releases a temporary lockout on the DMA scanner.
    pub fn release_lockout(&mut self, tgid_or_freq: &str) -> bool {
        info!(scanner_id = %self.config.id, target = %tgid_or_freq, "Releasing temporary lockout");
        let _ = self.send_cmd(&format!("UNLOCK,{tgid_or_freq}"));
        self.status.avoided = false;
        true
    }
}
