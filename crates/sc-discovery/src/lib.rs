//! `sc-discovery`: Serial COM/TTY port enumeration (`MDL`/`SI` probing) and UDP LAN scanner discovery (`SUS`).
//!
//! Reference: `COMPORT-AND-SCANNER-SELECTION.md` §3–5, `discovery/serial_detector.py`, `discovery/net_detector.py`.

#![deny(unsafe_code)]

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, Instant};

use sc_core::ScannerModel;
use serde::{Deserialize, Serialize};
use serialport::{SerialPortInfo, SerialPortType};
use tracing::{debug, warn};

/// Fixed candidate baud rate list probed by `ProScan` Auto-Detect (`FrmCommPS.cs:793`).
pub const CANDIDATE_BAUDS: [u32; 7] = [115_200, 57600, 38400, 19200, 9600, 4800, 2400];

/// Result of probing a single serial COM/TTY port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedPort {
    /// OS device name (e.g. `"COM3"` or `"/dev/ttyACM0"`).
    pub port: String,
    /// Extracted numeric port digits (e.g. `"3"`).
    pub port_digits: String,
    /// Availability status: `"Available"`, `"In Use By This Instance"`, `"In Use By Another Program"`, or `"Check Driver"`.
    pub status: String,
    /// Detected Uniden model string (e.g. `"BCD436HP"`, `"BCD996P2"`), or empty if none responded.
    pub model: String,
    /// Baud rate at which the scanner responded (defaults to `115200` if none responded).
    pub baud_rate: u32,
    /// Optional USB Vendor ID if the port is a USB-serial device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_vid: Option<u16>,
    /// Optional USB Product ID if the port is a USB-serial device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_pid: Option<u16>,
}

/// Result of a Uniden UDP LAN broadcast discovery probe (`SUS,UNIDEN,SCANNER\r` on port 50536).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredLanScanner {
    pub ip_address: String,
    pub model: String,
    pub name: String,
    pub raw_response: String,
}

/// Metadata for an OS-enumerated serial port returned by `GET /api/ports/available` (`server.py:156-188`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailablePortInfo {
    pub device: String,
    pub description: String,
    pub hwid: String,
    pub is_uniden: bool,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_model: Option<String>,
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn check_linux_tty_sysfs(
    base_name: &str,
    port_type: &SerialPortType,
    sysfs_root: &std::path::Path,
) -> bool {
    if matches!(
        port_type,
        SerialPortType::UsbPort(_) | SerialPortType::PciPort | SerialPortType::BluetoothPort
    ) {
        return false;
    }
    if !sysfs_root.exists() {
        return false;
    }
    let tty_dir = sysfs_root.join(base_name);
    if !tty_dir.exists() {
        return false;
    }
    if let Ok(tty_type) = std::fs::read_to_string(tty_dir.join("type")) {
        if tty_type.trim() == "0" {
            return true;
        }
    }
    !tty_dir.join("device").exists() && matches!(port_type, SerialPortType::Unknown)
}

