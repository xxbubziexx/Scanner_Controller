# Uniden Scanner Mutual Exclusion & Recording Controller

High-performance mutual exclusion and audio recording controller for dual Uniden scanners (e.g., **BCD436HP**, **BCD996P2**, **SDS100**, **SDS200**). Synchronizes multiple radios to prevent duplicate channel recording, provides real-time dual-channel loopback monitoring, and formats audio calls with 100% ProScan-compliant metadata for automated ingestion into ScanScribe and transcription pipelines.

---

## Features

- **Dual-Radio Synchronization & Mutual Exclusion**:
  - Automatically detects when both scanners land on the same frequency or talkgroup (TGID).
  - Skips the secondary radio via non-destructive rotary knob rotation / channel advance without setting permanent lockouts.
  - Automatically aborts secondary duplicate recordings before they are saved or dispatched.
- **Crystal-Clear Real-Time Loopback Monitoring**:
  - Independent dual-channel ring buffering (Channel 0 = Scanner A, Channel 1 = Scanner B).
  - Panning modes: **`STEREO_SPLIT`** (A=Left ear, B=Right ear) or **`CENTER`** (sample-level summing across both speakers).
  - Smooth 15ms de-click gain ramping during ducking and muting.
- **ScanScribe & Ingestion Feeder**:
  - Output formats: **RIFF WAV** (`pros` chunk + `LIST-INFO`) or **MP3** (ID3v2.3 + `pros` chunk).
  - Bit-exact support for all 33 ProScan Custom Formatter placeholders (`%TT`, `%D`, `%C`, `%TG`, `%G`, `%S`, `%F`, etc.).
  - Configurable JSON metadata sidecars (`.json`) alongside audio recordings with a single toggle in GUI.
- **Cross-Platform Architectures**:
  - **Native Desktop GUI**: Built with CustomTkinter (`gui.py` and standalone `ScannerControllerGUI.exe`).
  - **Headless & Web Service**: Built with FastAPI and WebSocket telemetry (`server.py`).
  - **Multi-Family Driver Engine**: Supports both DMA (996P2/325P2/XT) and HomePatrol (436HP/536HP/SDS) command dialects.

---

## Quick Start

### 1. Requirements & Setup
```bash
# Clone the repository
git clone https://github.com/xxbubziexx/Scanner_Controller.git
cd Scanner_Controller

# Install Python dependencies
pip install -r requirements.txt
```

### 2. Run the Native GUI
```bash
python gui.py
```
Or run the standalone pre-compiled executable:
```bash
dist\ScannerControllerGUI.exe
```

### 3. Run the Web Server / Daemon
```bash
python server.py
```
Open your browser at `http://127.0.0.1:8000` to access the web controller.

---

## Running Automated Tests
```bash
python -m unittest discover tests
```

---

## Reverse Engineering Documentation

Detailed reverse-engineered specifications from ProScan source are documented in:
* [`KEYPAD-AND-DISPLAY.md`](KEYPAD-AND-DISPLAY.md): Wire protocols, knob rotation, keypad dialects.
* [`FORMATTERS-AND-FILENAME-SCHEME.md`](FORMATTERS-AND-FILENAME-SCHEME.md): All 33 custom specifiers, sequential replacement ordering, naming rules.
* [`RECORDING-METADATA-FORMAT.md`](RECORDING-METADATA-FORMAT.md): Custom binary RIFF `pros` chunk structure and ID3v2.3 tags.
* [`AUDIO-IO-PIPELINE.md`](AUDIO-IO-PIPELINE.md): Dual-channel ring buffer, loopback monitor, and PortAudio architecture.
* [`SERIAL-POLLING-AND-LAN-AUDIO.md`](SERIAL-POLLING-AND-LAN-AUDIO.md): Pipelined serial polling bursts (`MDL`, `STS`, `GLG`, `VOL`, `SQL`, `PWR`, `GSI`).
* [`DMA-VS-HP-ARCHITECTURE.md`](DMA-VS-HP-ARCHITECTURE.md): Structural differences between DMA and HomePatrol scanner families.
* [`VOX-ALGORITHM.md`](VOX-ALGORITHM.md): Audio squelch detection and hang timers.

---

## License

MIT License.
