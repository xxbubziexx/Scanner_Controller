//! Desktop application shell (`sc-desktop`) replacing [`gui.py`](../../../../gui.py).
//!
//! Embeds [`sc-core`], [`sc-engine`], and [`sc-server`] in-process, enforces a single-instance
//! lock so two processes never contend for the same serial COM/TTY port (Prime Directive #4),
//! tracks background call-recording state, and renders the [`static/`](../../../../static/) UI
//! in a native app-mode `WebView2`/Chromium window.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::time::Duration;

use sc_core::AppConfig;
use sc_engine::EngineStateSnapshot;
use sc_server::run_hardware_server;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Errors returned by [`sc-desktop`].
#[derive(Debug, Error)]
pub enum DesktopError {
    /// Another instance of Scanner Controller is already holding the single-instance lock.
    #[error("Another Scanner Controller instance is already running (lock port {lock_port} busy)")]
    AlreadyRunning { lock_port: u16 },

    /// Underlying HTTP/WebSocket server or engine error.
    #[error("Server error: {0}")]
    Server(#[from] sc_server::ServerError),
}

/// Recording indicator state for window/tray status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingIndicator {
    /// Master call recording (`feeder.enable_feeder`) is disabled.
    Disabled,
    /// Master call recording is enabled and armed, waiting for squelch open.
    Standby,
    /// At least one scanner is actively capturing and recording a call transmission.
    Active,
}

impl RecordingIndicator {
    /// Human-readable status label matching `gui.py` and `static/`.
    #[must_use]
    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Disabled => "REC DISABLED",
            Self::Standby => "REC STANDBY",
            Self::Active => "REC ACTIVE",
        }
    }
}

/// Computes the current [`RecordingIndicator`] from an [`EngineStateSnapshot`].
#[must_use]
pub const fn compute_recording_indicator(state: &EngineStateSnapshot) -> RecordingIndicator {
    if !state.config.feeder.enable_feeder {
        return RecordingIndicator::Disabled;
    }
    let rec_a = state.config.scanner_a.enabled
        && state.config.scanner_a.audio_enabled
        && state.scanner_a.receiving
        && !state.scanner_a.avoided;
    let rec_b = state.config.scanner_b.enabled
        && state.config.scanner_b.audio_enabled
        && state.scanner_b.receiving
        && !state.scanner_b.avoided;

    if rec_a || rec_b {
        RecordingIndicator::Active
    } else {
        RecordingIndicator::Standby
    }
}

/// Exclusive single-instance process guard backed by a loopback TCP listener.
///
/// Prevents multiple desktop instances from opening the same scanner serial ports
/// simultaneously (Prime Directive #4).
#[derive(Debug)]
pub struct SingleInstanceGuard {
    _listener: TcpListener,
    lock_port: u16,
}

impl SingleInstanceGuard {
    /// Attempts to acquire the single-instance lock on `127.0.0.1:<lock_port>`.
    ///
    /// # Errors
    ///
    /// Returns [`DesktopError::AlreadyRunning`] if another instance already holds `lock_port`.
    pub fn try_acquire(lock_port: u16) -> Result<Self, DesktopError> {
        let addr = SocketAddr::from(([127, 0, 0, 1], lock_port));
        let listener =
            TcpListener::bind(addr).map_err(|_| DesktopError::AlreadyRunning { lock_port })?;
        Ok(Self {
            _listener: listener,
            lock_port,
        })
    }

    /// Returns the lock port held by this guard.
    #[must_use]
    pub const fn lock_port(&self) -> u16 {
        self.lock_port
    }
}

/// Configuration for the [`sc-desktop`] application shell.
#[derive(Debug, Clone)]
pub struct DesktopShellConfig {
    /// Local bind host (default `"127.0.0.1"`).
    pub host: String,
    /// Local HTTP/WebSocket port (default `8000`).
    pub port: u16,
    /// Single-instance lock port (default `45891`).
    pub lock_port: u16,
    /// Start minimized in background without popping up a foreground window.
    pub start_minimized: bool,
    /// Keep running in background when window is minimized/closed until explicit shutdown.
    pub minimize_to_tray: bool,
    /// Optional path to `config.json` for persistent settings.
    pub config_path: Option<PathBuf>,
}

impl Default for DesktopShellConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8000,
            lock_port: 45_891,
            start_minimized: false,
            minimize_to_tray: true,
            config_path: None,
        }
    }
}

