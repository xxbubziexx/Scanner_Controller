# Scanner_Controller Active Context & Inter-Model Blackboard

This living document is the **shared memory** between AI models, human developers, and workstations
(Windows dev machines and the Linux/Raspberry Pi live node).

> **MANDATORY FOR ALL AGENTS:**
> 1. Read [`AGENTS.md`](AGENTS.md) and this file at the start of every session.
> 2. Append a new entry to the **Handoff Log** below when you finish your task or switch models/machines.

---

## 1. System Health & Current State

- **Current Architecture Phase:** Python → Rust migration **planned**, Phase 0 baseline verified
  (see [`Plan/rust_conversion_master_plan.md`](Plan/rust_conversion_master_plan.md)).
- **Production Stack:** Python (FastAPI `server.py` headless / CustomTkinter `gui.py`), commit `7ed6a22`.
- **Python Test Baseline:** `59 passed in 1.44s` (`python -m pytest tests/`, Python 3.11.9).
- **Active Focus:** Phase 0: baseline tag, golden fixture generator, Rust workspace scaffold, CI.

---

## 2. In-Progress Features & Next Objectives

- [ ] Tag `python-baseline-v1` and record the pytest pass count above.
- [ ] Write `tools/gen_fixtures.py` and commit `rust/fixtures/`.
- [ ] Scaffold `rust/` workspace with shared lints + CI (Windows, Ubuntu, aarch64 check).
- [ ] Spike: `mp3lame-encoder` cross-compile for `aarch64-unknown-linux-gnu`.

## 3. Open Questions / Decisions Pending

1. Desktop UI: Tauri reusing `static/` (recommended) vs `egui`.
2. Serial I/O: blocking thread-per-scanner vs `tokio-serial`.
3. LAN/RTSP audio: parity scope or post-cutover?
4. Poll-failure semantics: Python drops *both* scanners' results if either poll fails
   (`engine/sync_manager.py:243-251`). Proposed: poll each scanner on its own in Rust.
5. Gemini's 3-phase split (2026-10-21) was reverted to P0–P7 with per-phase files (see log, 2026-10-22).
   Please confirm the user wants the one-file-per-phase layout to be permanent.

---

## 4. Model Handoff Log

### 2026-10-11 16:35 | Claude Opus 5.5 (Windows Dev Workstation)
- **Phase:** Pre-Phase 0 (planning)
- **Work Performed:**
  - Created [`Plan/rust_conversion_master_plan.md`](Plan/rust_conversion_master_plan.md): 8-phase (P0–P7)
    strangler migration with module→crate mapping, dependency candidates, exit gates, risk register.
  - Created [`AGENTS.md`](AGENTS.md) (prime directives, Rust/Python guardrails, handoff protocol) and this file.
- **Intentional Behaviour Deviations from Python:** None (no code changed).
- **Live Capture Impact:** None.
- **Verification Performed:** Documentation only. No tests run.
- **Gotchas / Warnings for Next Model:**
  - Rust toolchain (`cargo`, `rustc`, `rustup`) is not yet installed on this Windows dev workstation. Needs `rustup-init.exe` / `winget install Rustlang.Rustup`.
  - Device indices in `config.json` are PortAudio (`sounddevice`) indices and won't map 1:1 to `cpal`.
    Plan to store device names.
  - Plan crate choices are candidates. Pin exact versions and confirm in Phase 0.
- **Next Tasks / Unfinished Work:** Begin Phase 0 (see §2 above).

### 2026-10-21 19:46 | Gemini 3.8 Flash (Windows Dev Workstation)
- **Phase:** Pre-Phase 1 (planning & architecture breakdown)
- **Work Performed:**
  - Structured the migration master plan into 3 dedicated, modular markdown plans:
    - [`Plan/phase_1_core_and_metadata.md`](Plan/phase_1_core_and_metadata.md): Baseline freeze, fixtures, workspace scaffolding, `sc-core`, and `sc-metadata`.
    - [`Plan/phase_2_drivers_and_audio.md`](Plan/phase_2_drivers_and_audio.md): `sc-drivers`, `sc-discovery`, and real-time `sc-audio` pipeline.
    - [`Plan/phase_3_engine_server_cutover.md`](Plan/phase_3_engine_server_cutover.md): `sc-engine`, `sc-feeder`, `sc-server`, `sc-desktop`, and production cutover.
- **Intentional Behaviour Deviations from Python:** None.
- **Live Capture Impact:** None.
- **Verification Performed:** Verified Python test baseline: `59 passed in 1.42s`.
- **Gotchas / Warnings for Next Model:**
  - Rust toolchain (`cargo`, `rustc`) must be installed on this Windows workstation before building the workspace.
  - Phase 1 execution begins with Step 1 (generating `rust/fixtures/` via `tools/gen_fixtures.py`) to provide golden test fixtures for `sc-metadata`.
- **Next Tasks / Unfinished Work:** Install Rust toolchain; run Step 1 of Phase 1 (`tools/gen_fixtures.py` and workspace scaffold).


### 2026-10-22 19:47 | Claude Sonnet 5.5 (Windows Dev Workstation)
- **Phase:** Pre-Phase 0 (plan restructure)
- **Work Performed:**
  - Reviewed Gemini's 3-file split and replaced it with one file per phase using the master plan's P0-P7 numbering: [`phase_0`](Plan/phase_0_baseline_and_scaffolding.md), [`phase_1`](Plan/phase_1_core_and_metadata.md), [`phase_2`](Plan/phase_2_drivers_and_discovery.md), [`phase_3`](Plan/phase_3_audio_pipeline.md), [`phase_4`](Plan/phase_4_engine_and_feeder.md), [`phase_5`](Plan/phase_5_server_and_shadow_run.md), [`phase_6`](Plan/phase_6_desktop_app.md), [`phase_7`](Plan/phase_7_packaging_and_cutover.md). The master plan now links to them. `phase_2_drivers_and_audio.md` and `phase_3_engine_server_cutover.md` were renamed and rewritten.
  - Restored gates Gemini dropped: API contract fixtures, feeder byte-identity, fault injection, `gui.py` audit checklist.
  - Moved the LAME aarch64 spike to Phase 0. The serial spike now precedes the `Transport` design in Phase 2.
  - Fixed Phase 1 metadata items against `RECORDING-METADATA-FORMAT.md`: ID3 frames are `TSEE/TRDA/TIT2/TPE1/TPE2` + raw `pros` (not `TALB/COMM/TXXX`); `pros` is a length-prefixed run of 22 `Key:Value\0` strings (no fixed 40/48 byte size); the MP3 tag and `pros` are gated on TIT2/TPE1/TPE2 being non-empty, while WAV is unconditional.
- **Intentional Behaviour Deviations from Python:** None (documentation only). The independent-poll fix remains a pending decision (Phase 4).
- **Live Capture Impact:** None.
- **Verification Performed:** Documentation only. Spec claims were read from the `.md` spec and not cross-checked against `proscan_metadata.py`. No tests run.
- **Gotchas / Warnings for Next Model:**
  - Phase 5's route list is copied from the master plan; grep could not extract routes from `server.py`. Diff against the real file first.
- **Next Tasks / Unfinished Work:** Install Rust toolchain; begin Phase 0.
