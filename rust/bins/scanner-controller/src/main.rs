//! `scanner-controller` CLI binary.
//!
//! Phase 2 subcommands:
//! - `scanner-controller detect`: Probes serial COM/TTY ports and optional LAN UDP scanners.
//! - `scanner-controller probe-keypad`: Tests raw `KEY` commands against a connected Uniden scanner
//!   (direct Rust port of `tools/probe_keypad.py`).

#![deny(unsafe_code)]

use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sc_discovery::{detect_serial_ports, discover_lan_scanners};
use sc_drivers::normalize_port_name;
use serialport::ClearBuffer;

#[derive(Debug, Parser)]
#[command(
    name = "scanner-controller",
    version,
    about = "Mutual-exclusion & ProScan-compatible recording controller for dual Uniden scanners"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Probe serial COM/TTY ports (and optionally UDP LAN) for connected Uniden scanners.
    Detect {
        /// Also broadcast UDP discovery on port 50536 for network-attached scanners (SDS200, BCD536HP).
        #[arg(long)]
        lan: bool,

        /// Comma-separated list of active ports to skip probing (Prime Directive 4).
        #[arg(long, value_delimiter = ',')]
        active_ports: Vec<String>,

        /// Output results as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Test raw keypad commands against a connected scanner and inspect STS display changes
    /// (Rust port of `tools/probe_keypad.py`).
    ProbeKeypad {
        /// Serial COM/TTY port (e.g. COM5, COM6, or bare digits 5, 6).
        #[arg(long, default_value = "COM6")]
        port: String,

        /// Serial baud rate (e.g. 19200, 57600, 115200).
        #[arg(long, default_value_t = 19200)]
        baud: u32,

        /// List of KEY commands to test.
        #[arg(
            long,
            num_args = 1..,
            default_values_t = vec![
                "KEY,>,P".to_string(),
                "KEY,<,P".to_string(),
                "KEY,^,P".to_string(),
                "KEY,V,P".to_string(),
                "KEY,V,L".to_string(),
                "KEY,A,L".to_string(),
                "KEY,B,L".to_string(),
                "KEY,C,L".to_string(),
                "KEY,S,P".to_string(),
                "KEY,H,P".to_string(),
                "KEY,M,P".to_string(),
            ]
        )]
        keys: Vec<String>,
    },
}

#[allow(clippy::print_stdout)]
fn run_detect(lan: bool, active_ports: &[String], as_json: bool) -> Result<()> {
    let serial_ports = detect_serial_ports(active_ports);
    let lan_scanners = if lan {
        discover_lan_scanners(Duration::from_secs(2))
    } else {
        Vec::new()
    };

    if as_json {
        let payload = serde_json::json!({
            "serial_ports": serial_ports,
            "lan_scanners": lan_scanners,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "{:<12} | {:<28} | {:<14} | {:<10}",
        "Port", "Status", "Scanner", "Baud Rate"
    );
    println!("{}", "-".repeat(72));
    if serial_ports.is_empty() {
        println!("(No serial ports detected)");
    } else {
        for p in &serial_ports {
            println!(
                "{:<12} | {:<28} | {:<14} | {:<10}",
                p.port,
                p.status,
                if p.model.is_empty() { "-" } else { &p.model },
                p.baud_rate
            );
        }
    }

    if lan {
        println!();
        println!("{:<18} | {:<14} | {:<24}", "IP Address", "Model", "Name");
        println!("{}", "-".repeat(62));
        if lan_scanners.is_empty() {
            println!("(No LAN scanners responded on UDP 50536)");
        } else {
            for s in &lan_scanners {
                println!("{:<18} | {:<14} | {:<24}", s.ip_address, s.model, s.name);
            }
        }
    }

    Ok(())
}

#[allow(clippy::print_stdout)]
fn run_probe_keypad(raw_port: &str, baud: u32, keys: &[String]) -> Result<()> {
    let port_name = normalize_port_name(raw_port);
    println!("Opening {port_name} at {baud} baud...");

    let mut ser = serialport::new(&port_name, baud)
        .timeout(Duration::from_millis(500))
        .open()
        .with_context(|| format!("Error opening {port_name}"))?;

    let _ = ser.write_request_to_send(true);
    let _ = ser.write_data_terminal_ready(true);
    thread::sleep(Duration::from_millis(500));
    let _ = ser.clear(ClearBuffer::Input);

    let send_cmd = |ser: &mut Box<dyn serialport::SerialPort>, cmd: &str| -> String {
        let _ = ser.clear(ClearBuffer::Input);
        let payload = format!("{cmd}\r");
        let _ = ser.write_all(payload.as_bytes());
        thread::sleep(Duration::from_millis(80));
        let mut buf = [0u8; 2048];
        let n = ser.read(&mut buf).unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).trim().to_string()
    };

    let mdl = send_cmd(&mut ser, "MDL");
    println!("Scanner Model Response: {mdl:?}");
    println!("{}", "-".repeat(65));
    println!(
        "{:<18} | {:<15} | STS Snapshot / Notes",
        "Command Sent", "Immediate Reply"
    );
    println!("{}", "-".repeat(65));

    for key in keys {
        let pre_sts = send_cmd(&mut ser, "STS");
        let key_reply = send_cmd(&mut ser, key);
        thread::sleep(Duration::from_millis(100));
        let post_sts = send_cmd(&mut ser, "STS");

        let diff = if pre_sts == post_sts {
            "No change".to_string()
        } else {
            format!(
                "Display state changed! Length: {} -> {}",
                pre_sts.len(),
                post_sts.len()
            )
        };

        println!("{:<18} | {:<15} | {}", key, format!("{key_reply:?}"), diff);
        thread::sleep(Duration::from_millis(300));
    }

    println!("{}", "-".repeat(65));
    println!("Probe complete.");
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Detect {
            lan,
            active_ports,
            json,
        } => run_detect(lan, &active_ports, json),
        Commands::ProbeKeypad { port, baud, keys } => run_probe_keypad(&port, baud, &keys),
    }
}
