# Phase 6: `sc-desktop` (Desktop App)

> **Objective:** Replace the 76 KB CustomTkinter [`gui.py`](../gui.py).
> **Prerequisites:** [Phase 5](phase_5_server_and_shadow_run.md) exit gate passed. Can overlap with Phase 7 packaging work.

---

## 1. Audit First
- [x] Audit `gui.py` and list every feature (VU meters, device dropdowns, tray, status panels, etc.) in a checklist table in this file. Mark each as *present in `static/`* or *missing*.
- [x] Resolve the open decision: **Desktop shell reusing `static/`** (recommended) vs `egui`. Record it in `ACTIVE_CONTEXT.md`.

### `gui.py` vs `static/` Feature Audit Checklist

| `gui.py` Feature / Control | Source Reference | Initial `static/` Status | Phase 6 Resolution |
|---|---|---|---|
| Master Mutual-Exclusion Sync Toggle (`sync.enable_sync`) | `gui.py:150-163` | Present (`#masterSyncToggle`) | Verified |
| Master Record Calls Toggle (`feeder.enable_feeder`) | `gui.py:165-177` | Missing | Added `#masterRecordToggle` in header bar |
| Background Recording Indicator Badge | `gui.py:178-192` | Missing | Added `#globalRecBadge` (`REC STANDBY` / `🔴 REC ACTIVE` / `REC DISABLED`) + document title indicator |
| Scanner A & B LCD Displays (`sys`, `dept`, `chan`, `tgid`, `freq`, `mod`, `rssi`) | `gui.py:345-410` | Present | Verified + added Unit ID (`#uidA`, `#uidB`) |
| Per-Card Recording Badge (`REC ON` / `🔴 REC` / `REC OFF`) | `gui.py:424-434` | Missing | Added `#recBadgeA` & `#recBadgeB` |
| Live Audio VU Meter (0–100% bar + `%` readout + Green/Amber/Red thresholds) | `gui.py:412-422`, `924-953` | Missing | Added `#vuBarA`/`#vuPctA` and `#vuBarB`/`#vuPctB` (`<60%` green, `60–84%` amber, `>=85%` red) |
| Per-Card Enable Radio & Enable Audio Toggles | `gui.py:420-436` | Missing on card | Added `#cardEnableA`/`B` & `#cardAudioEnableA`/`B` |
| Manual Skip / Avoid (`KEY,AVD` / `KEY,L/O`) | `gui.py:438-448` | Present | Verified |
| 34-Model Catalog Dropdown & Baud Rate Filtering | `gui.py:450-482` | Present | Verified |
| Serial Auto-Detect (`\rMDL\r`, `\rSI\r`) & LAN UDP Discovery (`50536`) | `gui.py:720-845` | Present | Verified |
| Lockout Method Selector (`KEYPRESS` vs `MEMORY`) | `gui.py:478` | Missing | Added `#cfg_lockout_a` & `#cfg_lockout_b` |
| Priority Mode (`EQUAL_PEERS`, `PRIMARY_SECONDARY`, `DEPARTMENT_PRIORITY`) | `gui.py:252` | Missing `DEPARTMENT_PRIORITY` | Added `DEPARTMENT_PRIORITY` option to `#priorityModeSelect` |
| Hang Time (`hang_time_seconds`) & Auto-Release (`auto_release_seconds`) | `gui.py:265-285` | Missing | Added `#hangTimeInput` & `#autoReleaseInput` |
| Active Holds Pill Bar (`active_exclusions`) | `gui.py:286-292` | Missing | Added `#activeExclusionsBar` |
| Live Audio Loopback Monitor Panel (Enable, Output Speaker `/api/audio/output_devices`, `STEREO_SPLIT`/`CENTER`, Volume Slider `0–100%`) | `gui.py:293-336` | Missing | Added full **🔊 Live Audio Loopback Monitor** panel |
| Exclusion Event Log + Clear View Button | `gui.py:670-705` | Missing Clear button | Added `🧹 Clear View` (`clearLogView()`) |
| ScanScribe Feeder Mode (`DIRECTORY_DROP`, `HTTP_WEBHOOK`, `LIVE_STREAM`), Format (`WAV`/`MP3`), Sample Rate, Inbox Dir, Test Dispatch | `gui.py:488-548` | Present (missing `LIVE_STREAM`) | Verified + added `LIVE_STREAM` |
| Webhook URL Input, Embed ProScan Tags Toggle, Write `.json` Sidecar Toggle | `gui.py:512-536` | Missing | Added `#cfg_feeder_webhook`, `#cfg_feeder_proscan_tags`, `#cfg_feeder_sidecar_json` |
| Filename & TIT2 Template Preset Chips + Live Preview | `gui.py:552-607` | Missing | Added `ProScan Default`, `Simple`, `Detailed`, `Dept/Chan` chips + live preview |
| ProScan 37 `%` Specifiers Reference Drawer | `gui.py:610-635` | Missing | Added collapsible `#specifiersGrid` drawer with all 37 tokens |

## 2. Build (Desktop Shell + Unified `static/` Path)
- [x] `sc-desktop` shell embedding `sc-core`, `sc-engine` and `sc-server` in-process, rendering `static/` in a native app-mode WebView2/Chromium window (`--app=http://127.0.0.1:8000`).
- [x] Implement every *missing* feature from the audit in `static/` or the shell. Any `static/` change must not break the Python-served UI.
- [x] Single-instance lock (`SingleInstanceGuard`), `--start-minimized`, background-recording indicator (`RecordingIndicator`), and one-click Windows launcher (`run_rust_app.bat`).

## Exit Gate
Every `gui.py` feature in the checklist has a matching feature in the new UI, verified item by item.
