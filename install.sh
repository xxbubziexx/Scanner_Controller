#!/usr/bin/env bash
# ==============================================================================
# Uniden Scanner Mutual Exclusion & Recording Controller
# Automated Production Installer for Linux (Debian, Ubuntu, Raspberry Pi OS, etc.)
# Supports the Rust single-binary daemon (`scanner-controller`) and instant
# one-command Python rollback (`--rollback-python`).
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
ROLLBACK_PYTHON=0

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
                                                                                      
         Automated Linux Rust Deployment, Systemd & Rollback Installer
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
  --rollback-python     Switch scanscribe-scanner.service back to the Python reference runtime.
  -y, --yes, --non-interactive
                        Accept all defaults and install non-interactively.
  -h, --help            Show this help message.

Examples:
  sudo ./install.sh --system --start
  ./install.sh --local
  sudo ./install.sh --system --user pi --inbox /data/inbox
  sudo ./install.sh --system --rollback-python --start
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
        --rollback-python)
            ROLLBACK_PYTHON=1
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

RUNTIME_LABEL="Rust (scanner-controller)"
if [[ "$ROLLBACK_PYTHON" -eq 1 ]]; then
    RUNTIME_LABEL="Python Rollback (uvicorn server:app)"
fi

echo -e "${CYAN}Configuration Summary:${NC}"
echo -e "  - Active Runtime:     ${BOLD}${RUNTIME_LABEL}${NC}"
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
            libasound2 \
            libasound2-dev \
            libudev1 \
            libudev-dev \
            pkg-config \
            build-essential \
            python3 \
            python3-pip \
            python3-venv \
            libportaudio2 \
            curl \
            git
    elif command -v dnf &>/dev/null; then
        echo -e "${GREEN}Detected Fedora / RHEL (dnf)${NC}"
        run_as_root dnf install -y \
            alsa-lib \
            alsa-lib-devel \
            systemd-devel \
            pkgconf-pkg-config \
            gcc \
            python3 \
            python3-pip \
            portaudio \
            curl \
            git
    elif command -v pacman &>/dev/null; then
        echo -e "${GREEN}Detected Arch Linux (pacman)${NC}"
        run_as_root pacman -Sy --noconfirm \
            alsa-lib \
            systemd-libs \
            pkgconf \
            base-devel \
            python \
            python-pip \
            portaudio \
            curl \
            git
    else
        echo -e "${YELLOW}[WARNING] Unknown package manager. Please ensure ALSA (libasound2) and libudev are installed.${NC}"
    fi
else
    echo -e "\n${YELLOW}[1/6] Skipping system package installation (--no-deps specified).${NC}"
fi

# 2. Configure Hardware Permissions (dialout & audio)
echo -e "\n${CYAN}[2/6] Configuring user permissions for serial ports and audio hardware...${NC}"
GROUPS_TO_ADD=""
if id "$TARGET_USER" &>/dev/null; then
    if getent group dialout &>/dev/null; then
        GROUPS_TO_ADD="dialout"
    elif getent group uucp &>/dev/null; then
        GROUPS_TO_ADD="uucp"
    fi
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

# 3. Setup Target Installation Directory (Prime Directive #5: Never overwrite config.json or recordings)
echo -e "\n${CYAN}[3/6] Deploying application files to ${TARGET_DIR}...${NC}"
if [[ -f "${TARGET_DIR}/config.json" ]]; then
    echo -e "${GREEN}Backing up existing ${TARGET_DIR}/config.json -> ${TARGET_DIR}/config.json.bak${NC}"
    run_as_root cp -p "${TARGET_DIR}/config.json" "${TARGET_DIR}/config.json.bak"
fi

if [[ "$TARGET_DIR" != "$SOURCE_DIR" ]]; then
    run_as_root mkdir -p "$TARGET_DIR"
    echo -e "Copying repository files from ${SOURCE_DIR} to ${TARGET_DIR} (preserving config.json and recordings)..."
    if command -v rsync &>/dev/null; then
        run_as_root rsync -a \
            --exclude="venv" \
            --exclude=".git" \
            --exclude="__pycache__" \
            --exclude="*.pyc" \
            --exclude="target" \
            --exclude="config.json" \
            --exclude="recordings" \
            "$SOURCE_DIR/" "$TARGET_DIR/"
    else
        run_as_root cp -rn "$SOURCE_DIR/"* "$TARGET_DIR/" || true
    fi
    if [[ ! -f "${TARGET_DIR}/config.json" && -f "${SOURCE_DIR}/config.json" ]]; then
        run_as_root cp "${SOURCE_DIR}/config.json" "${TARGET_DIR}/config.json"
    fi
fi

if [[ "$IS_ROOT" -eq 1 || "$INSTALL_MODE" == "system" ]]; then
    run_as_root chown -R "${TARGET_USER}:${TARGET_USER}" "$TARGET_DIR"
fi

