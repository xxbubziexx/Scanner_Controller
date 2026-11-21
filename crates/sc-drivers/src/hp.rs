//! Uniden `HomePatrol` / x36HP / SDS Series driver (`ScannerClass 3 & 4`: BCD436HP, BCD536HP, SDS100, SDS200, etc.).
//!
//! Protocol reference: `DMA-VS-HP-ARCHITECTURE.md` §2 (`RadioSpecific.cs:1055`), `KEYPAD-AND-DISPLAY.md`,
//! and `drivers/bcd436hp.py`.

use std::thread;
use std::time::{Duration, Instant};

use quick_xml::events::Event;
use quick_xml::Reader;
use sc_core::{normalize_frequency, ScannerConfig, ScannerStatus};
use tracing::{debug, error, info, warn};

use crate::parse_util::{extract_section, now_unix_seconds, set_inactive_status};
use crate::transport::Transport;

/// Pipelined multi-command poll string for `HomePatrol` / SDS series scanners (`RadioSpecific.cs:1055`).
pub const HP_POLL_COMMAND: &str = "MDL\rSTS\rGLG\rVOL\rSQL\rPWR\rGSI,1";

const DMA_MODEL_NAMES: [&str; 11] = [
    "BCD996P2", "BCD325P2", "BCD996XT", "BCD396XT", "BCT15X", "BCT15", "BC346XT", "BC346XTC",
    "BR330T", "BCD996T", "BCD396T",
];

/// Strips non-printable display icons (`0x00..=0x1F`, `0x7F..=0xFF`) to spaces and trims whitespace.
#[must_use]
pub fn clean_display_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if (0x20..=0x7E).contains(&b) {
            out.push(b as char);
        } else {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

fn is_all_underscores(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b == b'_')
}

