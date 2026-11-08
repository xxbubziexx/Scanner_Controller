# Scanner_Controller Active Context & Inter-Model Blackboard

This living document is the **shared memory** between AI models, human developers, and workstations
(Windows dev machines and the Linux/Raspberry Pi live node).

> **MANDATORY FOR ALL AGENTS:**
> 1. Read [`AGENTS.md`](AGENTS.md) and this file at the start of every session.
> 2. Append a new entry to the **Handoff Log** below when you finish your task or switch models/machines.

---

## 1. System Health & Current State

- **Current Architecture Phase:** Phase 0, Phase 1 & Phase 2 complete; entering **Phase 3: `sc-audio` (Real-Time Audio Pipeline)**
  (see [`Plan/phase_3_audio_pipeline.md`](Plan/phase_3_audio_pipeline.md)).
- **Production Stack:** Python (FastAPI `server.py` headless / CustomTkinter `gui.py`), commit `7ed6a22`.
- **Python Test Baseline:** `59 passed in 1.16s` (`python -m pytest tests/`, Python 3.11.9, tagged `python-baseline-v1`).
- **Rust Toolchain:** rustc/cargo 1.99.1 (`stable-x86_64-pc-windows-gnu` host, `aarch64-unknown-linux-gnu` target installed).
- **Active Focus:** Phase 3 — `sc-audio` (`PreRollRingBuffer`, VOX gate, resampler, `cpal` capture + loopback mixer via `rtrb`, WAV/MP3 encoders).

---

## 2. In-Progress Features & Next Objectives

- [x] Tag `python-baseline-v1` on commit `7ed6a22` (59 passed in 1.55s).
- [x] Write `tools/gen_fixtures.py` and commit `rust/fixtures/` (metadata, formatters, drivers, audio, feeder, proscan samples).
- [x] Scaffold `rust/` workspace with shared lints (`unwrap_used = "deny"`, clippy priority) + CI (`.github/workflows/rust_ci.yml`).
- [x] Target check: verified `aarch64-unknown-linux-gnu` cross-compilation passes cleanly.
- [x] Implement `sc-core`: `ScannerModel` (34 models), `AppConfig`, `ScannerConfig`, `ScannerStatus`, deserialize `config.json`.
- [x] Implement `sc-metadata`: `build_pros_chunk`, `parse_pros_chunk`, ID3v2.3 tag writer, RIFF `LIST`/`INFO`, 37 `%` formatters.
- [x] Verify 100% byte parity against `rust/fixtures/metadata/*.bin` and `rust/fixtures/formatters.json`.
- [x] Add property-based testing (`proptest`) for `pros` chunk roundtrip (10,000 cases passed).
- [x] Implement `sc-drivers`: `Transport` trait (`SerialTransport`, `TcpTransport`, `UdpTransport`, `ReplayTransport`), DMA parser/formatter, HomePatrol parser/formatter (`quick-xml`), Classic parser/formatter, 34-model `DriverFactory`.
- [x] Implement `sc-discovery`: serial COM port scanner (with `active_ports` guard and VID/PID metadata) and LAN UDP detector (`SUS\r` on UDP 50536).
- [x] Implement `scanner-controller` CLI binary with `detect` and `probe-keypad` subcommands.
- [ ] Implement `sc-audio`: `PreRollRingBuffer`, `VoxGate`, de-click ramps, resampler, `cpal` + `rtrb` capture/loopback, WAV/MP3 encoders.

## 3. Open Questions / Decisions Pending

1. Desktop UI: Tauri reusing `static/` (recommended) vs `egui`.
2. **Serial I/O (Resolved in Phase 2 Spike):** Chose `serialport = { version = "4.7", default-features = false }` with a synchronous `Transport` trait (`&mut self`) running on a dedicated thread per scanner (`spawn_blocking` / worker thread).
   - *Rationale:* (a) `default-features = false` eliminates the `libudev` C dependency on Linux, allowing `aarch64-unknown-linux-gnu` cross-compilation from Windows/Ubuntu with zero C sysroot dependencies while still enumerating `/sys/class/tty` on Linux and Win32 SetupAPI on Windows; (b) synchronous `&mut self` avoids per-poll `Box<dyn Future>` heap allocations at 150 ms intervals and gives direct access to `write_request_to_send(true)`, `write_data_terminal_ready(true)`, `clear(ClearBuffer::Input)`, and `bytes_to_read()` for the 40ms/20ms pipelined batch drain.
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