/// Launches a chromeless native app-mode window (Edge `WebView2` / Chromium `--app=<url>`)
/// or falls back to the system default browser handler.
pub fn launch_native_app_window(url: &str) {
    #[cfg(target_os = "windows")]
    {
        // Try Microsoft Edge in native chromeless app mode first
        let edge_launched = std::process::Command::new("cmd")
            .args([
                "/C",
                "start",
                "",
                "msedge",
                &format!("--app={url}"),
                "--window-size=1460,940",
            ])
            .spawn()
            .is_ok();

        if !edge_launched {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn();
        }
    }
    #[cfg(target_os = "linux")]
    {
        let chromium_launched = std::process::Command::new("chromium-browser")
            .args([&format!("--app={url}"), "--window-size=1460,940"])
            .spawn()
            .is_ok()
            || std::process::Command::new("chromium")
                .args([&format!("--app={url}"), "--window-size=1460,940"])
                .spawn()
                .is_ok();

        if !chromium_launched {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// Runs the in-process desktop application (`sc-core` + `sc-engine` + `sc-server` + native app window).
///
/// If another instance is already running, focuses the existing UI window and returns `Ok(())`
/// without opening any hardware serial ports.
///
/// # Errors
///
/// Returns [`DesktopError`] if the embedded server fails to bind or encounters a fatal error.
pub async fn run_desktop_app(
    config: AppConfig,
    shell_cfg: DesktopShellConfig,
    shutdown_token: CancellationToken,
) -> Result<(), DesktopError> {
    let browser_host = if shell_cfg.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        shell_cfg.host.as_str()
    };
    let ui_url = format!("http://{browser_host}:{}", shell_cfg.port);

    let _instance_guard = match SingleInstanceGuard::try_acquire(shell_cfg.lock_port) {
        Ok(guard) => guard,
        Err(DesktopError::AlreadyRunning { lock_port }) => {
            tracing::warn!(
                lock_port,
                url = %ui_url,
                "Existing Scanner Controller instance detected; opening UI window without touching serial ports"
            );
            if !shell_cfg.start_minimized {
                launch_native_app_window(&ui_url);
            }
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let addr: SocketAddr = format!("{}:{}", shell_cfg.host, shell_cfg.port)
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], shell_cfg.port)));

    if shell_cfg.start_minimized {
        tracing::info!(
            url = %ui_url,
            minimize_to_tray = shell_cfg.minimize_to_tray,
            "Desktop app started minimized in background"
        );
    } else {
        let url_clone = ui_url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(650));
            launch_native_app_window(&url_clone);
        });
    }

    run_hardware_server(config, shell_cfg.config_path, addr, shutdown_token).await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::HashMap;

    use sc_core::{AppConfig, ScannerStatus};

    use super::*;

    #[test]
    fn test_single_instance_lock_prevents_second_owner() {
        // Bind to an ephemeral port first to find a free port, then test SingleInstanceGuard
        let probe = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
        let port = probe.local_addr().expect("local_addr").port();
        drop(probe);

        let guard1 = SingleInstanceGuard::try_acquire(port).expect("first instance should acquire");
        assert_eq!(guard1.lock_port(), port);

        let second = SingleInstanceGuard::try_acquire(port);
        assert!(matches!(
            second,
            Err(DesktopError::AlreadyRunning { lock_port }) if lock_port == port
        ));

        drop(guard1);
        let guard2 =
            SingleInstanceGuard::try_acquire(port).expect("re-acquire after drop should succeed");
        assert_eq!(guard2.lock_port(), port);
    }

    #[test]
    fn test_background_recording_indicator_logic() {
        let mut cfg = AppConfig::default();
        let mut st_a = ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP");
        let st_b = ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2");

        let mut snap = EngineStateSnapshot {
            scanner_a: st_a.clone(),
            scanner_b: st_b.clone(),
            config: cfg.clone(),
            active_exclusions: HashMap::new(),
            event_log: Vec::new(),
            audio_levels: HashMap::new(),
            audio_level: 0,
        };

        assert_eq!(
            compute_recording_indicator(&snap),
            RecordingIndicator::Standby
        );
        assert_eq!(compute_recording_indicator(&snap).as_label(), "REC STANDBY");

        st_a.receiving = true;
        snap.scanner_a = st_a.clone();
        assert_eq!(
            compute_recording_indicator(&snap),
            RecordingIndicator::Active
        );
        assert_eq!(compute_recording_indicator(&snap).as_label(), "REC ACTIVE");

        // If avoided by mutual exclusion, should not report active recording
        st_a.avoided = true;
        snap.scanner_a = st_a;
        assert_eq!(
            compute_recording_indicator(&snap),
            RecordingIndicator::Standby
        );

        // If master feeder is disabled, indicator is Disabled
        cfg.feeder.enable_feeder = false;
        snap.config = cfg;
        assert_eq!(
            compute_recording_indicator(&snap),
            RecordingIndicator::Disabled
        );
    }
}
