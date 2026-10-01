#!/usr/bin/env bash
# ==============================================================================
# Uniden Scanner Mutual Exclusion & Recording Controller
# Automated Production Installer for Linux (Debian, Ubuntu, Raspberry Pi OS, etc.)
# ==============================================================================
set -euo pipefail

# Text styling
BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Default parameters
DEFAULT_INSTALL_DIR="/opt/scanner_controller"
DEFAULT_INBOX_DIR="/var/scanscribe/inbox"
DEFAULT_PORT=8000

INSTALL_MODE="auto" # "system" or "local" or "auto"
TARGET_DIR=""
TARGET_USER=""
INSTALL_SERVICE=""
START_SERVICE=0
PORT="$DEFAULT_PORT"
INBOX_DIR="$DEFAULT_INBOX_DIR"
SKIP_SYS_DEPS=0
NON_INTERACTIVE=0

SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

# Banner
print_banner() {
    cat << "EOF"
==============================================================================
   ____                                   ____            _             _ _           
  / ___|  ___ __ _ _ __  _ __   ___ _ __ / ___|___  _ __ | |_ _ __ ___ | | | ___ _ __ 
  \___ \ / __/ _` | '_ \| '_ \ / _ \ '__| |   / _ \| '_ \| __| '__/ _ \| | |/ _ \ '__|
   ___) | (_| (_| | | | | | | |  __/ |  | |__| (_) | | | | |_| | | (_) | | |  __/ |   
  |____/ \___\__,_|_| |_|_| |_|\___|_|   \____\___/|_| |_|\__|_|  \___/|_|_|\___|_|   
                                                                                      
             Automated Linux Fast Deployment & Systemd Installer
==============================================================================
EOF
}

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --system              Deploy system-wide to /opt/scanner_controller and setup systemd service.
  --local               Install locally in the current directory ($(pwd)).
  --dir <path>          Custom target installation directory.
  --user <username>     Target user to own files and execute systemd service.
  --service             Install and enable systemd service (scanscribe-scanner.service).
  --no-service          Do not install systemd service.
  --start               Start the systemd service immediately after installation.
  --port <port>         Web UI and API port (default: ${DEFAULT_PORT}).
  --inbox <path>        ScanScribe feeder inbox path (default: ${DEFAULT_INBOX_DIR}).
  --no-deps             Skip system package manager (apt/dnf/pacman) installation.
  -y, --yes, --non-interactive
                        Accept all defaults and install non-interactively.
  -h, --help            Show this help message.

Examples:
  sudo ./install.sh --system --start
  ./install.sh --local
  sudo ./install.sh --system --user pi --inbox /data/inbox
EOF
    exit 0
}

# Parse command line options
while [[ $# -gt 0 ]]; do
    case "$1" in
        --system)
            INSTALL_MODE="system"
            shift
            ;;
        --local)
            INSTALL_MODE="local"
            shift
            ;;
        --dir)
            TARGET_DIR="$2"
            shift 2
            ;;
        --user)
            TARGET_USER="$2"
            shift 2
            ;;
        --service)
            INSTALL_SERVICE=1
            shift
            ;;
        --no-service)
            INSTALL_SERVICE=0
            shift
            ;;
        --start)
            START_SERVICE=1
            shift
            ;;
        --port)
            PORT="$2"
            shift 2
            ;;
        --inbox)
            INBOX_DIR="$2"
            shift 2
            ;;
        --no-deps)
            SKIP_SYS_DEPS=1
            shift
            ;;
        -y|--yes|--non-interactive)
            NON_INTERACTIVE=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo -e "${RED}[ERROR] Unknown option: $1${NC}"
            usage
            ;;
    esac
done

print_banner

# Determine caller and privileges
IS_ROOT=0
if [[ $EUID -eq 0 ]]; then
    IS_ROOT=1
fi

# Detect defaults based on mode and execution context
if [[ -z "$TARGET_USER" ]]; then
    if [[ "$IS_ROOT" -eq 1 && -n "${SUDO_USER:-}" && "${SUDO_USER:-}" != "root" ]]; then
        TARGET_USER="$SUDO_USER"
    else
        TARGET_USER="${USER:-$(id -un)}"
    fi
fi

if [[ -z "$TARGET_DIR" ]]; then
    if [[ "$INSTALL_MODE" == "system" ]]; then
        TARGET_DIR="$DEFAULT_INSTALL_DIR"
    elif [[ "$INSTALL_MODE" == "local" ]]; then
        TARGET_DIR="$SOURCE_DIR"
    else
        if [[ "$IS_ROOT" -eq 1 ]]; then
            INSTALL_MODE="system"
            TARGET_DIR="$DEFAULT_INSTALL_DIR"
        else
            INSTALL_MODE="local"
            TARGET_DIR="$SOURCE_DIR"
        fi
    fi
fi

if [[ -z "$INSTALL_SERVICE" ]]; then
    if [[ "$INSTALL_MODE" == "system" || "$IS_ROOT" -eq 1 ]]; then
        INSTALL_SERVICE=1
    else
        INSTALL_SERVICE=0
    fi
fi

echo -e "${CYAN}Configuration Summary:${NC}"
echo -e "  - Deployment Mode:    ${BOLD}${INSTALL_MODE}${NC}"
echo -e "  - Install Directory:  ${BOLD}${TARGET_DIR}${NC}"
echo -e "  - Service User:       ${BOLD}${TARGET_USER}${NC}"
echo -e "  - Inbox Directory:    ${BOLD}${INBOX_DIR}${NC}"
echo -e "  - Web Port:           ${BOLD}${PORT}${NC}"
echo -e "  - Install Systemd:    ${BOLD}$([[ "$INSTALL_SERVICE" -eq 1 ]] && echo "Yes" || echo "No")${NC}"
echo -e "  - Start on Finish:    ${BOLD}$([[ "$START_SERVICE" -eq 1 ]] && echo "Yes" || echo "No")${NC}"
echo ""

if [[ "$NON_INTERACTIVE" -eq 0 ]]; then
    read -rp "Proceed with installation? [Y/n] " CONFIRM
    CONFIRM="${CONFIRM:-Y}"
    if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
        echo -e "${YELLOW}Installation aborted by user.${NC}"
        exit 0
    fi
fi

# Privilege helpers
run_as_root() {
    if [[ "$IS_ROOT" -eq 1 ]]; then
        "$@"
    else
        if command -v sudo &>/dev/null; then
            sudo "$@"
        else
            echo -e "${RED}[ERROR] Root privileges required for: $*${NC}"
            echo -e "Please install sudo or run this installer with root privileges."
            exit 1
        fi
    fi
}

run_as_target_user() {
    if [[ "$IS_ROOT" -eq 1 ]]; then
        if [[ "$TARGET_USER" != "root" && -n "$TARGET_USER" ]]; then
            if command -v sudo &>/dev/null; then
                sudo -u "$TARGET_USER" "$@"
            else
                su -s /bin/bash "$TARGET_USER" -c "$(printf "%q " "$@")"
            fi
        else
            "$@"
        fi
    else
        "$@"
    fi
}

# 1. Install System Dependencies
if [[ "$SKIP_SYS_DEPS" -eq 0 ]]; then
    echo -e "\n${CYAN}[1/6] Checking and installing system packages...${NC}"
    if command -v apt-get &>/dev/null; then
        echo -e "${GREEN}Detected Debian / Ubuntu / Raspberry Pi OS (apt-get)${NC}"
        run_as_root apt-get update -y
        run_as_root apt-get install -y --no-install-recommends \
            python3 \
            python3-pip \
            python3-venv \
            python3-dev \
            libportaudio2 \
            portaudio19-dev \
            libasound2-dev \
            python3-tk \
            build-essential \
            curl \
            git
    elif command -v dnf &>/dev/null; then
        echo -e "${GREEN}Detected Fedora / RHEL (dnf)${NC}"
        run_as_root dnf install -y \
            python3 \
            python3-pip \
            python3-devel \
            portaudio \
            portaudio-devel \
            alsa-lib-devel \
            python3-tkinter \
            gcc \
            curl \
            git
    elif command -v pacman &>/dev/null; then
        echo -e "${GREEN}Detected Arch Linux (pacman)${NC}"
        run_as_root pacman -Sy --noconfirm \
            python \
            python-pip \
            portaudio \
            alsa-lib \
            tk \
            base-devel \
            curl \
            git
    else
        echo -e "${YELLOW}[WARNING] Unknown package manager. Please ensure Python 3, venv, and PortAudio (libportaudio2) are installed.${NC}"
    fi
else
    echo -e "\n${YELLOW}[1/6] Skipping system package installation (--no-deps specified).${NC}"
fi

# 2. Configure Hardware Permissions (dialout & audio)
echo -e "\n${CYAN}[2/6] Configuring user permissions for serial ports and audio hardware...${NC}"
GROUPS_TO_ADD=""
if id "$TARGET_USER" &>/dev/null; then
    # Dialout / uucp for serial access
    if getent group dialout &>/dev/null; then
        GROUPS_TO_ADD="dialout"
    elif getent group uucp &>/dev/null; then
        GROUPS_TO_ADD="uucp"
    fi
    # Audio group
    if getent group audio &>/dev/null; then
        GROUPS_TO_ADD="${GROUPS_TO_ADD:+$GROUPS_TO_ADD,}audio"
    fi

    if [[ -n "$GROUPS_TO_ADD" ]]; then
        run_as_root usermod -aG "$GROUPS_TO_ADD" "$TARGET_USER"
        echo -e "${GREEN}Added user '${TARGET_USER}' to group(s): ${GROUPS_TO_ADD}${NC}"
    fi
else
    echo -e "${YELLOW}[WARNING] Target user '${TARGET_USER}' not found; skipping group configuration.${NC}"
fi

# 3. Setup Target Installation Directory
echo -e "\n${CYAN}[3/6] Deploying application files to ${TARGET_DIR}...${NC}"
if [[ "$TARGET_DIR" != "$SOURCE_DIR" ]]; then
    run_as_root mkdir -p "$TARGET_DIR"
    echo -e "Copying repository files from ${SOURCE_DIR} to ${TARGET_DIR}..."
    if command -v rsync &>/dev/null; then
        run_as_root rsync -a --delete \
            --exclude="venv" \
            --exclude=".git" \
            --exclude="__pycache__" \
            --exclude="*.pyc" \
            --exclude="dist" \
            --exclude="build" \
            "$SOURCE_DIR/" "$TARGET_DIR/"
    else
        run_as_root cp -rT "$SOURCE_DIR" "$TARGET_DIR"
    fi
fi

# Ensure target directory ownership
if [[ "$IS_ROOT" -eq 1 || "$INSTALL_MODE" == "system" ]]; then
    run_as_root chown -R "${TARGET_USER}:${TARGET_USER}" "$TARGET_DIR"
fi

# Ensure Inbox directory exists and is writable
echo -e "Ensuring ScanScribe inbox directory exists at ${INBOX_DIR}..."
run_as_root mkdir -p "$INBOX_DIR"
run_as_root chown -R "${TARGET_USER}:${TARGET_USER}" "$INBOX_DIR"
run_as_root chmod 775 "$INBOX_DIR"

# 4. Set Up Python Virtual Environment & Install Dependencies
echo -e "\n${CYAN}[4/6] Setting up Python virtual environment in ${TARGET_DIR}/venv...${NC}"
VENV_DIR="${TARGET_DIR}/venv"
VENV_PYTHON="${VENV_DIR}/bin/python3"
VENV_PIP="${VENV_DIR}/bin/pip"

if [[ ! -d "$VENV_DIR" ]]; then
    run_as_target_user python3 -m venv "$VENV_DIR"
fi

echo -e "Upgrading pip, setuptools, and wheel..."
run_as_target_user "$VENV_PYTHON" -m pip install --upgrade pip setuptools wheel -q

echo -e "Installing Python requirements from ${TARGET_DIR}/requirements.txt..."
run_as_target_user "$VENV_PIP" install -r "${TARGET_DIR}/requirements.txt"

# 5. Configure Systemd Service
if [[ "$INSTALL_SERVICE" -eq 1 ]]; then
    echo -e "\n${CYAN}[5/6] Creating systemd service (scanscribe-scanner.service)...${NC}"
    SERVICE_FILE="/etc/systemd/system/scanscribe-scanner.service"
    
    # Write dynamic systemd unit
    TMP_SERVICE="$(mktemp)"
    cat << EOF > "$TMP_SERVICE"
[Unit]
Description=Uniden Scanner Controller & ScanScribe Feeder Service
After=network.target sound.target

[Service]
Type=simple
User=${TARGET_USER}
Group=${TARGET_USER}
WorkingDirectory=${TARGET_DIR}
ExecStart=${VENV_PYTHON} -m uvicorn server:app --host 0.0.0.0 --port ${PORT}
Restart=always
RestartSec=5
Environment=PYTHONUNBUFFERED=1
Environment=SCANSCRIBE_INBOX_DIR=${INBOX_DIR}
Environment=SCANNER_A_PORT=/dev/ttyACM0
Environment=SCANNER_B_PORT=NONE

# Standard permissions and capabilities
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

    run_as_root cp "$TMP_SERVICE" "$SERVICE_FILE"
    run_as_root rm -f "$TMP_SERVICE"
    run_as_root chmod 644 "$SERVICE_FILE"

    echo -e "Reloading systemd daemon..."
    run_as_root systemctl daemon-reload
    run_as_root systemctl enable scanscribe-scanner.service

    if [[ "$START_SERVICE" -eq 1 ]]; then
        echo -e "Starting scanscribe-scanner.service..."
        run_as_root systemctl restart scanscribe-scanner.service
        sleep 2
        run_as_root systemctl status scanscribe-scanner.service --no-pager || true
    fi
else
    echo -e "\n${YELLOW}[5/6] Skipping systemd service setup.${NC}"
fi

# 6. Verification and Hardware Discovery
echo -e "\n${CYAN}[6/6] Verifying installation and hardware devices...${NC}"
run_as_target_user "$VENV_PYTHON" - << 'PYEOF'
import sys

print("  - Python version:", sys.version.split()[0])

modules = [
    ("fastapi", "FastAPI"),
    ("uvicorn", "Uvicorn ASGI Server"),
    ("sounddevice", "SoundDevice (PortAudio Audio I/O)"),
    ("serial", "PySerial (Scanner COM I/O)"),
    ("mutagen", "Mutagen (ID3 / Broadcast Metadata)"),
    ("lameenc", "Lameenc (MP3 Encoding Engine)"),
    ("numpy", "NumPy (DSP / Vox Audio Processing)"),
]

all_ok = True
for mod, desc in modules:
    try:
        __import__(mod)
        print(f"    [OK] {desc}")
    except Exception as e:
        print(f"    [FAIL] {desc}: {e}")
        all_ok = False

if not all_ok:
    print("\n[WARNING] Some Python libraries failed to import properly.")
else:
    print("\n  - All core dependencies loaded successfully!")

# Check audio devices
try:
    import sounddevice as sd
    devices = sd.query_devices()
    print(f"  - Detected {len(devices)} audio device(s) via PortAudio.")
except Exception as e:
    print(f"  - Audio device query note: {e}")

# Check serial devices
import glob
serial_ports = sorted(glob.glob("/dev/ttyACM*") + glob.glob("/dev/ttyUSB*"))
if serial_ports:
    print(f"  - Detected serial scanner ports: {', '.join(serial_ports)}")
else:
    print("  - No /dev/ttyACM* or /dev/ttyUSB* devices detected currently.")
    print("    (Plug in your Uniden scanner via USB and set to Serial / Mass Storage mode OFF)")
PYEOF

echo -e "\n${GREEN}==============================================================================${NC}"
echo -e "${BOLD}${GREEN}Installation Complete!${NC}"
echo -e "${GREEN}==============================================================================${NC}"
echo -e "Installation path: ${BOLD}${TARGET_DIR}${NC}"
echo -e "Virtualenv Python: ${BOLD}${VENV_PYTHON}${NC}"
echo -e "Inbox directory:   ${BOLD}${INBOX_DIR}${NC}"
echo -e "Web Interface URL: ${BOLD}http://<your-ip>:${PORT}${NC}"
echo ""

if [[ "$INSTALL_SERVICE" -eq 1 ]]; then
    echo -e "${CYAN}Service Management Commands:${NC}"
    echo -e "  - Start service:   ${BOLD}sudo systemctl start scanscribe-scanner${NC}"
    echo -e "  - Stop service:    ${BOLD}sudo systemctl stop scanscribe-scanner${NC}"
    echo -e "  - Check status:    ${BOLD}sudo systemctl status scanscribe-scanner${NC}"
    echo -e "  - View live logs:  ${BOLD}sudo journalctl -u scanscribe-scanner -f${NC}"
    echo ""
fi

echo -e "${CYAN}Manual Standalone Launch:${NC}"
echo -e "  cd ${TARGET_DIR}"
echo -e "  ./run.sh"
echo -e "  # Or directly:"
echo -e "  ${VENV_PYTHON} -m uvicorn server:app --host 0.0.0.0 --port ${PORT}"
echo ""

if [[ -n "${GROUPS_TO_ADD:-}" ]]; then
    echo -e "${YELLOW}[NOTE] If user '${TARGET_USER}' was added to dialout/audio groups,${NC}"
    echo -e "${YELLOW}       log out and log back in (or run 'newgrp dialout') for permissions to take effect.${NC}"
fi
echo -e "${GREEN}==============================================================================${NC}"
