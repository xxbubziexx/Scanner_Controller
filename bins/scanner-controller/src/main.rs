//! `scanner-controller` CLI and headless/web server binary.
//!
//! Subcommands:
//! - `scanner-controller` / `scanner-controller serve`: Starts the Mutual Exclusion Engine + Web Dashboard (`http://127.0.0.1:8000`).
//! - `scanner-controller detect`: Probes serial COM/TTY ports and optional LAN UDP scanners.
//! - `scanner-controller probe-keypad`: Tests raw `KEY` commands against a connected Uniden scanner.

#![deny(unsafe_code)]

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sc_core::AppConfig;
use sc_desktop::{run_desktop_app, DesktopShellConfig, SingleInstanceGuard};
use sc_discovery::{detect_serial_ports, discover_lan_scanners};
use sc_drivers::normalize_port_name;
use sc_server::run_hardware_server;
use serialport::ClearBuffer;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(
    name = "scanner-controller",
    version,
    about = "Mutual-exclusion & ProScan-compatible recording controller for dual Uniden scanners"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Launch the Desktop Application Shell (single-instance lock + embedded engine + native app window).
    Desktop {
        /// Host address to bind (default 127.0.0.1).
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// TCP port for the embedded UI and REST/WebSocket API.
        #[arg(long, default_value_t = 8000)]
        port: u16,

        /// Path to config.json (defaults to ./config.json or ../config.json).
        #[arg(long)]
        config: Option<PathBuf>,

        /// Start minimized in the background without opening a foreground window.
        #[arg(long)]
        start_minimized: bool,
    },

    /// Start the headless/web Scanner Sync Engine and Web Control Dashboard.
    Serve {
        /// Host address to bind (e.g. 127.0.0.1 or 0.0.0.0).
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// TCP port for the web dashboard and REST/WebSocket API.
        #[arg(long, default_value_t = 8000)]
        port: u16,

        /// Path to config.json (defaults to ./config.json or ../config.json).
        #[arg(long)]
        config: Option<PathBuf>,

        /// Do not automatically open the web browser on startup.
        #[arg(long)]
        no_browser: bool,

        /// Single-instance loopback lock port (prevents duplicate processes contending for hardware).
        #[arg(long, default_value_t = 45_891)]
        lock_port: u16,

        /// Disable the single-instance lock guard.
        #[arg(long)]
        no_lock: bool,
    },

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

fn resolve_config_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    let cwd_cfg = PathBuf::from("config.json");
    if cwd_cfg.exists() {
        return cwd_cfg;
    }
    let parent_cfg = Path::new("..").join("config.json");
    if parent_cfg.exists() {
        return parent_cfg;
    }
    cwd_cfg
}

fn try_open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

fn spawn_shutdown_watcher(shutdown_token: CancellationToken) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut sigterm) => {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            tracing::info!("SIGINT/Ctrl+C received; initiating graceful shutdown...");
                        }
                        _ = sigterm.recv() => {
                            tracing::info!("SIGTERM received; initiating graceful shutdown...");
                        }
                    }
                }
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    tracing::info!("Ctrl+C received; initiating graceful shutdown...");
                }
            }
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_ok() {
                tracing::info!("Ctrl+C received; initiating graceful shutdown...");
            }
        }
        shutdown_token.cancel();
    });
}

#[allow(clippy::print_stdout)]
async fn run_serve(
    host: &str,
    port: u16,
    explicit_cfg: Option<PathBuf>,
    no_browser: bool,
    lock_port: u16,
    no_lock: bool,
) -> Result<()> {
    let _instance_guard = if no_lock {
        None
    } else {
        Some(SingleInstanceGuard::try_acquire(lock_port).with_context(|| {
            format!("Another scanner-controller instance is already running (lock port {lock_port} busy)")
        })?)
    };

    let cfg_path = resolve_config_path(explicit_cfg);
    let config = if cfg_path.exists() {
        tracing::info!(path = %cfg_path.display(), "Loading configuration from disk");
        AppConfig::load_from_file(&cfg_path).unwrap_or_else(|err| {
            tracing::warn!(error = %err, "Failed to parse config.json; using defaults");
            AppConfig::default()
        })
    } else {
        tracing::info!("No config.json found; starting with default configuration");
        AppConfig::default()
    };

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("Invalid bind address {host}:{port}"))?;

    let shutdown_token = CancellationToken::new();
    spawn_shutdown_watcher(shutdown_token.clone());

    if !no_browser {
        let browser_host = if host == "0.0.0.0" { "127.0.0.1" } else { host };
        let url = format!("http://{browser_host}:{port}");
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(800));
            try_open_browser(&url);
        });
    }

    run_hardware_server(config, Some(cfg_path), addr, shutdown_token).await?;
    Ok(())
}

async fn run_desktop(
    host: &str,
    port: u16,
    explicit_cfg: Option<PathBuf>,
    start_minimized: bool,
) -> Result<()> {
    let cfg_path = resolve_config_path(explicit_cfg);
    let config = if cfg_path.exists() {
        tracing::info!(path = %cfg_path.display(), "Loading configuration from disk");
        AppConfig::load_from_file(&cfg_path).unwrap_or_else(|err| {
            tracing::warn!(error = %err, "Failed to parse config.json; using defaults");
            AppConfig::default()
        })
    } else {
        tracing::info!("No config.json found; starting with default configuration");
        AppConfig::default()
    };

    let shutdown_token = CancellationToken::new();
    spawn_shutdown_watcher(shutdown_token.clone());

    let shell_cfg = DesktopShellConfig {
        host: host.to_string(),
        port,
        start_minimized,
        config_path: Some(cfg_path),
        ..DesktopShellConfig::default()
    };

    run_desktop_app(config, shell_cfg, shutdown_token).await?;
    Ok(())
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

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("JACK_NO_START_SERVER").is_none() {
        std::env::set_var("JACK_NO_START_SERVER", "1");
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        None => run_desktop("127.0.0.1", 8000, None, false).await,
        Some(Commands::Desktop {
            host,
            port,
            config,
            start_minimized,
        }) => run_desktop(&host, port, config, start_minimized).await,
        Some(Commands::Serve {
            host,
            port,
            config,
            no_browser,
            lock_port,
            no_lock,
        }) => run_serve(&host, port, config, no_browser, lock_port, no_lock).await,
        Some(Commands::Detect {
            lan,
            active_ports,
            json,
        }) => run_detect(lan, &active_ports, json),
        Some(Commands::ProbeKeypad { port, baud, keys }) => run_probe_keypad(&port, baud, &keys),
    }
}
