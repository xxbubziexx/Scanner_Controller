# Phase 5: `sc-server` & Headless Daemon (Shadow Run)

> **Objective:** Drop-in replacement for [`server.py`](../server.py). The existing [`static/`](../static/) web UI works unchanged.
> **Prerequisites:** [Phase 4](phase_4_engine_and_feeder.md) exit gate passed.

---

## 1. Routes (paths, methods and JSON shapes identical to Python)
- [ ] `GET /`, `GET /favicon.ico`
- [ ] `GET /api/state`, `POST /api/config`
- [ ] `POST /api/scanner/{id}/model`, `POST /api/skip/{id}`
- [ ] `/api/audio/devices`, `/api/audio/output_devices`, `/api/ports/available`, `/api/ports/detect`, `/api/scanners/discover_lan`, `/api/models/catalog`
- [ ] `POST /api/feeder/config`, `POST /api/feeder/dispatch_test`
- [ ] `WS /ws` (state snapshots, same message shape)
- [ ] Before implementing, diff this list against the actual routes in `server.py`. Add anything missing.

## 2. Contract Tests
- [ ] Replay the Phase 0 `fixtures/api/*.json` against the Rust server and diff the responses (ignoring timestamps).
- [ ] Load `static/` in a browser against the Rust backend and check every panel (no edits to HTML, CSS or JS).

## 3. Daemon
- [ ] Embed `static/` with `rust-embed` (single binary).
- [ ] Graceful shutdown (SIGINT/SIGTERM/Ctrl+C) via `CancellationToken`: stop polling, flush in-flight recordings, close ports.
- [ ] systemd unit (`Type=notify` optional), `tracing-journald`.

## 4. Shadow Run (≥ 7 days)
A serial port has one owner, so Python and Rust cannot run on the same radios at the same time.
- [ ] Alternate Python and Rust on the live Pi (e.g. daily), each writing to its **own** inbox. Use a spare scanner pair for true side-by-side runs if available.
- [ ] Compare calls per hour, duration distributions, exclusion event logs, metadata bytes, and ScanScribe transcription success.
- [ ] Python stays the instant rollback target throughout.

## Exit Gate
Web UI fully functional on the Rust backend, contract tests green, and every shadow-run discrepancy explained or fixed.