### 2026-11-08 14:00 | Gemini 3.8 Flash (Windows Dev Workstation)
- **Phase:** Phase 0 (Baseline, Fixtures & Scaffolding) -> Phase 1 Entry
- **Work Performed:**
  - Verified Python test suite baseline: `59 passed in 1.55s` on Python 3.11.9.
  - Tagged `python-baseline-v1` on git commit `7ed6a22`.
  - Installed Rust toolchain (rustc/cargo 1.99.1, `stable-x86_64-pc-windows-gnu` host, `aarch64-unknown-linux-gnu` target).
  - Implemented `tools/gen_fixtures.py` and generated golden test vectors in `rust/fixtures/` (23 metadata binary blobs, formatters.json matrix with 45 cases, 5 driver transcripts, audio & VOX fixtures, feeder schemas, and 5 ProScan sample files).
  - Scaffolded Cargo workspace `rust/` with `sc-core`, `sc-metadata`, and `xtask`. Configured workspace lints with strict clippy priorities and `unwrap_used = "deny"`.
  - Created `.github/workflows/rust_ci.yml` verifying formatting, clippy, and compilation on Windows, Ubuntu, and ARM64 Pi target.
  - Verified `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo check --target aarch64-unknown-linux-gnu` all pass with zero warnings.
- **Intentional Behaviour Deviations from Python:** None.
- **Live Capture Impact:** None.
- **Verification Performed:**
  - `python -m pytest tests/`: 59 passed in 1.55s.
  - `python tools/gen_fixtures.py`: all fixtures written cleanly.
  - `cargo fmt --all -- --check`: 0 diffs.
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: passed cleanly.
- **Gotchas / Warnings for Next Model:**
  - Workspace target directory is git-ignored via `rust/.gitignore`.
- **Next Tasks / Unfinished Work:** Begin Phase 1 Step 1 — implement `sc-core` domain models and serialize/deserialize against `config.json`.

### 2026-11-08 14:35 | Claude 3.7 Sonnet (Windows Dev Workstation)
- **Phase:** Phase 1 — `sc-core` & `sc-metadata` (Complete)
- **Work Performed:**
  - Implemented `sc-core`:
    - [`rust/crates/sc-core/src/enums.rs`](rust/crates/sc-core/src/enums.rs): 34 `ScannerModel` variants, `ConnectionMode`, `LockoutMethod`, `PriorityMode`, `ExclusionMode`, `AudioChannelMode`, `FeederMode`, model-specific baud rate validator, `ALLOWED_SAMPLE_RATES`.
    - [`rust/crates/sc-core/src/status.rs`](rust/crates/sc-core/src/status.rs): `ScannerStatus` struct and `normalize_frequency` (ported from `drivers/base_driver.py`).
    - [`rust/crates/sc-core/src/config.rs`](rust/crates/sc-core/src/config.rs): `ScannerConfig`, `ScanScribeFeederConfig`, `SyncConfig`, `AudioLoopbackConfig`, `AppConfig` with serde JSON serializers and validators.
    - Verified loading and roundtripping the real production `config.json`.
  - Implemented `sc-metadata`:
    - [`rust/crates/sc-metadata/src/record.rs`](rust/crates/sc-metadata/src/record.rs): `ProScanMetadata` with all 22 fields + serde aliases.
    - [`rust/crates/sc-metadata/src/clean.rs`](rust/crates/sc-metadata/src/clean.rs): `clean_ascii_1` (ProScan printable ASCII filter & max 253 truncation).
    - [`rust/crates/sc-metadata/src/pros.rs`](rust/crates/sc-metadata/src/pros.rs): `build_pros_chunk` & `parse_pros_chunk` (spec §1 exact 22-item run, length backpatching, fixed padding, odd-length pad byte). Added `sanitize_field` for robust ASCII sanitization.
    - [`rust/crates/sc-metadata/src/id3.rs`](rust/crates/sc-metadata/src/id3.rs): `encode_syncsafe`, `build_id3v23_text_frame`, `write_mp3_id3v23` (spec §2 ID3v2.3 writer with template gating).
    - [`rust/crates/sc-metadata/src/riff.rs`](rust/crates/sc-metadata/src/riff.rs): `build_riff_info_subchunk`, `write_wav_riff_info` (spec §3-4 RIFF LIST/INFO + pros chunk).
    - [`rust/crates/sc-metadata/src/formatters.rs`](rust/crates/sc-metadata/src/formatters.rs): `format_template` (37 formatters in exact `General.cs:33805` order), `format_streaming_metadata` (Engine B), `sanitize_filename`, `sanitize_folder_path`.
  - Added property-based testing (`proptest`) running 10,000 cases of roundtrip build/parse on `pros` chunk.
  - Added comprehensive test asserting byte-for-byte exact equality on all 23 binary metadata fixtures in `rust/fixtures/metadata/`.
