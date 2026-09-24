#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
cd "$DIR"

echo "=========================================================="
echo "    Uniden Scanner Mutual Exclusion Controller (Linux)   "
echo "=========================================================="

if ! command -v python3 &>/dev/null; then
    echo "[ERROR] python3 could not be found. Please install python3 (e.g. sudo apt install python3 python3-venv)."
    exit 1
fi

# Ensure user is in dialout group for serial permissions
if ! groups | grep -q "\bdialout\b"; then
    echo "[WARNING] Current user may not be in 'dialout' group to access /dev/tty* ports."
    echo "          Run: sudo usermod -aG dialout $USER (and log out/in)."
fi

if [ ! -d "venv" ]; then
    echo "[INFO] Creating virtual environment in ./venv..."
    python3 -m venv venv
fi

source venv/bin/activate
pip install -q -r requirements.txt

echo "[INFO] Starting server on http://0.0.0.0:8000 ..."
python3 -m uvicorn server:app --host 0.0.0.0 --port 8000
