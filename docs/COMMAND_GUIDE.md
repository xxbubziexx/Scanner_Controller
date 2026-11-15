# Scanner Controller — Complete Command & CLI Reference Guide

Welcome to the command guide for **Scanner_Controller** (Rust single-binary edition).

The compiled binary `scanner-controller` (`scanner-controller.exe` on Windows) is an all-in-one executable that serves as the mutual-exclusion controller, audio capture engine, ProScan-compatible tagger, ScanScribe pipeline feeder, and web control dashboard.

---

## 1. Quick Launch Shortcuts

| Platform | Command / Shortcut | Description |
|---|---|---|
| **Windows** | Double-click `run_rust_app.bat` or run `.\run_rust_app.bat` | Launches release binary in Desktop App mode |
| **Windows** | `.\dist\scanner-controller.exe` | Direct execution with default settings (`http://127.0.0.1:8000`) |
| **Linux / Pi** | `scanner-controller serve --host 0.0.0.0 --no-browser` | Headless daemon mode for background service |
| **Linux / Pi** | `sudo systemctl restart scanscribe-scanner` | Systemd service management |

---

## 2. CLI Subcommands Overview

The binary provides 4 main subcommands:

```
scanner-controller [SUBCOMMAND] [OPTIONS]
```

```
SUBCOMMANDS:
    desktop        Launch in Desktop App mode with single-instance protection (Default if no args given)
    serve          Start headless / web server mode without native app window
    detect         Probe serial COM/TTY ports and discover LAN network scanners
    probe-keypad   Interactive keypad testing and STS display snapshot inspection
    help           Print help information or help for a specific subcommand
```

---

## 3. Subcommand Details & Options

### `scanner-controller desktop` (Default Mode)
Runs the in-process controller, enforces a single-instance guard on port `45891` (so two instances never fight over the same serial COM port), and opens a chromeless native app window to the UI.

```bat
# Default launch (binds 127.0.0.1:8000, loads config.json, opens window)
scanner-controller.exe

# Custom port or config file
scanner-controller.exe desktop --port 8080 --config C:\path\to\config.json

# Start minimized in background (daemon-like, no window popup)
scanner-controller.exe desktop --start-minimized
```

**Options:**
- `--host <IP>`: Local address to bind (default: `127.0.0.1`).
- `--port <PORT>`: Web dashboard port (default: `8000`).
- `--config <PATH>`: Explicit path to `config.json` (defaults to `./config.json` or `../config.json`).
- `--start-minimized`: Run in background without popping open a foreground window.

---

### `scanner-controller serve` (Headless Daemon Mode)
Ideal for servers, Raspberry Pi nodes, Docker containers, and `systemd` background services where no graphical window is needed.

```bash
# Run headless on all interfaces (accessible from other computers on the LAN)
./scanner-controller serve --host 0.0.0.0 --port 8000 --no-browser

# Run with explicit config path
./scanner-controller serve --config /opt/scanner_controller/config.json --no-browser
```

**Options:**
- `--host <IP>`: Interface address to bind (`127.0.0.1` for local-only, `0.0.0.0` for LAN access).
- `--port <PORT>`: Web dashboard port (default: `8000`).
- `--config <PATH>`: Path to `config.json`.
- `--no-browser`: Do not attempt to spawn a web browser on startup.

---

### `scanner-controller detect` (Hardware & Port Prober)
Scans system COM/TTY ports at baud rates from 115,200 down to 2,400 baud sending `\rMDL\r` and `\rSI\r` queries to detect connected Uniden models. Also supports network UDP broadcast discovery.

```bat
# Scan all serial COM ports for connected Uniden scanners
scanner-controller.exe detect

# Scan serial ports AND broadcast UDP discovery on port 50536 for LAN scanners (SDS200, BCD536HP)
scanner-controller.exe detect --lan

# Skip probing ports that are already in active use by other software
scanner-controller.exe detect --active-ports COM3,COM4

# Output detection results in machine-readable JSON format
scanner-controller.exe detect --lan --json
```

**Options:**
- `--lan`: Broadcast UDP discovery packets on port `50536` for network-attached scanners.
- `--active-ports <PORTS>`: Comma-separated list of ports to skip (e.g. `COM1,COM5`).
- `--json`: Format output as JSON (for scripting or integration).

---

### `scanner-controller probe-keypad` (Keypad & Protocol Diagnostics)
Replacement for legacy diagnostic script `tools/probe_keypad.py`. Tests raw Uniden `KEY` serial commands and reports display changes from the `STS` response.

```bat
# Probe default keys on COM5 at 115200 baud
scanner-controller.exe probe-keypad --port COM5 --baud 115200

# Test specific keys (e.g. Avoid/Skip, Hold, Menu)
scanner-controller.exe probe-keypad --port COM5 --baud 115200 --keys "KEY,>,P" "KEY,H,P" "KEY,M,P"
```

**Options:**
- `--port <PORT>`: Serial port name or number (e.g. `COM5`, `COM6`, `/dev/ttyACM0`).
- `--baud <BAUD>`: Serial baud rate (e.g. `115200`, `57600`, `19200`).
- `--keys <KEYS...>`: One or more `KEY` command strings to send.

---

## 4. Environment Variables

You can configure or override settings using environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `RUST_LOG` | `info` | Logging verbosity: `error`, `warn`, `info`, `debug`, `trace`. Example: `RUST_LOG=sc_drivers=debug,info` |
| `SCANSCRIBE_INBOX_DIR` | `A:\Scanscribe_Projects\scanscribe\inbox` (Win)<br>`/var/scanscribe/inbox` (Linux) | Directory where recorded audio and `.json` sidecars are dropped |
| `SCANNER_A_PORT` | `COM5` (Win)<br>`/dev/ttyACM0` (Linux) | Fallback port for Scanner A if not set in `config.json` |
| `SCANNER_B_PORT` | `NONE` | Fallback port for Scanner B if not set in `config.json` |
| `SCANNER_CONFIG_FILE` | `./config.json` | Explicit path to `config.json` file |

---

## 5. Linux Service & Deployment Commands

The Linux installer [`install.sh`](install.sh) supports the following production management commands:

```bash
# Automated system deployment (installs Rust binary, groups, inbox, systemd service)
sudo ./install.sh --system --start

# Deploy to custom directory and user
sudo ./install.sh --system --dir /opt/scanner --user pi --inbox /mnt/recordings/inbox --start

# Check live service status and journal logs
sudo systemctl status scanscribe-scanner
sudo journalctl -u scanscribe-scanner -f

# Instant 1-command rollback to Python reference implementation (if ever needed)
sudo ./install.sh --system --rollback-python --start
```

---

## 6. Developer & Build Helper Tasks (`xtask`)

From the `rust/` directory, you can run automated tasks using `cargo xtask`:

```bat
# Run full CI quality verification (cargo fmt + cargo clippy -D warnings + cargo test)
cargo run -p xtask -- ci

# Compile release binary and package it into root dist/ directory
cargo run -p xtask -- package

# Build release binary for a specific target (e.g. Raspberry Pi aarch64)
cargo run -p xtask -- build-release --target aarch64-unknown-linux-gnu
```

