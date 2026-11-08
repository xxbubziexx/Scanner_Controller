# Phase 5: `sc-server` & Headless Daemon (Shadow Run)

> **Objective:** Drop-in replacement for [`server.py`](../server.py). The existing [`static/`](../static/) web UI works unchanged.
> **Prerequisites:** [Phase 4](phase_4_engine_and_feeder.md) exit gate passed.

---

## 1. Routes (paths, methods and JSON shapes identical to Python)
- [x] `GET /`, `GET /favicon.ico`
- [x] `GET /api/state`, `POST /api/config`
- [x] `POST /api/scanner/{id}/model`, `POST /api/skip/{id}`
- [x] `/api/audio/devices`, `/api/audio/output_devices`, `/api/ports/available`, `/api/ports/detect`, `/api/scanners/discover_lan`, `/api/models/catalog`
- [x] `POST /api/feeder/config`, `POST /api/feeder/dispatch_test`
- [x] `WS /ws` (state snapshots, same message shape)
- [x] Before implementing, diff this list against the actual routes in `server.py`. Add anything missing.

## 2. Contract Tests
- [x] Replay the Phase 0 `fixtures/api/*.json` against the Rust server and diff the responses (ignoring timestamps).
- [x] Load `static/` in a browser against the Rust backend and check every panel (no edits to HTML, CSS or JS).

## 3. Daemon
- [x] Embed `static/` with `rust-embed` (single binary).
- [x] Graceful shutdown (SIGINT/SIGTERM/Ctrl+C) via `CancellationToken`: stop polling, flush in-flight recordings, close ports.
- [x] systemd unit (`Type=notify` optional), `tracing-journald`.

## 4. Shadow Run (≥ 7 days)
A serial port has one owner, so Python and Rust cannot run on the same radios at the same time.
- [ ] Alternate Python and Rust on the live Pi (e.g. daily), each writing to its **own** inbox. Use a spare scanner pair for true side-by-side runs if available.
- [ ] Compare calls per hour, duration distributions, exclusion event logs, metadata bytes, and ScanScribe transcription success.
- [ ] Python stays the instant rollback target throughout.

## Exit Gate
Web UI fully functional on the Rust backend, contract tests green, and every shadow-run discrepancy explained or fixed.