- **Intentional Behaviour Deviations from Python:** None. Byte-for-byte exact compatibility maintained across all 23 binary fixtures and 45 template test cases.
- **Live Capture Impact:** None.
- **Verification Performed:**
  - `cargo test --workspace`: all 11 tests passing (6 in `sc-core`, 5 in `sc-metadata` including 10,000 proptest cases).
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings, clean.
  - `cargo doc --workspace --no-deps`: built cleanly.
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: clean (ARM64 cross-compile OK).
- **Gotchas / Warnings for Next Model:**
  - When running `cargo` in PowerShell sessions on this dev machine, ensure `$env:PATH` contains `$env:USERPROFILE\.cargo\bin` and the LLVM MinGW directory (`C:\Users\MAC10\AppData\Local\Microsoft\WinGet\Packages\MartinStorsjo.LLVM-MinGW.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\llvm-mingw-20250114-ucrt-x86_64\bin`).
- **Next Tasks / Unfinished Work:** Begin Phase 2 — implement `sc-drivers` and `sc-discovery` per [`Plan/phase_2_drivers_and_discovery.md`](Plan/phase_2_drivers_and_discovery.md).

### 2026-11-08 15:15 | Claude 3.7 Sonnet (Windows Dev Workstation)
- **Phase:** Phase 2 — `sc-drivers`, `sc-discovery` & CLI `scanner-controller` (Complete)
- **Work Performed:**
  - **Serial I/O Decision Spike:** Selected `serialport = { version = "4.7", default-features = false }` with a synchronous `Transport` trait (`&mut self`) executed on a dedicated thread per scanner. Disabling `default-features` removes the Linux `libudev` C dependency so `aarch64-unknown-linux-gnu` cross-compiles cleanly with zero external C sysroot dependencies, while preserving `/sys/class/tty` enumeration on Linux and Win32 SetupAPI on Windows.
  - **Implemented `sc-drivers`:**
    - [`rust/crates/sc-drivers/src/transport.rs`](rust/crates/sc-drivers/src/transport.rs): `Transport` trait, `SerialTransport` (asserts RTS/DTR, flushes input buffer on open, 40ms/20ms pipelined batch drain), `TcpTransport`, `UdpTransport` (UDP 50536 for SDS200/BCD536HP), `ReplayTransport` (fixture replay + command recording), `normalize_port_name`.
    - [`rust/crates/sc-drivers/src/parse_util.rs`](rust/crates/sc-drivers/src/parse_util.rs): `extract_section` and `now_unix_seconds`.
    - [`rust/crates/sc-drivers/src/dma.rs`](rust/crates/sc-drivers/src/dma.rs): Pure DMA parser (`parse_dma_response` for `MDL`, `PWR`, `GLG`, `STS`, DMR color code / NXDN RAN / CTCSS / DCS tone decoding) + `DmaDriver<T>` (`poll`, `send_skip`, `temporary_lockout`, `release_lockout`, `should_try_connect`).
    - [`rust/crates/sc-drivers/src/hp.rs`](rust/crates/sc-drivers/src/hp.rs): Pure HomePatrol parser (`parse_hp_response` via `quick-xml` for `GSI,1` `<ScannerInfo>`, `parse_sts_fallback` for LCD `____` dividers, `clean_display_text`, encrypted `P25Status="Enc"` activity trigger, and seamless fallback for DMA scanners misconfigured as BCD436HP) + `HpDriver<T>` (`send_skip` using `KEY,>,P` -> `KEY,>, P` -> `KEY,Z,P` toggle and NEVER sending `KEY,S,P`; `dismiss_menu` `KEY,.,P`; `temporary_lockout` `KEY,V,P` -> `KEY,A,L`).
    - [`rust/crates/sc-drivers/src/classic.rs`](rust/crates/sc-drivers/src/classic.rs): Pure Classic parser (`parse_classic_response` for `STS`/`STC`) + `ClassicDriver<T>`.
    - [`rust/crates/sc-drivers/src/factory.rs`](rust/crates/sc-drivers/src/factory.rs): `DriverFamily`, `driver_family_for_model` (all 34 `ScannerModel` variants), `AnyScannerDriver<T>`, `HardwareTransport`, `create_driver_for_model`, `create_hardware_driver`.
  - **Implemented `sc-discovery`:**
    - [`rust/crates/sc-discovery/src/lib.rs`](rust/crates/sc-discovery/src/lib.rs): `CANDIDATE_BAUDS`, `DetectedPort` (with optional `usb_vid`/`usb_pid`), `DiscoveredLanScanner`, `parse_mdl_response`, `parse_si_response`, `extract_port_digits`, `parse_sus_response`, `detect_serial_ports` (honors Prime Directive 4 by skipping `active_ports`), `discover_lan_scanners` (UDP 50536 broadcast).
  - **Implemented CLI binary `scanner-controller`:**
    - [`rust/bins/scanner-controller/src/main.rs`](rust/bins/scanner-controller/src/main.rs): `scanner-controller detect [--lan] [--active-ports ...] [--json]` and `scanner-controller probe-keypad --port <PORT> --baud <BAUD> [--keys ...]` (port of `tools/probe_keypad.py`).
  - **Fixed Phase 0 Driver Fixture Generator Bug:**
    - In Phase 0, [`tools/gen_fixtures.py`](tools/gen_fixtures.py) had hand-typed dicts in `generate_driver_fixtures()` instead of invoking `BCD996P2Driver.poll()` and `BCD436HPDriver.poll()`, causing `dma_trunk_expected.json` to have `"frequency": 85.35` (instead of `853.5` from `normalize_frequency(85350000)`) and `dma_scanning_expected.json` to have `"channel_name": ""` (instead of `"Scanning..."`). Updated `generate_driver_fixtures()` to run the actual Python drivers and regenerated `rust/fixtures/drivers/*_expected.json`.
