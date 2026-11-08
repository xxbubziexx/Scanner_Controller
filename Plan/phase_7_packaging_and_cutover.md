# Phase 7: Packaging, Cutover & Python Retirement

> **Objective:** Ship the Rust build to the live node and retire Python safely.
> **Prerequisites:** [Phase 5](phase_5_server_and_shadow_run.md) and [Phase 6](phase_6_desktop_app.md) exit gates passed.
> New user-facing features remain forbidden until this phase's exit gate.

---

## 1. Packaging
- [x] Release artifacts: `x86_64-pc-windows-msvc` / `x86_64-pc-windows-gnu`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (Pi), optional `armv7` (`cargo run -p xtask -- package`).
- [x] Rewrite [`install.sh`](../install.sh): install the prebuilt binary, create user/groups (`dialout`/`uucp`, `audio`), inbox dir, systemd unit. Keep **all** current flags: `--system`, `--local`, `--dir`, `--user`, `--service`, `--no-service`, `--start`, `--port`, `--inbox`, `--no-deps`, `-y`, plus `--rollback-python`.
- [x] Keep the `scanscribe-scanner` service name so ScanScribe-side tooling is unaffected.
- [x] [`Dockerfile`](../Dockerfile): multi-stage Rust build.

## 2. Config Migration
- [x] Rust reads the old `config.json`. On first save it writes device names next to the indices. Never overwrite or delete the user's `config.json`, inbox or recordings (back up before the first save).

## 3. Cutover
- [x] Cut over on the live node with a **one-command rollback** to Python (`sudo ./install.sh --system --rollback-python --start`), kept ready for 14 days.
- [ ] Monitor calls per hour and ScanScribe ingest errors daily during the soak.

## 4. Python Retirement (only after the exit gate)
- [ ] Move Python to `legacy/` (or tag and delete). Drop `requirements.txt`, `*.spec`, `build_*.bat`.
- [ ] Update [`README.md`](../README.md). Keep all reverse-engineering `.md` docs (they remain the spec).
- [ ] Decide the repo layout (keep `rust/` or promote it to the root).

## Exit Gate
14 days on Rust in production with zero dropped calls, zero crashes and no rollback. ScanScribe ingests every file and sidecar without metadata errors.
