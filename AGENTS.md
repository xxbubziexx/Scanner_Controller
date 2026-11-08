# Scanner_Controller Agent Guidelines & Operational Protocol

Welcome to **Scanner_Controller**, a mutual-exclusion and recording controller for dual Uniden scanners
(BCD996P2, BCD436HP, SDS100/200, and others). It polls the radios over serial or LAN, stops both radios from
recording the same TGID/frequency, monitors live audio, and writes recordings tagged with ProScan-compatible
metadata. Those recordings go to the **ScanScribe** transcription pipeline.

> **CRITICAL ENVIRONMENT CONTEXT:**
> The controller feeds ScanScribe with real public-safety radio traffic, typically from a Raspberry Pi /
> Linux node running as a systemd service (`scanscribe-scanner`). Development happens on Windows and Linux,
> by different AI models and humans. The project is **mid-migration from Python to Rust**
> (see [`Plan/rust_conversion_master_plan.md`](Plan/rust_conversion_master_plan.md)). Every agent must follow
> these rules to protect live capture and keep the two codebases from drifting apart.

---

## 1. Prime Directives

1. **Never lose a call.** A completed transmission must never be dropped, truncated, or silently
   discarded. The only exception is the *intentional* duplicate-abort performed by mutual exclusion.
   Failed dispatches must be logged and retried or written to a fallback path.
2. **Metadata is byte-exact.** The `pros` chunk, ID3v2.3 tag, RIFF `LIST`/`INFO` chunk, and `%` template
   output must match [`RECORDING-METADATA-FORMAT.md`](RECORDING-METADATA-FORMAT.md) and
   [`FORMATTERS-AND-FILENAME-SCHEME.md`](FORMATTERS-AND-FILENAME-SCHEME.md) byte for byte. ScanScribe and
   ProScan-compatible tools rely on it.
3. **The reverse-engineering docs are the spec.** The `*.md` docs in the repo root carry `file:line`
   provenance from the decompiled ProScan binaries. When code and docs disagree, **stop and flag it**
   in `ACTIVE_CONTEXT.md`. Don't "fix" either one silently. Don't edit provenance lines without
   re-verifying them against the source.
4. **One owner per serial port.** Never write code or run tools that open a scanner COM/TTY port while
   another process (the live service, the Python app, the Rust app) may hold it.
5. **Preserve user data.** Never delete or overwrite `config.json`, the inbox directory, or recordings.
   Treat the sample recordings in the repo root as read-only fixtures.

---

## 2. Rust Guardrails (`rust/` workspace)

* **Parity before features.** No new user-facing features until the Phase 7 exit gate. Each port must
  match its Phase 0 golden fixture in `rust/fixtures/`. If a test disagrees with a fixture, fix the code.
  Never regenerate the fixture to make a test pass without writing down why.
* **Error handling:**
  * No `unwrap()` / `expect()` / `panic!` in non-test code (enforced by workspace clippy lints).
    `expect()` is allowed only for real invariants, with a message explaining why it can't fail.
  * Libraries return `thiserror` enums. `anyhow` is only allowed in `bins/` and `xtask/`.
  * Never discard a `Result` with `let _ =` unless a comment explains why that is safe.
* **Real-time audio callback rules (cpal callbacks):** no heap allocation, no `Mutex`/`RwLock`, no
  logging, no file/network/serial I/O, no channel that can block. Only lock-free SPSC (`rtrb`) and atomics.
  All other work happens on worker threads.
* **Concurrency & lifecycles:**
  * Every spawned task/thread must be tracked (`JoinSet`, `JoinHandle` stored, or `TaskTracker`) and must
    stop via `CancellationToken` on shutdown. No fire-and-forget `tokio::spawn`.
  * Never block inside async code: serial reads, LAME encoding and file writes go through
    `spawn_blocking` or dedicated threads.
  * One failing scanner must never stall the other. Poll each scanner on its own.
* **Pure core, thin I/O.** Protocol codecs, metadata, formatters, VOX and exclusion logic must be pure,
  deterministic functions with no I/O, so they can be tested against fixtures. Hardware access goes
  through the `Transport` trait and cpal wrappers.
* **`unsafe`** is forbidden unless a `// SAFETY:` comment justifies it and the PR/handoff entry calls it out.
* **Logging:** use `tracing` with structured fields (`scanner_id`, `tgid`, `freq`). No `println!` outside
  CLI output.
* **Wire compatibility:** `config.json` keys/enum strings, REST paths, `/ws` message shapes and the feeder's
  `CallTransmissionPayload` JSON must stay compatible with the existing Python versions and `static/` UI
  until the plan says otherwise.
* **Quality gate before every handoff:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.

---

## 3. Python Legacy Guardrails (reference implementation)

The Python code is **frozen to bug fixes only** while the port is in progress. It is the behavioural
reference and the production fallback.

* Any Python bug fix **must** be mirrored in the Rust plan/backlog, and fixtures must be regenerated through
  `tools/gen_fixtures.py` if the output changes. Note this in the handoff log.
* Don't add new features, dependencies, or refactors to the Python tree.
* Don't widen `except Exception:` usage. When touching a block, narrow it or log it with context.
* Run `python -m pytest tests/` before handing off any Python change.

---

## 4. Inter-Model Handoff Protocol (Mandatory)

Work is spread across models, sessions and machines, so shared memory lives in
[`ACTIVE_CONTEXT.md`](ACTIVE_CONTEXT.md).

1. **Session start:** read `AGENTS.md`, then `ACTIVE_CONTEXT.md`, then the current phase section of
   [`Plan/rust_conversion_master_plan.md`](Plan/rust_conversion_master_plan.md).
2. **During work:** stay inside the current phase's scope. If you find a spec/code discrepancy or need
   a design decision, record it under *Open Questions* instead of guessing.
3. **Session end:** append a handoff entry to `ACTIVE_CONTEXT.md` (newest at the bottom) using this template,
   and tick completed checkboxes in the master plan:

```markdown
### YYYY-MM-DD HH:MM | <Model name> (<Machine: Windows Dev / Linux Pi / ...>)
- **Phase:** <e.g. Phase 1 — sc-metadata>
- **Work Performed:**
  - <what changed, with file links>
- **Intentional Behaviour Deviations from Python:** <none | list + rationale>
- **Live Capture Impact:** <none | describe>
- **Verification Performed:** <commands run + results, e.g. `cargo test --workspace`: 84 passed>
- **Gotchas / Warnings for Next Model:**
  - <...>
- **Next Tasks / Unfinished Work:** <...>
```

4. **Never claim verification you did not run.** If hardware wasn't available, write "not hardware-tested".

---

## 5. Repository Map

| Path | Purpose |
|---|---|
| `Plan/` | Migration master plan and phase notes |
| `rust/` | Rust Cargo workspace (target architecture) |
| `rust/fixtures/` | Golden fixtures generated from Python + real ProScan files |
| `config.py`, `drivers/`, `discovery/`, `audio/`, `engine/`, `feeder/`, `metadata/` | Python reference implementation |
| `server.py`, `static/` | FastAPI backend + web UI (UI is reused by the Rust server) |
| `gui.py` | CustomTkinter desktop GUI (to be replaced in Phase 6) |
| `tests/` | Python pytest suite |
| `*.md` (root) | Reverse-engineered ProScan specs (source of truth) |
| `install.sh`, `scanscribe-scanner.service`, `Dockerfile` | Deployment |