- **Intentional Behaviour Deviations from Python:** None. `DetectedPort` includes optional `usb_vid`/`usb_pid` fields skipped when `None` in JSON serialization.
- **Live Capture Impact:** None. Hardware soak test on real BCD996P2 + BCD436HP is marked **not hardware-tested** until hardware is connected.
- **Verification Performed:**
  - `cargo test --workspace`: 22 passed (`6` in `sc-core`, `5` in `sc-discovery`, `6` in `sc-drivers`, `5` in `sc-metadata` including 10,000 `proptest` cases).
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: clean (ARM64 Pi cross-check passed).
  - `cargo run --bin scanner-controller -- --help`: verified CLI subcommands (`detect`, `probe-keypad`).
  - `python -m pytest tests/`: 59 passed in 1.16s.
- **Gotchas / Warnings for Next Model:**
  - On this Windows workstation, two MinGW packages exist under `%LOCALAPPDATA%\Microsoft\WinGet\Packages\`. Always prepend `C:\Users\MAC10\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin` (which provides `libgcc_eh` + `dlltool.exe` required by the `stable-x86_64-pc-windows-gnu` toolchain when linking proc-macro DLLs) ahead of `MartinStorsjo.LLVM-MinGW.UCRT`.
- **Next Tasks / Unfinished Work:** Begin Phase 3 — implement `sc-audio` per [`Plan/phase_3_audio_pipeline.md`](Plan/phase_3_audio_pipeline.md).