fn find_frequency_in_text(text: &str) -> Option<f64> {
    // Match `\d{2,4}\.\d{3,5}` in display text (e.g. "461.5250MHz")
    let bytes = text.as_bytes();
    for (dot_idx, &b) in bytes.iter().enumerate() {
        if b == b'.' {
            let mut start = dot_idx;
            while start > 0 && bytes[start - 1].is_ascii_digit() {
                start -= 1;
            }
            let int_len = dot_idx - start;
            if !(2..=4).contains(&int_len) {
                continue;
            }

            let mut end = dot_idx + 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let frac_len = end - (dot_idx + 1);
            if (3..=5).contains(&frac_len) {
                if let Ok(val) = text[start..end].parse::<f64>() {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn find_tgid_in_text(text: &str) -> Option<String> {
    // Case-insensitive search for `TGID:\s*(\d+)` or `TG:(\d+)`
    let upper = text.to_ascii_uppercase();
    for prefix in ["TGID:", "TG:"] {
        if let Some(pos) = upper.find(prefix) {
            let after = text[pos + prefix.len()..].trim_start();
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
    }
    None
}

/// Parses an LCD screen `STS` response when `GLG` or `GSI` XML are unavailable (`drivers/bcd436hp.py:137`).
#[allow(clippy::too_many_lines)]
pub fn parse_sts_fallback(res: &str, status: &mut ScannerStatus) {
    let parts: Vec<&str> = res.split(',').map(str::trim).collect();
    let dividers: Vec<usize> = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| if is_all_underscores(p) { Some(i) } else { None })
        .collect();

    let mut sys_name = "Scanning...".to_string();
    let mut dept_name = String::new();
    let mut chan_name = String::new();
    let mut freq_val: Option<f64> = None;
    let mut tgid_val: Option<String> = None;
    let mut sql_open = false;
    let mut rssi_val: i32 = 0;

    if dividers.len() >= 3 {
        let sys_lines: Vec<String> = (dividers[0] + 1..dividers[1])
            .filter(|&i| !is_all_underscores(parts[i]))
            .map(|i| clean_display_text(parts[i]))
            .filter(|s| !s.is_empty())
            .collect();

        let dept_lines: Vec<String> = (dividers[1] + 1..dividers[2])
            .filter(|&i| !is_all_underscores(parts[i]))
            .map(|i| clean_display_text(parts[i]))
            .filter(|s| !s.is_empty())
            .collect();

        let end_chan = if dividers.len() > 3 {
            dividers[3]
        } else {
            parts.len()
        };
        let chan_lines: Vec<String> = (dividers[2] + 1..end_chan)
            .filter(|&i| !is_all_underscores(parts[i]))
            .map(|i| clean_display_text(parts[i]))
            .filter(|s| !s.is_empty())
            .collect();

        if !sys_lines.is_empty() {
            let take_n = sys_lines.len().min(2);
            sys_name = sys_lines[..take_n].join(" ");
        }
        if let Some(first_dept) = dept_lines.first() {
            dept_name.clone_from(first_dept);
        }
        if let Some(first_chan) = chan_lines.first() {
            chan_name.clone_from(first_chan);
        }

        let chan_text_all = chan_lines.join(" ");
        freq_val = find_frequency_in_text(&chan_text_all);
        tgid_val = find_tgid_in_text(&chan_text_all);

        if parts.len() > 31 {
            sql_open = parts[30] == "1";
            if let Ok(r) = parts[31].parse::<i32>() {
                rssi_val = r;
            }
        }
    } else {
        if parts.len() > 6 {
            let cleaned = clean_display_text(parts[6]);
            if !cleaned.is_empty() {
                sys_name = cleaned;
            }
        }
        if parts.len() > 12 {
            dept_name = clean_display_text(parts[12]);
        }
        if parts.len() > 18 {
            chan_name = clean_display_text(parts[18]);
        }
    }

    let is_stopped = (!chan_name.is_empty()
        && !matches!(chan_name.as_str(), "Scanning..." | "SCAN" | "ID SEARCH"))
        || (sql_open && (freq_val.is_some() || tgid_val.is_some() || !dept_name.is_empty()));

    if is_stopped {
        let is_receiving = sql_open;
        status.system_name = sys_name;
        status.dept_name = dept_name;
        status.channel_name.clone_from(&chan_name);
        status.frequency = freq_val;
        status.tgid = tgid_val;
        status.receiving = is_receiving;
        status.rssi = if rssi_val > 0 {
            rssi_val
        } else if is_receiving {
            4
        } else {
            0
        };
        status.status_line = if is_receiving {
            format!("RECV: {chan_name}")
        } else {
            format!("HOLD: {chan_name}")
        };
    } else {
        status.system_name = if sys_name.is_empty() {
            "Scanning...".to_string()
        } else {
            sys_name
        };
        status.dept_name = dept_name;
        status.channel_name = "Scanning...".to_string();
        status.frequency = None;
        status.tgid = None;
        status.receiving = false;
        status.avoided = false;
        status.rssi = 0;
        status.status_line = "SCANNING".to_string();
    }

    status.last_update = now_unix_seconds();
}

/// Pure deterministic parser for a `HomePatrol` / SDS pipelined response (`MDL`, `PWR`, `GLG`, `GSI,1` XML, `STS`).
#[allow(clippy::too_many_lines)]
pub fn parse_hp_response(res: &str, status: &mut ScannerStatus) {
    if res.trim().is_empty() {
        return;
    }

    // 1. Model identification (MDL,)
    if let Some(mdl) = extract_section(res, "MDL,") {
        let trimmed = mdl.trim();
        if !trimmed.is_empty() {
            status.model = trimmed.to_string();
        }
    }

    // 2. Extract PWR block (PWR,rssi,freq)
    let mut freq_val: Option<f64> = None;
    let mut rssi_val: i32 = 0;
    if let Some(pwr_str) = extract_section(res, "PWR,") {
        let pwr_tokens: Vec<&str> = pwr_str.split(',').map(str::trim).collect();
        if pwr_tokens.len() >= 2 {
            if pwr_tokens[0] != "-999" {
                if let Ok(r) = pwr_tokens[0].parse::<i32>() {
                    rssi_val = r;
                }
            }
            if let Ok(raw_f) = pwr_tokens[1].parse::<f64>() {
                freq_val = normalize_frequency(Some(raw_f));
            }
        }
    }

    // 3. Check for GLG block
    let glg_opt = extract_section(res, "GLG,").or_else(|| {
        res.split_once("GLG,")
            .and_then(|(_, rest)| rest.split('\r').next())
            .map(|s| s.trim().to_string())
    });

    // 4. Check for GSI,1 XML payload (<ScannerInfo>...</ScannerInfo>)
    let xml_payload = res
        .split_once("<ScannerInfo>")
        .and_then(|(_, after)| after.split_once("</ScannerInfo>"))
        .map(|(inner, _)| format!("<ScannerInfo>{inner}</ScannerInfo>"));

    // If GLG or XML was received, perform structured parsing
    if glg_opt.is_some() || xml_payload.is_some() {
        let mut sys_name = String::new();
        let mut dept_name = String::new();
        let mut chan_name = String::new();
        let mut tgid_val: Option<String> = None;
        let mut is_receiving = false;

        if let Some(ref glg_str) = glg_opt {
            let glg: Vec<&str> = glg_str.split(',').map(str::trim).collect();
            let is_dma_model = DMA_MODEL_NAMES.contains(&status.model.as_str());

            // Polymorphic field at index 0 (Frequency or Talkgroup)
            let item0 = if glg.is_empty() {
                String::new()
            } else {
                glg[0].replace("ID:", "").trim().to_string()
            };

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

            // Hierarchy names in standard Uniden sequence
            if glg.len() > 4 && !glg[4].is_empty() {
                sys_name = glg[4].to_string();
            }
            if glg.len() > 5 && !glg[5].is_empty() {
                dept_name = glg[5].to_string();
            }
            if glg.len() > 6 && !glg[6].is_empty() {
                chan_name = glg[6].to_string();
            }

            // Activity flag
            if is_dma_model {
                is_receiving = glg.len() > 7 && glg[7] == "1";
            } else {
                if glg.len() >= 5 {
                    is_receiving = glg[glg.len() - 5] == "1";
                }
                if !is_receiving && xml_payload.is_none() && glg.len() > 7 && glg[7] == "1" {
                    is_receiving = true;
                }
            }
        }

        // Parse XML fields from GSI,1 using quick-xml
        if let Some(ref xml_str) = xml_payload {
            let mut reader = Reader::from_str(xml_str);
            reader.config_mut().trim_text(true);

            loop {
                match reader.read_event() {
                    Ok(Event::Empty(ref e) | Event::Start(ref e)) => {
                        let tag_name = e.name();
                        match tag_name.as_ref() {
                            b"Department" => {
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"Name" {
                                        if let Ok(val) = attr.unescape_value() {
                                            let trimmed = val.trim();
                                            if !trimmed.is_empty() {
                                                dept_name = trimmed.to_string();
                                            }
                                        }
                                    }
                                }
                            }
                            b"ConvFrequency" | b"TrunkFrequency" | b"TGID" => {
                                for attr in e.attributes().flatten() {
                                    if let Ok(val) = attr.unescape_value() {
                                        let trimmed = val.trim();
                                        match attr.key.as_ref() {
                                            b"Name" if !trimmed.is_empty() => {
                                                chan_name = trimmed.to_string();
                                            }
                                            b"TGID" if !trimmed.is_empty() => {
                                                tgid_val = Some(trimmed.to_string());
                                            }
                                            b"Freq" if !trimmed.is_empty() => {
                                                if let Ok(raw_f) = trimmed.parse::<f64>() {
                                                    freq_val = normalize_frequency(Some(raw_f));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            b"Property" => {
                                for attr in e.attributes().flatten() {
                                    if let Ok(val) = attr.unescape_value() {
                                        let trimmed = val.trim();
                                        match attr.key.as_ref() {
                                            b"P25Status" => {
                                                if trimmed.eq_ignore_ascii_case("enc") {
                                                    // Encrypted activity counts as receiving (drivers/bcd436hp.py:353)
                                                    is_receiving = true;
                                                }
                                            }
                                            b"Rssi" if trimmed != "-999" => {
                                                if let Ok(r) = trimmed.parse::<i32>() {
                                                    rssi_val = r;
                                                }
                                            }
                                            b"U_Id" if !trimmed.is_empty() => {
                                                status.unit_id = Some(trimmed.to_string());
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(err) => {
                        debug!(error = %err, "XML parse error in GSI block");
                        break;
                    }
                    _ => {}
                }
            }
        }

        if is_receiving && (!chan_name.is_empty() || freq_val.is_some() || tgid_val.is_some()) {
            if chan_name.is_empty() {
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
                sys_name
            };
            status.dept_name = dept_name;
            status.channel_name.clone_from(&chan_name);
            status.frequency = freq_val;
            status.tgid = tgid_val;
            status.receiving = true;
            status.rssi = if rssi_val > 0 { rssi_val } else { 4 };
            status.status_line = format!("RECV: {}", status.channel_name);
        } else {
            status.system_name = if sys_name.is_empty() {
                "Scanning...".to_string()
            } else {
                sys_name
            };
            status.dept_name = dept_name;
            status.channel_name = "Scanning...".to_string();
            status.frequency = None;
            status.tgid = None;
            status.receiving = false;
            status.avoided = false;
            status.rssi = 0;
            status.status_line = "SCANNING".to_string();
        }

        status.last_update = now_unix_seconds();
        return;
    }

    // 5. Fallback: Parse STS LCD screen text if GLG / XML were not returned
    if let Some(sts_str) = extract_section(res, "STS,") {
        parse_sts_fallback(&format!("STS,{sts_str}"), status);
    } else if res.starts_with("STS,") {
        parse_sts_fallback(res, status);
    }
}

/// Stateful `HomePatrol` / SDS scanner driver wrapping a [`Transport`].
pub struct HpDriver<T: Transport> {
    config: ScannerConfig,
    status: ScannerStatus,
    transport: T,
    last_connect_attempt: Option<Instant>,
    connect_retry_interval: Duration,
    logged_connection_error: bool,
}

impl<T: Transport> HpDriver<T> {
    /// Creates a new `HpDriver` with the given configuration and transport.
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

    /// Reconstructs an `HpDriver` from parts while preserving an open transport during family switching.
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

    /// Checks whether the driver should attempt to connect right now.
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
                    baud = self.config.baud_rate,
                    "HomePatrol/SDS scanner connected"
                );
                true
            }
            Err(e) => {
                if !self.logged_connection_error {
                    warn!(
                        scanner_id = %self.config.id,
                        port = %self.config.port,
                        error = %e,
                        "Could not connect HomePatrol/SDS scanner; will retry periodically"
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
                error!(scanner_id = %self.config.id, error = %e, "Error communicating with HomePatrol/SDS scanner");
                self.status.connected = false;
                String::new()
            }
        }
    }

    /// Queries the scanner for live status and updates `self.status`.
    pub fn poll(&mut self) -> &ScannerStatus {
        if !self.status.connected && !self.connect() {
            return &self.status;
        }

        let res = self.send_cmd(HP_POLL_COMMAND);
        if !res.is_empty() {
            parse_hp_response(&res, &mut self.status);
        }
        &self.status
    }

    /// Advances past the current transmission on BCD436HP/SDS without lockout or opening menus.
    ///
    /// Sends Rotary Knob Right (`KEY,>,P`), falling back to `KEY,>, P`, then Channel Hold
    /// toggle (`KEY,Z,P` x 2).
    /// **NEVER sends `KEY,S,P`** because `'S'` triggers System Quick Key navigation on 436HP/SDS.
    pub fn send_skip(&mut self) -> bool {
        info!(scanner_id = %self.config.id, "Sending KEY,>,P (Rotary Knob Step / Skip)");
        let mut res = self.send_cmd("KEY,>,P");
        if res.is_empty() || res.contains("ERR") || res.contains("NG") {
            res = self.send_cmd("KEY,>, P");
        }
        if res.is_empty() || res.contains("ERR") || res.contains("NG") {
            info!(
                scanner_id = %self.config.id,
                "Knob step failed; sending Channel Hold cycle (KEY,Z,P)"
            );
            let _ = self.send_cmd("KEY,Z,P");
            thread::sleep(Duration::from_millis(50));
            res = self.send_cmd("KEY,Z,P");
        }
        res.contains("OK") || !res.is_empty()
    }

    /// Sends `KEY,.,P` (No/Back) to dismiss any open Quick Key or setup menus.
    pub fn dismiss_menu(&mut self) -> bool {
        info!(scanner_id = %self.config.id, "Sending KEY,.,P to dismiss menus");
        let res = self.send_cmd("KEY,.,P");
        res.contains("OK") || !res.is_empty()
    }

    /// Sends temporary Avoid keypress (`KEY,V,P`, fallback `KEY,A,L`) on BCD436HP/SDS.
    pub fn temporary_lockout(&mut self, tgid_or_freq: &str) -> bool {
        info!(
            scanner_id = %self.config.id,
            target = %tgid_or_freq,
            "Sending KEY,V,P (Avoid button)"
        );
        let mut res = self.send_cmd("KEY,V,P");
        if res.is_empty() || res.contains("ERR") || res.contains("NG") {
            res = self.send_cmd("KEY,A,L");
        }
        self.status.avoided = true;
        res.contains("OK") || !res.is_empty()
    }

    /// Clears temporary Avoid on BCD436HP/SDS (`KEY,V,P`).
    pub fn release_lockout(&mut self, tgid_or_freq: &str) -> bool {
        info!(scanner_id = %self.config.id, target = %tgid_or_freq, "Releasing avoid");
        let _ = self.send_cmd("KEY,V,P");
        self.status.avoided = false;
        true
    }
}