# Ensure Inbox directory exists and is writable
echo -e "Ensuring ScanScribe inbox directory exists at ${INBOX_DIR}..."
run_as_root mkdir -p "$INBOX_DIR"
run_as_root chown -R "${TARGET_USER}:${TARGET_USER}" "$INBOX_DIR"
run_as_root chmod 775 "$INBOX_DIR"

# 4. Install or Build Rust Binary (and optionally Python rollback environment)
echo -e "\n${CYAN}[4/6] Installing scanner-controller binary...${NC}"
RUST_BIN="${TARGET_DIR}/scanner-controller"

if [[ -f "${SOURCE_DIR}/dist/scanner-controller" ]]; then
    echo -e "${GREEN}Installing prebuilt binary from ${SOURCE_DIR}/dist/scanner-controller...${NC}"
    run_as_root cp "${SOURCE_DIR}/dist/scanner-controller" "$RUST_BIN"
    run_as_root chmod 755 "$RUST_BIN"
elif [[ -f "${SOURCE_DIR}/target/release/scanner-controller" ]]; then
    echo -e "${GREEN}Installing release binary from ${SOURCE_DIR}/target/release/scanner-controller...${NC}"
    run_as_root cp "${SOURCE_DIR}/target/release/scanner-controller" "$RUST_BIN"
    run_as_root chmod 755 "$RUST_BIN"
elif command -v cargo &>/dev/null && [[ -f "${SOURCE_DIR}/Cargo.toml" ]]; then
    echo -e "${CYAN}Building release binary from source via Cargo...${NC}"
    (cd "${SOURCE_DIR}" && run_as_target_user cargo build --release --bin scanner-controller)
    run_as_root cp "${SOURCE_DIR}/target/release/scanner-controller" "$RUST_BIN"
    run_as_root chmod 755 "$RUST_BIN"
else
    echo -e "${YELLOW}[WARNING] No prebuilt scanner-controller binary or cargo toolchain found.${NC}"
fi

VENV_DIR="${TARGET_DIR}/venv"
VENV_PYTHON="${VENV_DIR}/bin/python3"
if [[ "$ROLLBACK_PYTHON" -eq 1 ]]; then
    echo -e "\n${CYAN}Ensuring Python rollback virtualenv in ${VENV_DIR}...${NC}"
    if [[ ! -d "$VENV_DIR" ]]; then
        run_as_target_user python3 -m venv "$VENV_DIR"
    fi
    run_as_target_user "$VENV_PYTHON" -m pip install --upgrade pip setuptools wheel -q
    run_as_target_user "${VENV_DIR}/bin/pip" install -r "${TARGET_DIR}/requirements.txt"
fi

# 5. Configure Systemd Service (keeps scanscribe-scanner.service name)
if [[ "$INSTALL_SERVICE" -eq 1 ]]; then
    echo -e "\n${CYAN}[5/6] Creating systemd service (scanscribe-scanner.service)...${NC}"
    SERVICE_FILE="/etc/systemd/system/scanscribe-scanner.service"

    if [[ "$ROLLBACK_PYTHON" -eq 1 ]]; then
        EXEC_START="${VENV_PYTHON} -m uvicorn server:app --host 0.0.0.0 --port ${PORT}"
    else
        EXEC_START="${RUST_BIN} serve --host 0.0.0.0 --port ${PORT} --no-browser"
    fi

    TMP_SERVICE="$(mktemp)"
    cat << EOF > "$TMP_SERVICE"
[Unit]
Description=Uniden Scanner Controller & ScanScribe Feeder Service
After=network.target sound.target

[Service]
Type=simple
User=${TARGET_USER}
Group=${TARGET_USER}
SupplementaryGroups=dialout audio
WorkingDirectory=${TARGET_DIR}
ExecStart=${EXEC_START}
# Instant rollback reference:
# Rust:   ${RUST_BIN} serve --host 0.0.0.0 --port ${PORT} --no-browser
# Python: ${VENV_PYTHON} -m uvicorn server:app --host 0.0.0.0 --port ${PORT}
Restart=always
RestartSec=5
Environment=RUST_LOG=info
Environment=PYTHONUNBUFFERED=1
Environment=SCANSCRIBE_INBOX_DIR=${INBOX_DIR}
Environment=SCANNER_A_PORT=/dev/ttyACM0
Environment=SCANNER_B_PORT=NONE

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

# 6. Verification
echo -e "\n${CYAN}[6/6] Verifying installation...${NC}"
if [[ -x "$RUST_BIN" ]]; then
    echo -e "  - Rust binary installed: ${GREEN}$("$RUST_BIN" --version)${NC}"
fi
echo -e "\n${GREEN}${BOLD}Installation Complete!${NC}"
echo -e "  - Web Dashboard: ${CYAN}http://localhost:${PORT}${NC}"
echo -e "  - Service Status: ${CYAN}sudo systemctl status scanscribe-scanner${NC}"
echo -e "  - Instant Python Rollback: ${YELLOW}sudo ./install.sh --system --rollback-python --start${NC}"
