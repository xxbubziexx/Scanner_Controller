# Phase 6: `sc-desktop` (Desktop App)

> **Objective:** Replace the 76 KB CustomTkinter [`gui.py`](../gui.py).
> **Prerequisites:** [Phase 5](phase_5_server_and_shadow_run.md) exit gate passed. Can overlap with Phase 7 packaging work.

---

## 1. Audit First
- [ ] Audit `gui.py` and list every feature (VU meters, device dropdowns, tray, status panels, etc.) in a checklist table in this file. Mark each as *present in `static/`* or *missing*.
- [ ] Resolve the open decision: **Tauri v2 reusing `static/`** (recommended) vs `egui`. Record it in `ACTIVE_CONTEXT.md`.

## 2. Build (Tauri path)
- [ ] Tauri v2 shell embedding `sc-core`, `sc-engine` and `sc-server` in-process, rendering `static/` in the webview.
- [ ] Implement every *missing* feature from the audit in `static/` or the shell. Any `static/` change must not break the Python-served UI.
- [ ] Tray icon, minimize-to-tray, start-minimized, background-recording indicator, single-instance lock.
- [ ] Windows installer (MSI/NSIS via the Tauri bundler).

## Exit Gate
Every `gui.py` feature in the checklist has a matching feature in the new UI, verified item by item.