fn is_phantom_linux_serial_port(p: &SerialPortInfo) -> bool {
    #[cfg(target_os = "linux")]
    {
        let base_name = std::path::Path::new(&p.port_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&p.port_name);
        check_linux_tty_sysfs(
            base_name,
            &p.port_type,
            std::path::Path::new("/sys/class/tty"),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = p;
        false
    }
}

/// Lists all serial ports currently enumerated by the OS without opening any port (`server.py:156-188`).
#[must_use]
pub fn list_available_ports() -> Vec<AvailablePortInfo> {
    let Ok(raw_ports) = serialport::available_ports() else {
        return Vec::new();
    };

    let mut keyed: Vec<(bool, bool, u32, AvailablePortInfo)> = Vec::new();
    for p in raw_ports {
        if is_phantom_linux_serial_port(&p) {
            continue;
        }
        let is_usb = matches!(p.port_type, SerialPortType::UsbPort(_));
        let (description, hwid, is_uniden) = match &p.port_type {
            SerialPortType::UsbPort(info) => {
                let prod = info.product.clone().unwrap_or_else(|| p.port_name.clone());
                let mfg = info.manufacturer.as_deref().unwrap_or("");
                let hw = format!("USB VID:PID={:04X}:{:04X}", info.vid, info.pid);
                let uniden = info.vid == 0x1965
                    || prod.to_ascii_lowercase().contains("uniden")
                    || mfg.to_ascii_lowercase().contains("uniden");
                (prod, hw, uniden)
            }
            _ => (p.port_name.clone(), String::new(), false),
        };
        let detected_model =
            ScannerModel::from_usb_description(&description).map(|m| m.as_str().to_string());
        let mut label = format!("{} - {}", p.port_name, description);
        if is_uniden {
            label.push_str(" (Uniden Radio Detected)");
        }
        let digits = extract_port_digits(&p.port_name);
        keyed.push((
            !is_uniden,
            !is_usb,
            digits,
            AvailablePortInfo {
                device: p.port_name,
                description,
                hwid,
                is_uniden,
                label,
                detected_model,
            },
        ));
    }
    keyed.sort_by_key(|a| (a.0, a.1, a.2));
    keyed.into_iter().map(|(_, _, _, info)| info).collect()
}

/// Parses a Uniden `MDL` command response (e.g. `"\rMDL,BCD436HP\r"` -> `Some("BCD436HP")`).
#[must_use]
pub fn parse_mdl_response(response: &str) -> Option<String> {
    let (_, after) = response.split_once("MDL,")?;
    let trimmed = after.trim();
    let candidate = trimmed
        .split([',', '\r', '\n'])
        .next()
        .map_or("", str::trim);
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

/// Parses a legacy Uniden `SI` command response (e.g. `"\rSI BC346XT,0,0,0\r"` -> `Some("BC346XT")`).
#[must_use]
pub fn parse_si_response(response: &str) -> Option<String> {
    let (_, after) = response.split_once("SI ")?;
    let trimmed = after.trim();
    let candidate = trimmed
        .split([',', '\r', '\n'])
        .next()
        .map_or("", str::trim);
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

/// Extracts numeric digits from a COM/TTY port string for numerical sorting (`"COM10"` -> `10`).
/// Returns `9999` if no digits are present.
#[must_use]
pub fn extract_port_digits(port_name: &str) -> u32 {
    let digits: String = port_name.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        9999
    } else {
        digits.parse::<u32>().unwrap_or(9999)
    }
}

/// Parses a Uniden UDP broadcast response (`SUS,UNIDEN,SCANNER,[Model],[Name]...`).
#[must_use]
pub fn parse_sus_response(data: &str, sender_ip: &str) -> Option<DiscoveredLanScanner> {
    let cleaned = data.trim();
    if !cleaned.starts_with("SUS,") {
        return None;
    }

    let parts: Vec<&str> = cleaned.split(',').collect();
    if parts.len() >= 5 {
        let model = parts[3].trim();
        let name = parts[4].trim();
        Some(DiscoveredLanScanner {
            ip_address: sender_ip.to_string(),
            model: if model.is_empty() {
                "Uniden IP Scanner".to_string()
            } else {
                model.to_string()
            },
            name: if name.is_empty() {
                "Network Scanner".to_string()
            } else {
                name.to_string()
            },
            raw_response: cleaned.to_string(),
        })
    } else if parts.len() >= 3 {
        let model = parts[1].trim();
        let name = parts[2].trim();
        Some(DiscoveredLanScanner {
            ip_address: sender_ip.to_string(),
            model: model.to_string(),
            name: if name.is_empty() {
                "Network Scanner".to_string()
            } else {
                name.to_string()
            },
            raw_response: cleaned.to_string(),
        })
    } else {
        None
    }
}

fn extract_usb_vid_pid(port_type: &SerialPortType) -> (Option<u16>, Option<u16>) {
    match port_type {
        SerialPortType::UsbPort(info) => (Some(info.vid), Some(info.pid)),
        _ => (None, None),
    }
}

/// Enumerates and probes system serial ports at standard Uniden baud rates (`\rMDL\r` and `\rSI\r`).
///
/// Honors Prime Directive 4: any port listed in `active_ports` (matched by full name or bare digits)
/// is immediately marked `"In Use By This Instance"` and is **never** opened.
#[must_use]
#[allow(clippy::too_many_lines, clippy::similar_names)]
pub fn detect_serial_ports(active_ports: &[String]) -> Vec<DetectedPort> {
    let Ok(raw_ports) = serialport::available_ports() else {
        warn!("Failed to enumerate system serial ports");
        return Vec::new();
    };

    // Sort ports numerically by port digits (COM1, COM2, COM9, COM10), excluding phantom Linux ttyS slots
    let mut raw_ports: Vec<SerialPortInfo> = raw_ports
        .into_iter()
        .filter(|p| !is_phantom_linux_serial_port(p))
        .collect();
    raw_ports.sort_by_key(|p| extract_port_digits(&p.port_name));

    let active_canonical: Vec<std::path::PathBuf> = active_ports
        .iter()
        .filter_map(|ap| std::fs::canonicalize(ap.trim()).ok())
        .collect();

    // Deduplicate ports by normalized digits/name (Common_PS_RF.cs:511)
    let mut seen_keys = HashSet::new();
    let unique_ports: Vec<SerialPortInfo> = raw_ports
        .into_iter()
        .filter(|p| {
            let digits = extract_port_digits(&p.port_name);
            let key = if digits == 9999 {
                p.port_name.clone()
            } else {
                digits.to_string()
            };
            seen_keys.insert(key)
        })
        .collect();

    let mut results = Vec::with_capacity(unique_ports.len());

    for port_info in unique_ports {
        let port_name = port_info.port_name;
        let digits_num = extract_port_digits(&port_name);
        let digits_str = digits_num.to_string();
        let (usb_vid, usb_pid) = extract_usb_vid_pid(&port_info.port_type);

        // Prime Directive 4: never open a port currently held by this instance (including via symlink)
        let port_canon = std::fs::canonicalize(&port_name).ok();
        let is_active = active_ports.iter().any(|ap| {
            let ap_trim = ap.trim();
            ap_trim.eq_ignore_ascii_case(&port_name) || ap_trim == digits_str
        }) || port_canon
            .as_ref()
            .is_some_and(|pc| active_canonical.contains(pc));

        if is_active {
            results.push(DetectedPort {
                port: port_name,
                port_digits: digits_str,
                status: "In Use By This Instance".to_string(),
                model: String::new(),
                baud_rate: 115_200,
                usb_vid,
                usb_pid,
            });
            continue;
        }

        let mut status = String::new();
        let mut found_model = String::new();
        let mut found_baud: u32 = 0;

        for (idx, &baud) in CANDIDATE_BAUDS.iter().enumerate() {
            match serialport::new(&port_name, baud)
                .timeout(Duration::from_millis(200))
                .open()
            {
                Ok(mut ser) => {
                    let _ = ser.write_request_to_send(true);
                    let _ = ser.write_data_terminal_ready(true);

                    if idx == 0 {
                        status = "Available".to_string();
                    }

                    // Probe 1: \rMDL\r
                    let _ = ser.write_all(b"\rMDL\r");
                    thread::sleep(Duration::from_millis(50));
                    let mut buf = [0u8; 256];
                    let n = ser.read(&mut buf).unwrap_or(0);
                    let res_mdl = String::from_utf8_lossy(&buf[..n]);

                    let mut model_opt = parse_mdl_response(&res_mdl);
                    if model_opt.is_none() {
                        // Probe 2 (fallback): \rSI\r
                        let _ = ser.write_all(b"\rSI\r");
                        thread::sleep(Duration::from_millis(50));
                        let n_si = ser.read(&mut buf).unwrap_or(0);
                        let res_si = String::from_utf8_lossy(&buf[..n_si]);
                        model_opt = parse_si_response(&res_si);
                    }

                    drop(ser);

                    if let Some(m) = model_opt {
                        found_model =
                            ScannerModel::from_mdl_str(&m).map_or(m, |sm| sm.as_str().to_string());
                        found_baud = baud;
                        break;
                    }
                }
                Err(e) => {
                    let err_lower = e.to_string().to_lowercase();
                    if idx == 0
                        && (err_lower.contains("access")
                            || err_lower.contains("denied")
                            || err_lower.contains("permission")
                            || err_lower.contains("busy"))
                    {
                        status = "In Use By Another Program".to_string();
                        break;
                    }
                }
            }
        }

        if status.is_empty() {
            status = "Check Driver".to_string();
        }

        results.push(DetectedPort {
            port: port_name,
            port_digits: digits_str,
            status,
            model: found_model,
            baud_rate: if found_baud > 0 { found_baud } else { 115_200 },
            usb_vid,
            usb_pid,
        });
    }

    results
}

/// Broadcasts the Uniden discovery payload (`SUS,UNIDEN,SCANNER\r`) over UDP port 50536
/// and collects responses from network-attached scanners (e.g. BCD536HP, SDS200).
#[must_use]
pub fn discover_lan_scanners(timeout: Duration) -> Vec<DiscoveredLanScanner> {
    let mut scanners = Vec::new();
    let mut seen_ips = HashSet::new();

    let sock = UdpSocket::bind("0.0.0.0:50536").or_else(|_| UdpSocket::bind("0.0.0.0:0"));

    let Ok(sock) = sock else {
        warn!("Failed to bind UDP socket for LAN scanner discovery");
        return scanners;
    };

    if let Err(e) = sock.set_broadcast(true) {
        warn!(error = %e, "Failed to enable SO_BROADCAST on UDP discovery socket");
        return scanners;
    }

    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));

    if let Err(e) = sock.send_to(b"SUS,UNIDEN,SCANNER\r", "255.255.255.255:50536") {
        debug!(error = %e, "UDP broadcast send_to returned error");
        return scanners;
    }

    let start = Instant::now();
    let mut buf = [0u8; 1024];
    while start.elapsed() < timeout {
        match sock.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let ip_str = addr.ip().to_string();
                if !seen_ips.contains(&ip_str) {
                    let resp_str = String::from_utf8_lossy(&buf[..n]);
                    if let Some(parsed) = parse_sus_response(&resp_str, &ip_str) {
                        seen_ips.insert(ip_str);
                        scanners.push(parsed);
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if start.elapsed() >= timeout {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    scanners
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use sc_core::ScannerModel;

    #[test]
    fn test_parse_mdl_response() {
        assert_eq!(
            parse_mdl_response("\rMDL,BCD436HP\r").as_deref(),
            Some("BCD436HP")
        );
        assert_eq!(
            parse_mdl_response("MDL,SDS100,V1.00\r").as_deref(),
            Some("SDS100")
        );
        assert_eq!(parse_mdl_response("ERR\r"), None);
    }

    #[test]
    fn test_parse_si_response() {
        assert_eq!(
            parse_si_response("SI BC346XT,0,0,0\r").as_deref(),
            Some("BC346XT")
        );
        assert_eq!(parse_si_response("INVALID\r"), None);
    }

    #[test]
    fn test_extract_port_digits() {
        assert_eq!(extract_port_digits("COM1"), 1);
        assert_eq!(extract_port_digits("COM3"), 3);
        assert_eq!(extract_port_digits("COM10"), 10);
        assert_eq!(extract_port_digits("COM25"), 25);
        assert_eq!(extract_port_digits("/dev/ttyUSB0"), 0);
        assert_eq!(extract_port_digits("NONE"), 9999);
    }

    #[test]
    fn test_model_baud_constraints() {
        assert_eq!(ScannerModel::BC125AT.allowed_baud_rates(), &[115_200]);
        assert_eq!(ScannerModel::BCD160DN.allowed_baud_rates(), &[115_200]);
        assert_eq!(
            ScannerModel::BC780XLT.allowed_baud_rates(),
            &[19200, 9600, 4800, 2400]
        );
        assert!(ScannerModel::BCD436HP.is_baud_rate_allowed(115_200));
        assert!(ScannerModel::BCD996P2.is_baud_rate_allowed(57600));
    }

    #[test]
    fn test_parse_sus_response() {
        let raw = "SUS,UNIDEN,SCANNER,BCD536HP,Main Tower Scanner\r";
        let parsed = parse_sus_response(raw, "192.168.1.50").expect("should parse SUS");
        assert_eq!(parsed.ip_address, "192.168.1.50");
        assert_eq!(parsed.model, "BCD536HP");
        assert_eq!(parsed.name, "Main Tower Scanner");
    }

    #[test]
    fn test_phantom_linux_tty_filtering() {
        let temp_sysfs = std::env::temp_dir().join(format!(
            "sc_tty_sysfs_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let tty_s0 = temp_sysfs.join("ttyS0");
        let tty_s1_real = temp_sysfs.join("ttyS1");
        let tty_acm1 = temp_sysfs.join("ttyACM1");
        std::fs::create_dir_all(&tty_s0).unwrap();
        std::fs::create_dir_all(tty_s1_real.join("device")).unwrap();
        std::fs::create_dir_all(&tty_acm1).unwrap();
        std::fs::write(tty_s0.join("type"), "0\n").unwrap();
        std::fs::write(tty_s1_real.join("type"), "4\n").unwrap();

        // Unknown port with type=0 -> phantom
        assert!(check_linux_tty_sysfs(
            "ttyS0",
            &SerialPortType::Unknown,
            &temp_sysfs
        ));
        // Unknown port with type=4 and device/ -> real UART
        assert!(!check_linux_tty_sysfs(
            "ttyS1",
            &SerialPortType::Unknown,
            &temp_sysfs
        ));
        // USB port -> never filtered even if sysfs has no type
        let usb = SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x1965,
            pid: 0x001a,
            serial_number: None,
            manufacturer: Some("UNIDEN AMERICA CORP.".to_string()),
            product: Some("BCD436HP Serial Port".to_string()),
        });
        assert!(!check_linux_tty_sysfs("ttyACM1", &usb, &temp_sysfs));
        assert_eq!(
            ScannerModel::from_usb_description("BCD436HP Serial Port")
                .map(|m| m.as_str().to_string()),
            Some("BCD436HP".to_string())
        );

        let _ = std::fs::remove_dir_all(&temp_sysfs);
    }
}
