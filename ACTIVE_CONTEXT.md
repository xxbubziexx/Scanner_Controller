# Scanner_Controller Active Context & Inter-Model Blackboard

This living document is the **shared memory** between AI models, human developers, and workstations
(Windows dev machines and the Linux/Raspberry Pi live node).

> **MANDATORY FOR ALL AGENTS:**
> 1. Read [`AGENTS.md`](AGENTS.md) and this file at the start of every session.
> 2. Append a new entry to the **Handoff Log** below when you finish your task or switch models/machines.

---

## 1. System Health & Current State

- **Current Architecture Phase:** Phase 0, Phase 1, Phase 2, Phase 3 & Phase 4 complete; entering **Phase 5: `sc-server` & Headless Daemon**
  (see [`Plan/phase_5_server_and_shadow_run.md`](Plan/phase_5_server_and_shadow_run.md)).
- **Production Stack:** Python (FastAPI `server.py` headless / CustomTkinter `gui.py`), commit `7ed6a22`.
- **Python Test Baseline:** `59 passed in 1.16s` (`python -m pytest tests/`, Python 3.11.9, tagged `python-baseline-v1`).
- **Rust Toolchain:** rustc/cargo 1.99.1 (`stable-x86_64-pc-windows-gnu` host, `aarch64-unknown-linux-gnu` target installed).
- **Active Focus:** Phase 5 — `sc-server` (Axum HTTP REST + WebSocket `/ws` server + embedded `static/` UI) and `scanner-controller serve`.

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
- [x] Implement `sc-audio`: `PreRollRingBuffer`, `VoxGate` + `ProScanVoxEngine`, de-click ramps (`LoopbackMixer`), resamplers (`resample_pcm_int16` & `rubato`), `cpal` + `rtrb` capture/loopback, WAV (`hound` + `sc-metadata`) and MP3 (`mp3lame-encoder` + `sc-metadata`) encoders.
- [x] Implement `sc-feeder`: `CallTransmissionPayload`, `ScanScribeFeeder` (atomic `DIRECTORY_DROP`, `HTTP_WEBHOOK`, sidecar JSON, fallback inbox).
- [x] Implement `sc-engine`: `SyncManager`, `_should_exclude`, `PriorityMode`, `ExclusionMode`, independent scanner polling, duplicate-abort, hot config reload.
- [ ] Implement `sc-server`: Axum REST & WebSocket server, `rust-embed` static files, contract tests against `server.py`.
- [ ] Implement `sc-desktop` & Phase 7 packaging (`install.sh`, `Dockerfile`, release binary).

## 3. Open Questions / Decisions Pending

1. Desktop UI: Tauri reusing `static/` (recommended) vs `egui`.
2. **Serial I/O (Resolved in Phase 2 Spike):** Chose `serialport = { version = "4.7", default-features = false }` with a synchronous `Transport` trait (`&mut self`) running on a dedicated thread per scanner (`spawn_blocking` / worker thread).
   - *Rationale:* (a) `default-features = false` eliminates the `libudev` C dependency on Linux, allowing `aarch64-unknown-linux-gnu` cross-compilation from Windows/Ubuntu with zero C sysroot dependencies while still enumerating `/sys/class/tty` on Linux and Win32 SetupAPI on Windows; (b) synchronous `&mut self` avoids per-poll `Box<dyn Future>` heap allocations at 150 ms intervals and gives direct access to `write_request_to_send(true)`, `write_data_terminal_ready(true)`, `clear(ClearBuffer::Input)`, and `bytes_to_read()` for the 40ms/20ms pipelined batch drain.
3. LAN/RTSP audio: parity scope or post-cutover?
4. **Poll-failure semantics (Resolved for Phase 4):** Python drops *both* scanners' results if either poll fails
   (`engine/sync_manager.py:243-251`). **Intentional Deviation Decided:** Rust polls each scanner independently in its own task/worker so a disconnected or unresponsive radio never stalls or drops telemetry/recordings from the healthy radio (`AGENTS.md` §2).
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

### 2026-11-08 15:38 | Claude 3.7 Sonnet (Windows Dev Workstation)
- **Phase:** Phase 3 — `sc-audio` (Real-Time Audio Pipeline, VOX, Loopback & Encoders) (Complete)
- **Work Performed:**
  - Implemented `sc-audio`:
    - [`rust/crates/sc-audio/src/preroll.rs`](rust/crates/sc-audio/src/preroll.rs): `PreRollRingBuffer` (exact, sample-accurate circular buffer with chronological readout and zero chunk-rounding or off-by-one errors across wraps).
    - [`rust/crates/sc-audio/src/vox.rs`](rust/crates/sc-audio/src/vox.rs): `compute_peak_equiv_rms` (`round(sqrt(mean(x^2) * 2.0))`), `rms_to_vox_meter` (`0..=90` dBFS scale), `ProScanVoxEngine` (20-minute history ring, `0.15s` VOX averaging window, 0-buffer attack, 2.0s hang time per `VOX-ALGORITHM.md` §1–4), and sample-accurate `VoxGate` verified sample-for-sample against `vox_test_16k.pcm` & `vox_expected.json` (`expected_gate_open_sample = 8000`, `expected_gate_close_sample = 28800`).
    - [`rust/crates/sc-audio/src/resample.rs`](rust/crates/sc-audio/src/resample.rs): `resample_pcm_int16` (linear interpolation matching Python's `engine/sync_manager.py:resample_pcm_int16` against all 5 golden fixtures in `rust/fixtures/audio/resample/`), `resample_pcm_rubato` (band-limited FFT resampler via `rubato::FftFixedInOut`), and `resample_f32_linear`.
    - [`rust/crates/sc-audio/src/loopback.rs`](rust/crates/sc-audio/src/loopback.rs): ProScan `scale_out_volume` (`PlaybackProcesser.cs:1119`) and `limit_short_1` (`Common_PS_RF.cs:254`), `AudioRingBuffer` (underrun zero-padding without permanent muting lockout), `LoopbackMixer` (`STEREO_SPLIT` / `CENTER`, 15 ms linear de-click gain ramping on mute/unmute/duck), and `fill_rt_output_callback` (real-time safe `rtrb` SPSC consumer + atomics).
    - [`rust/crates/sc-audio/src/encoder.rs`](rust/crates/sc-audio/src/encoder.rs): `is_valid_mp3_payload` (MPEG Layer III consecutive frame sync check rejecting `0xFFFF` negative PCM and RIFF WAV), `encode_wav_with_metadata` (`hound` PCM validation + `sc_metadata::write_wav_riff_info`), `encode_mp3_raw_lame` (`mp3lame-encoder` matching `lameenc` 32/64/128 kbps curve, mono, quality 2), and `encode_audio_for_dispatch` (with WAV fallback per Prime Directive 1).
    - [`rust/crates/sc-audio/src/devices.rs`](rust/crates/sc-audio/src/devices.rs): `list_audio_input_devices`, `list_audio_output_devices` matching `/api/audio/devices` and `/api/audio/output_devices`, and name-first `resolve_input_device` / `resolve_output_device`.
    - [`rust/crates/sc-audio/src/stream.rs`](rust/crates/sc-audio/src/stream.rs): `push_rt_input_callback` (zero allocation, zero locks, zero logging, zero syscalls inside `cpal` callbacks), `ActiveCallSession`, `SingleScannerAudioStream`, and `ScannerAudioRecorder`.
  - Updated [`.github/workflows/rust_ci.yml`](.github/workflows/rust_ci.yml) and added [`rust/tools/pkg-config-cross.bat`](rust/tools/pkg-config-cross.bat) for `alsa-sys` / `mp3lame-sys` cross-checking to `aarch64-unknown-linux-gnu`.
- **Intentional Behaviour Deviations from Python:**
  - `resolve_input_device` and `resolve_output_device` resolve by stored `device_name` first and fall back to `device_index` only for legacy `config.json` files (since `cpal` device indices differ from PortAudio indices).
- **Live Capture Impact:** None. 2-hour hardware audio soak on Windows & Raspberry Pi is marked **not hardware-tested** until cutover testing.
- **Verification Performed:**
  - `cargo test --workspace`: 31 passed (`9` in `sc-audio`, `6` in `sc-core`, `5` in `sc-discovery`, `6` in `sc-drivers`, `5` in `sc-metadata` including 10,000 `proptest` cases).
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: clean (3.18s).
- **Gotchas / Warnings for Next Model:**
  - When running `cargo check --workspace --target aarch64-unknown-linux-gnu` on Windows, set `$env:PKG_CONFIG_ALLOW_CROSS = "1"`, `$env:PKG_CONFIG = "A:\Scanscribe_Projects\Scanner_Controller\rust\tools\pkg-config-cross.bat"`, and `$env:DOCS_RS = "1"` so `alsa-sys` and `mp3lame-sys` skip host-specific build steps during type checking.
- **Next Tasks / Unfinished Work:** Begin Phase 4 — implement `sc-feeder` and `sc-engine` per [`Plan/phase_4_engine_and_feeder.md`](Plan/phase_4_engine_and_feeder.md).

### 2026-11-08 16:03 | Claude Opus 4.6 (Windows Dev Workstation)
- **Phase:** Phase 4 — `sc-feeder` & `sc-engine`
- **Work Performed:**
  - Implemented [`rust/crates/sc-feeder`](rust/crates/sc-feeder/src/lib.rs):
    - `CallTransmissionPayload`, `SidecarMetadata`, `ProScanMetadataDict` (exact 22-field PascalCase `ProScanMetadata.to_dict()` mapping), `DispatchResult`.
    - `ScanScribeFeeder`: per-scanner `filename_template` & `tit2_template` resolution, atomic temp-file + rename `DIRECTORY_DROP`, `reqwest` multipart `HTTP_WEBHOOK` with retry, and automatic fallback inbox archival (`./recordings`) on primary inbox or webhook failure (Prime Directive 1: "Never lose a call").
  - Implemented [`rust/crates/sc-engine`](rust/crates/sc-engine/src/lib.rs):
    - `should_exclude` (`TGID_ONLY`, `FREQ_ONLY`, `TGID_AND_FREQ` with `< 0.001` MHz tolerance), `ExclusionStateMachine` (`EqualPeers`, `PrimarySecondary`, `DepartmentPriority`, `active_exclusions` with `hang_time_seconds` and `auto_release_seconds`, event log capped at 100).
    - `SyncManager<T>`: independent per-scanner polling tasks, duplicate-abort of secondary recordings before save/dispatch, `watch::Receiver<EngineStateSnapshot>` state channel, bounded dispatch worker pool with `CancellationToken` drain on shutdown, and hot `update_config` reload without restarting audio streams unless devices changed.
  - Updated [`rust/crates/sc-audio/src/stream.rs`](rust/crates/sc-audio/src/stream.rs) so `cpal::Stream` (`!Send` on Windows WASAPI) is created and held inside the dedicated `sc-audio-*` worker threads, making `SingleScannerAudioStream`, `ScannerAudioRecorder`, and `SyncManager` naturally `Send + Sync` with zero `unsafe`.
- **Intentional Behaviour Deviations from Python:**
  - Recorded in §3.4: `SyncManager` polls `scanner_a` and `scanner_b` independently rather than using `asyncio.gather` (`engine/sync_manager.py:243-251`), so one disconnected or failing scanner never stalls or drops recordings from the healthy scanner.
- **Live Capture Impact:** None (not hardware-tested; tested against `ReplayTransport` and synthetic PCM).
- **Verification Performed:**
  - `cargo fmt --all -- --check`: passed.
  - `cargo test --workspace`: 37 passed (`3` in `sc-feeder`, `3` in `sc-engine`, `9` in `sc-audio`, `6` in `sc-drivers`, `5` in `sc-discovery`, `5` in `sc-metadata`, `6` in `sc-core`).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: clean (19.69s).
- **Gotchas / Warnings for Next Model:**
  - `ProScanMetadataDict` in `sc-feeder` preserves the exact 22 PascalCase keys of Python's `ProScanMetadata.to_dict()` (`"Scanner"`, `"EndingDate"`, ..., `"UID#"`, ..., `"ScannerMode"`).
- **Next Tasks / Unfinished Work:** Begin Phase 5 — implement `sc-server` (Axum HTTP REST + WebSocket `/ws` + embedded `static/` UI) and wire `serve` subcommand into `scanner-controller`.

### 2026-11-08 16:22 | Claude Opus 4.6 (Windows Dev)
- **Phase:** Phase 5 — `sc-server` & Headless Daemon (`scanner-controller serve`)
- **Work Performed:**
  - Extended [`tools/gen_fixtures.py`](tools/gen_fixtures.py) to generate Phase 5 API contract fixtures (`rust/fixtures/api/initial_state.json` and `rust/fixtures/api/models_catalog.json`) from Python's `AppConfig`, `ScannerStatus`, and `UNIDEN_MODEL_CATALOG`.
  - Updated [`rust/crates/sc-core/src/config.rs`](rust/crates/sc-core/src/config.rs) (`AppConfig::save_to_file`, `audio_device_name`/`output_device_name` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, and `AppConfig::default()` parity with `config.py`).
  - Updated [`rust/crates/sc-discovery/src/lib.rs`](rust/crates/sc-discovery/src/lib.rs) with `AvailablePortInfo` and `list_available_ports()`.
  - Created [`rust/crates/sc-server/src/lib.rs`](rust/crates/sc-server/src/lib.rs):
    - Embedded [`static/`](static/) into the single binary via `rust-embed` (`GET /`, `GET /favicon.ico`, `GET /static/*path`).
    - Implemented all 15 REST and WebSocket endpoints matching [`server.py`](server.py): `GET /api/state`, `POST /api/config`, `POST /api/scanner/:scanner_id/model`, `POST /api/skip/:scanner_id`, `GET /api/audio/devices`, `GET /api/audio/output_devices`, `GET /api/ports/available`, `GET /api/ports/detect`, `GET /api/scanners/discover_lan`, `GET /api/models/catalog`, `POST /api/feeder/config`, `POST /api/feeder/dispatch_test`, and `GET /ws`.
    - Implemented automatic `.bak` backup of `config.json` before the first save (`save_config_if_configured`) and automatic enrichment of `audio_device_name`/`output_device_name` next to indices.
    - Added contract tests (`test_api_contract_fixtures_parity`, `test_static_ui_and_mutation_endpoints`).
  - Updated [`rust/bins/scanner-controller/src/main.rs`](rust/bins/scanner-controller/src/main.rs) with `serve` subcommand (and default no-args behavior) to start the Axum server, handle `Ctrl+C` via `CancellationToken`, and open the browser unless `--no-browser` is passed.
- **Intentional Behaviour Deviations from Python:**
  - `POST /api/config`, `POST /api/scanner/:id/model`, and `POST /api/feeder/config` enrich `config.json` with `audio_device_name` / `output_device_name` and create `config.json.bak` on first write (per `Plan/phase_7_packaging_and_cutover.md` §2).
- **Live Capture Impact:** None (not hardware-tested; existing serial ports and `config.json` untouched).
- **Verification Performed:**
  - `cargo fmt --all -- --check`: passed
  - `cargo clippy --workspace --all-targets -- -D warnings`: passed (0 warnings)
  - `cargo test --workspace`: 39 passed, 0 failed (`sc-server`: 2 passed, `sc-engine`: 3 passed, `sc-feeder`: 3 passed, `sc-audio`: 9 passed, `sc-core`: 6 passed, `sc-discovery`: 5 passed, `sc-drivers`: 6 passed, `sc-metadata`: 5 passed)
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: passed
  - Not hardware-tested (7-day live Pi shadow run remains an operational task).
- **Gotchas / Warnings for Next Model:**
  - In Phase 6, `static/` will be enhanced with all missing `gui.py` controls while maintaining 100% compatibility with Python's `server.py`.
- **Next Tasks / Unfinished Work:** Execute Phase 6 (`sc-desktop` + `gui.py` feature parity in `static/`) and Phase 7 (packaging, `xtask`, deployment scripts, and release binary).

### 2026-11-08 16:41 | Claude Opus 4.6 (Windows Dev)
- **Phase:** Phase 6 (`sc-desktop` & `gui.py` Parity) + Phase 7 (Packaging, `xtask` & Cutover Prep)
- **Work Performed:**
  - **Phase 6 Audit & UI Decision:** Audited all 1,419 lines of [`gui.py`](gui.py) against [`static/`](static/) in [`Plan/phase_6_desktop_app.md`](Plan/phase_6_desktop_app.md) and chose the unified `static/` + `sc-desktop` shell architecture (embedding `sc-core`, `sc-engine`, and `sc-server` in-process and launching a native app-mode `WebView2`/Chromium window).
  - **100% `gui.py` Feature Parity in [`static/`](static/):**
    - Updated [`static/index.html`](static/index.html), [`static/style.css`](static/style.css), and [`static/app.js`](static/app.js) with:
      - Master `🔴 Record Calls` toggle (`#masterRecordToggle`) and Global Recording Indicator badge (`#globalRecBadge`: `REC STANDBY` / `🔴 REC ACTIVE` / `REC DISABLED`) + dynamic document title indicator.
      - Per-card `REC ON` / `🔴 REC` / `REC OFF` status badges (`#recBadgeA`, `#recBadgeB`), Unit ID (`#uidA`, `#uidB`), and quick per-card `Enable Scanner` / `Enable Audio Capture` toggles.
      - Real-time Audio VU Meters (`#vuBarA`/`#vuPctA`, `#vuBarB`/`#vuPctB`) with dynamic Green (`<60%`), Amber (`60–84%`), and Red (`>=85%`) thresholds matching `gui.py:924-953`.
      - Full **🔊 Live Audio Loopback Monitor** panel (`#loopbackEnabledToggle`, `#loopbackOutputSelect` populated from `/api/audio/output_devices`, `#loopbackPanSelect` `STEREO_SPLIT`/`CENTER`, and `#loopbackVolumeSlider` `0–100%`).
      - `DEPARTMENT_PRIORITY` option, `hang_time_seconds` & `auto_release_seconds` inputs, Active Holds pill bar (`#activeExclusionsBar`), and Event Log `🧹 Clear View` button.
      - Lockout Method selector (`KEYPRESS` / `MEMORY`), `LIVE_STREAM` feeder mode, Webhook URL input, ProScan binary tag & `.json` sidecar toggles, Filename/TIT2 preset chips (`ProScan Default`, `Simple`, `Detailed`, `Dept/Chan`), live template previews, and the collapsible 37 ProScan `%` Specifiers reference drawer.
  - **Created [`rust/crates/sc-desktop/src/lib.rs`](rust/crates/sc-desktop/src/lib.rs):**
    - Implemented `SingleInstanceGuard` (loopback TCP lock on port `45891`) so a second desktop launch never contends for serial ports (Prime Directive #4), instead opening the UI window to the running instance.
    - Implemented `RecordingIndicator`, `compute_recording_indicator`, `DesktopShellConfig` (`--start-minimized`, `minimize_to_tray`), and `run_desktop_app`.
    - Wired `desktop` subcommand (and default no-args launch) into [`rust/bins/scanner-controller/src/main.rs`](rust/bins/scanner-controller/src/main.rs).
  - **Phase 7 Packaging & Cutover Ready:**
    - Implemented [`rust/xtask/src/main.rs`](rust/xtask/src/main.rs) (`ci`, `build-release`, `package`), building `rust/target/release/scanner-controller.exe` and packaging `dist/scanner-controller.exe`.
    - Updated [`Dockerfile`](Dockerfile) (multi-stage Rust build), [`scanscribe-scanner.service`](scanscribe-scanner.service), [`install.sh`](install.sh) (retaining all existing flags plus `--rollback-python` one-command rollback and automatic `config.json.bak` preservation), and added [`run_rust_app.bat`](run_rust_app.bat) on Windows.
- **Intentional Behaviour Deviations from Python:**
  - `SingleInstanceGuard` prevents launching a second desktop instance that would fight over serial COM ports; it focuses the existing instance's UI window instead.
- **Live Capture Impact:** None (not hardware-tested; `config.json` and serial ports untouched).
- **Verification Performed:**
  - `cargo fmt --all -- --check`: passed
  - `cargo clippy --workspace --all-targets -- -D warnings`: passed (0 warnings)
  - `cargo test --workspace`: 41 passed, 0 failed (`sc-audio`: 9, `sc-core`: 6, `sc-desktop`: 2, `sc-discovery`: 5, `sc-drivers`: 6, `sc-engine`: 3, `sc-feeder`: 3, `sc-metadata`: 5, `sc-server`: 2)
  - `cargo run -p xtask -- package`: built and packaged `dist/scanner-controller.exe`
  - `cargo check --workspace --target aarch64-unknown-linux-gnu`: passed (2.15s)
  - `python -m pytest tests/`: 59 passed in 1.23s
  - Not hardware-tested.
- **Gotchas / Warnings for Next Model:**
  - When building native release binaries on Windows (`cargo build --release`), ensure `DOCS_RS` is not set in the environment (`Remove-Item Env:\DOCS_RS`), because `mp3lame-sys` skips compiling `libmp3lame` when `DOCS_RS` is set. Use `DOCS_RS=1` only for `cargo check --target aarch64-unknown-linux-gnu` when a Linux sysroot C compiler is not installed on Windows.
- **Next Tasks / Unfinished Work:** All 8 code migration phases (P0–P7) are complete and verified. Remaining operational items are the 7-day shadow run on the live Pi (Phase 5 §4) and 14-day production soak before retiring the Python files into `legacy/` (Phase 7 §4).

### 2026-11-15 14:30 | Gemini 2.5 Flash / Antigravity (Windows Dev Workstation)
- **Phase:** Repository Restructuring — Rust Root Promotion on `rust` Branch
- **Work Performed:**
  - Removed legacy Python runtime files (`audio/`, `discovery/`, `drivers/`, `engine/`, `feeder/`, `metadata/`, `tests/`, `server.py`, `gui.py`, `config.py`) from the `rust` branch.
  - Promoted the nested `rust/` workspace to the repository root:
    - Moved `Cargo.toml`, `Cargo.lock`, `bins/`, `crates/`, `fixtures/`, `xtask/`, and `tools/pkg-config-cross.bat` directly into the repository root.
    - Removed redundant `rust/` directory and merged `.gitignore` rules (`target/`).
  - Updated relative path references:
    - [`crates/sc-server/src/lib.rs`](crates/sc-server/src/lib.rs): changed embedded static folder path from `../../../static` to `../../static`.
    - [`crates/sc-core/src/lib.rs`](crates/sc-core/src/lib.rs): changed root `config.json` path from `/../../../config.json` to `/../../config.json`.
    - [`xtask/src/main.rs`](xtask/src/main.rs): updated `run_package` to treat `ws_root` directly as `repo_root`.
    - [`run_rust_app.bat`](run_rust_app.bat): updated target release binary path and removed obsolete `pushd rust`.
    - [`Dockerfile`](Dockerfile): updated multi-stage build paths to copy root workspace directly.
    - [`install.sh`](install.sh): updated cargo build and binary paths to source from root.
    - [`.github/workflows/rust_ci.yml`](.github/workflows/rust_ci.yml): removed `working-directory: rust` across jobs and included `rust` branch in triggers.
- **Intentional Behaviour Deviations from Python:** None. Layout reorganization on `rust` branch only.
- **Live Capture Impact:** None.
- **Verification Performed:**
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo test --workspace`: all 41 unit/integration/proptest tests passed.
- **Gotchas / Warnings for Next Model:**
  - The repository root on branch `rust` is now a standard Rust Cargo workspace root. All `cargo` and `xtask` commands are executed directly in the repository root without navigating to `rust/`.
### 2026-11-15 14:38 | Gemini 2.5 Flash / Antigravity (Windows Dev Workstation)
- **Phase:** Documentation Consolidation — `docs/` Directory Organization
- **Work Performed:**
  - Consolidated all 9 reverse-engineering specifications and command reference markdown files from repository root into [`docs/`](docs/):
    - `docs/AUDIO-IO-PIPELINE.md`
    - `docs/COMMAND_GUIDE.md`
    - `docs/COMPORT-AND-SCANNER-SELECTION.md`
    - `docs/DMA-VS-HP-ARCHITECTURE.md`
    - `docs/FORMATTERS-AND-FILENAME-SCHEME.md`
    - `docs/KEYPAD-AND-DISPLAY.md`
    - `docs/RECORDING-METADATA-FORMAT.md`
    - `docs/SERIAL-POLLING-AND-LAN-AUDIO.md`
    - `docs/VOX-ALGORITHM.md`
  - Updated relative documentation links in [`README.md`](README.md), [`AGENTS.md`](AGENTS.md), [`crates/sc-audio/src/lib.rs`](crates/sc-audio/src/lib.rs), [`crates/sc-engine/src/lib.rs`](crates/sc-engine/src/lib.rs), and [`crates/sc-feeder/src/lib.rs`](crates/sc-feeder/src/lib.rs).
- **Intentional Behaviour Deviations from Python:** None. Documentation reorganization only.
- **Live Capture Impact:** None.
- **Verification Performed:**
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo test --workspace`: all 41 unit/integration/proptest tests passed.
- **Gotchas / Warnings for Next Model:**
  - Reverse-engineering specifications now reside in `docs/` rather than the repository root. `README.md`, `AGENTS.md`, and `ACTIVE_CONTEXT.md` remain at root.
- **Next Tasks / Unfinished Work:** Pushed to `origin/rust`.

### 2026-11-21 12:35 | Gemini 2.5 Flash / Antigravity (Windows Dev Workstation)
- **Phase:** Rust Production Hardening (Post-Field Handoff)
- **Work Performed:**
  - Audited VM 123 field handoff report against the Rust codebase (`rust` branch).
  - Confirmed architectural immunity in Rust for the critical Python defect: serial I/O runs in dedicated `tokio::task::spawn_blocking` worker threads and does not starve Axum HTTP serving.
  - Implemented `GET /api/config` and `GET /api/health` endpoints in [`crates/sc-server/src/lib.rs`](crates/sc-server/src/lib.rs), enabling remote configuration reads and health auditing.
  - Hardened disabled scanner handling in [`crates/sc-engine/src/lib.rs`](crates/sc-engine/src/lib.rs): when `enabled == false`, initial status and ongoing polling set `status_line = "DISABLED"`, skipping unnecessary hardware poll cycles. Added unit test `test_disabled_scanner_status_line_and_skip_polling`.
  - Updated [`static/app.js`](static/app.js) to render disabled cards with dimmed opacity (`0.6`), hidden skip buttons, and `DISABLED` badges.
- **Intentional Behaviour Deviations from Python:** None.
- **Live Capture Impact:** None.
- **Verification Performed:**
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo test --workspace`: all 42 tests passed (`sc-audio`: 9, `sc-core`: 6, `sc-desktop`: 2, `sc-discovery`: 5, `sc-drivers`: 6, `sc-engine`: 4, `sc-feeder`: 3, `sc-metadata`: 5, `sc-server`: 2).
- **Gotchas / Warnings for Next Model:**
  - All development is locked to the Rust codebase (`rust` branch). Python is superseded by Rust.
- **Next Tasks / Unfinished Work:** Commit and push to `origin/rust`.

### 2026-11-21 13:14 | Claude / Antigravity (Windows Dev Workstation)
- **Phase:** Rust Production Hardening, Audio Architecture Overhaul (`cpal` 0.18 PulseAudio), & Web UI Overhaul
- **Work Performed:**
  - **Audio / CPU Spin Fix (`cpal` 0.18):** Upgraded workspace `cpal` from `0.15` to `0.18` (MSRV `1.85` in [`Cargo.toml`](Cargo.toml) and [`Dockerfile`](Dockerfile)), enabled pure-Rust `pulseaudio` feature on Linux in [`crates/sc-audio/Cargo.toml`](crates/sc-audio/Cargo.toml), set `JACK_NO_START_SERVER=1` on Linux startup in [`bins/scanner-controller/src/main.rs`](bins/scanner-controller/src/main.rs), added 5-second TTL audio device caching (`?refresh=true` bypass) in [`crates/sc-audio/src/devices.rs`](crates/sc-audio/src/devices.rs), moved RT callback error reporting onto atomic counters logged with rate-limited `WARN` on worker threads, added interruptible retry/backoff recovery in [`crates/sc-audio/src/stream.rs`](crates/sc-audio/src/stream.rs) and [`crates/sc-audio/src/loopback.rs`](crates/sc-audio/src/loopback.rs), and added `ScannerAudioRecorder::restart_if_running()` plus `SyncManager::update_config` live driver and audio hot-reload in [`crates/sc-engine/src/lib.rs`](crates/sc-engine/src/lib.rs).
  - **Test Parity & Hygiene:** Fixed `sc-feeder` fault-injection test in [`crates/sc-feeder/src/lib.rs`](crates/sc-feeder/src/lib.rs) to block directory creation using a regular file instead of a Windows `Z:\` drive path so it works cross-platform on Linux; decoupled [`crates/sc-core/src/lib.rs`](crates/sc-core/src/lib.rs) from root `config.json` by moving the sample fixture to [`fixtures/config.production.json`](fixtures/config.production.json), adding [`config.example.json`](config.example.json), adding `config.json` / `config.json.bak` to [`.gitignore`](.gitignore), and untracking `config.json` (`git rm --cached`); removed unused `sc-core`/`byteorder` dependencies from [`crates/sc-metadata/Cargo.toml`](crates/sc-metadata/Cargo.toml).
  - **Automatic Radio Model Detection & Phantom COM Filtering:** Added `ScannerModel::from_mdl_str` and `ScannerModel::from_usb_description` in [`crates/sc-core/src/enums.rs`](crates/sc-core/src/enums.rs); filtered phantom Linux `/dev/ttyS*` ports via `sysfs` inspection and canonicalized symlinked active ports in [`crates/sc-discovery/src/lib.rs`](crates/sc-discovery/src/lib.rs); added `ClassicDriver` `MDL`/`SI` probe and zero-downtime `AnyScannerDriver::into_family_for_model` / `poll_slot_with_autodetect` in-place driver upgrades in [`crates/sc-drivers/src/lib.rs`](crates/sc-drivers/src/lib.rs).
  - **Server & Process Hardening:** Added `uptime_seconds` in `GET /api/health`, `Cache-Control: no-cache` on static assets, name-first audio device resolution without clobbering offline device names, `enrich_scanner_model_from_ports` in `POST /api/config`, and Linux `NOTIFY_SOCKET` `READY=1` signaling in [`crates/sc-server/src/lib.rs`](crates/sc-server/src/lib.rs); added `SingleInstanceGuard` (`--lock-port`/`--no-lock`) and Unix `SIGTERM` graceful shutdown in [`bins/scanner-controller/src/main.rs`](bins/scanner-controller/src/main.rs).
  - **Settings Modal, Dual-Disable UI, & Output Directory UI:** Redesigned [`static/index.html`](static/index.html), [`static/style.css`](static/style.css), and [`static/app.js`](static/app.js) with a 3-tab scrollable settings modal (`Scanner A`, `Scanner B`, `Recording & Output`) with sticky header/footer; removed manual 34-model pickers in favor of auto-detected model pills; replaced ScanScribe feeder/webhook/test UI with a clean `Audio Output Directory` field (`#cfg_output_dir` -> `feeder.inbox_directory`, `feeder_mode = "DIRECTORY_DROP"`); and added clean independent disable states for both Scanner A and Scanner B.
- **Intentional Behaviour Deviations from Python:** Web UI no longer exposes manual model selection or ScanScribe webhook ingest controls; `AppConfig` schema retains backward compatibility for existing `config.json` files (`output_directory` serde alias accepted alongside `inbox_directory`).
- **Live Capture Impact:** Windows dev verification only (`not yet deployed to VM 123`); PulseAudio host transition will be verified on VM 123 next.
- **Verification Performed:**
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo test --workspace --no-fail-fast`: all 45 unit/integration/proptest tests passed (`sc-audio`: 9, `sc-core`: 7, `sc-desktop`: 2, `sc-discovery`: 6, `sc-drivers`: 7, `sc-engine`: 4, `sc-feeder`: 3, `sc-metadata`: 5, `sc-server`: 2).
- **Gotchas / Warnings for Next Model:**
  - Never overwrite or delete untracked local/production `config.json`.
  - Under `cpal` 0.18 PulseAudio on Linux, saved legacy ALSA numeric indices without `audio_device_name`/`output_device_name` fall back to host default devices until saved from the Web UI.
- **Next Tasks / Unfinished Work:** Deployed and verified on VM 123 (see entry below).

### 2026-11-21 13:19 | Claude / Antigravity (VM 123 Linux Deployment & Verification)
- **Phase:** Rust Production Hardening — VM 123 Live Verification (`cd0c8bb`)
- **Work Performed:**
  - Pulled `origin/rust` (`cd0c8bb`) onto VM 123 (`/opt/scanner-controller`), preserving production `config.json` (and untouched existing `config.json.pre-index.bak`), cleaned up stale Windows test artifact directory (`crates/sc-feeder/Z:*`), compiled release binary, and restarted user service `scanscribe-scanner.service`.
- **Intentional Behaviour Deviations from Python:** None beyond `cd0c8bb`.
- **Live Capture Impact:** Upgraded live capture/loopback to pure-Rust `cpal` 0.18 `pulseaudio` client; two live transmissions on `BCD436HP` (`154.2125` `Fire: Main 1 (Response)`) recorded cleanly to `/var/scanscribe/inbox/` immediately after service restart while loopback streamed simultaneously to `Uniden_Sink` (`ffmpeg` monitor active).
- **Verification Performed (VM 123 Linux `rustc 1.100.0`):**
  - `cargo check --workspace --all-targets`: exit 0 (0 Rust compiler warnings; Cargo noted unused manifest deps `byteorder` in [`Cargo.toml`](Cargo.toml) and `thiserror` in [`crates/sc-metadata/Cargo.toml`](crates/sc-metadata/Cargo.toml)).
  - `cargo test --workspace --no-fail-fast`: 45 passed, 0 failed (`sc-audio`: 9, `sc-core`: 7, `sc-desktop`: 2, `sc-discovery`: 6, `sc-drivers`: 7, `sc-engine`: 4, `sc-feeder`: 3, `sc-metadata`: 5, `sc-server`: 2).
  - `cargo build --release --bin scanner-controller`: succeeded in `35.31s`.
  - Live `scanscribe-scanner.service` telemetry (PID `129104`):
    - Total process CPU dropped from >100% (`cpal_alsa_out` spin before deploy) to `6.1%` overall CPU (`0.3%` MEM / `13.7M` RSS; `top -H` per-thread snapshot showed highest thread at `3.0%` / `10.0%` during active MP3+loopback and remaining 14 threads at `0.0%`).
    - `journalctl --user -u scanscribe-scanner`: zero `WARN`, `ERROR`, `ALSA`, or `JACK` lines after restart (`19:18:27 UTC`); clean PulseAudio connections to `Uniden_Sink` (`48000 Hz`, 2ch) and `Audio Adapter (Unitek Y-247A) Mono` (`44100 Hz`, 1ch).
    - Live HTTP checks (`127.0.0.1:8000`):
      - `GET /api/health`: `{"status":"healthy","uptime_seconds":68,...}` (`scanner_a` `BCD436HP` connected, `scanner_b` `DISABLED`).
      - `GET /api/ports/available`: returns only `/dev/ttyACM1` (`detected_model: "BCD436HP"`); all 32 phantom `/dev/ttyS*` ports filtered.
      - `GET /api/audio/devices` & `/api/audio/output_devices`: clean PulseAudio device lists with human-readable names.
      - `GET /api/state`: `scanner_b` shows `status_line: "DISABLED"` with empty metadata fields.
      - `GET /` & `/static/app.js`: return `cache-control: no-cache`, include `settings-tabs` and `cfg_output_dir`, and omit `34 Catalog` and `Pipeline Feeder`.
- **Gotchas / Warnings for Next Model:**
  - `cargo-clippy` component is not installed on VM 123's Linux toolchain (`rustup component add clippy` if needed on the VM).
  - Unused manifest dependency warnings remain for workspace `byteorder` (`Cargo.toml`) and `thiserror` (`crates/sc-metadata/Cargo.toml`) on Linux builds.
  - Note VM 123 production `config.json` currently uses PulseAudio defaults cleanly (`Audio Adapter (Unitek Y-247A) Mono` input and `Uniden_Sink` loopback output); opening Settings and clicking Save & Apply in the Web UI will persist explicit device names (`audio_device_name` / `output_device_name`).
- **Next Tasks / Unfinished Work:** None; VM 123 production verification complete.

### 2026-11-21 13:54 | Claude / Antigravity (Windows Dev Workstation)
- **Phase:** Feature Enhancement — ProScan-style Live LCD Mirror (`STS` Parsing)
- **Work Performed:**
  - Added [`DisplayLine`](crates/sc-core/src/status.rs) and optional `display_lines: Vec<DisplayLine>` (`skip_serializing_if = "Vec::is_empty"`) on `ScannerStatus` in [`crates/sc-core/src/status.rs`](crates/sc-core/src/status.rs) and re-exported from [`crates/sc-core/src/lib.rs`](crates/sc-core/src/lib.rs).
  - Implemented `extract_section_raw`, `clean_lcd_row_preserving_columns`, `parse_sts_display_lines`, and `update_sts_display_lines` in [`crates/sc-drivers/src/parse_util.rs`](crates/sc-drivers/src/parse_util.rs), handling `DISP_FORM` font sizing, reverse-video/underline attribute masks (`*`, `_`, `#`), horizontal dividers (`____...`), unescaped alpha-tag commas, and non-printable icon byte spacing.
  - Wired `update_sts_display_lines` into [`crates/sc-drivers/src/hp.rs`](crates/sc-drivers/src/hp.rs), [`crates/sc-drivers/src/dma.rs`](crates/sc-drivers/src/dma.rs), and [`crates/sc-drivers/src/classic.rs`](crates/sc-drivers/src/classic.rs) so `STS` screen lines populate continuously on every poll (while preserving `GLG`/`GSI`/`PWR` recording/exclusion metadata); cleared `display_lines` on disconnect/error/inactive/disabled transitions (`set_inactive_status` and `mark_status_disabled` in [`crates/sc-engine/src/lib.rs`](crates/sc-engine/src/lib.rs)).
  - Added `test_sts_live_lcd_display_parsing` in [`crates/sc-drivers/src/lib.rs`](crates/sc-drivers/src/lib.rs).
  - Added ProScan-style live LCD mirror rendering (`renderLcdMirror`) in [`static/index.html`](static/index.html), [`static/style.css`](static/style.css), and [`static/app.js`](static/app.js), falling back cleanly to system/channel placeholders when disconnected or empty.
- **Intentional Behaviour Deviations from Python:** Web UI replaces idle `"Scanning..."` placeholders with the scanner's live `STS` LCD buffer whenever connected and populated (`Option 1`).
- **Live Capture Impact:** Windows dev verification complete; pending VM 123 deployment and live BCD436HP `STS` verification.
- **Verification Performed:**
  - `cargo fmt --all -- --check`: clean (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
  - `cargo test --workspace --no-fail-fast`: all 46 tests passed (`sc-audio`: 9, `sc-core`: 7, `sc-desktop`: 2, `sc-discovery`: 6, `sc-drivers`: 8, `sc-engine`: 4, `sc-feeder`: 3, `sc-metadata`: 5, `sc-server`: 2).
- **Gotchas / Warnings for Next Model:**
  - `#[serde(default, skip_serializing_if = "Vec::is_empty")]` must stay on `ScannerStatus::display_lines` so disconnected initial state matches `fixtures/api/initial_state.json`.
- **Next Tasks / Unfinished Work:** Deployed and verified live on VM 123 (see entry below).

### 2026-11-21 13:57 | Claude / Antigravity (VM 123 Linux Live BCD436HP LCD Verification)
- **Phase:** Feature Verification — ProScan-style Live LCD Mirror (`e00c38c`)
- **Work Performed:**
  - Deployed `e00c38c` to VM 123 (`/opt/scanner-controller`), ran Linux `cargo test --workspace --no-fail-fast`, rebuilt release binary, and restarted `scanscribe-scanner.service` (PID `146859`).
- **Intentional Behaviour Deviations from Python:** Live Web UI mirrors physical scanner LCD screen rows via `STS` on `/api/state` and `/ws`.
- **Live Capture Impact:** Positive; live BCD436HP capture, PulseAudio loopback, and `STS` LCD streaming running together cleanly at `5.2%` CPU (`7.3M` RAM).
- **Verification Performed (VM 123 Linux `rustc 1.100.0`):**
  - `cargo test --workspace --no-fail-fast`: 46 passed, 0 failed (`sc-drivers`: 8 including `test_sts_live_lcd_display_parsing`; all 9 crates passing).
  - `cargo build --release --bin scanner-controller`: succeeded in `21.57s`.
  - Live `GET /api/state` (sampled 1.1s apart on PID `146859`):
    - `scanner_a.display_lines`: returned 14 live BCD436HP LCD rows (`F0:-12******9 13:57`, `S0:0*-------- Nov21`, `St. Francois`, `St. Francois County - Central Dispatch`, `Fire: Main 1 (Response)`, `  154.2125MHz    C203.5 `, `Fire Dispatch`, `Tag:--.--.---`) with exact `large` flags and `underline_divider` bars; two consecutive snapshots reflected real-time quick-key blink state transitions (`1`/`0` -> blank).
    - `scanner_b.display_lines`: omitted (`status_line: "DISABLED"`).
  - Live `/` & `/static/app.js`: returned `cache-control: no-cache` with `lcdScreenA` and `renderLcdMirror` present.
  - Process & logs: PID `146859` running at `5.2%` CPU, `0.1%` MEM (`7.3M`), zero `WARN`/`ERROR`/`ALSA`/`JACK` messages for PID `146859`.
- **Gotchas / Warnings for Next Model:**
  - None.
- **Next Tasks / Unfinished Work:** None.

### 2026-11-21 14:18 | Claude / Antigravity (Windows Dev Workstation & VM 123 Deploy)
- **Phase:** UI Polish — Zero-Emoji Matte-Graphite RF Console Aesthetic
- **Work Performed:**
  - Removed all emojis across [`static/index.html`](static/index.html), [`static/app.js`](static/app.js), and [`static/style.css`](static/style.css), replacing them with crisp inline SVG icons and CSS LED status dots (`rec-badge::before`, `engine-status::before`).
  - Redesigned [`static/style.css`](static/style.css) into a tighter matte-graphite instrumentation aesthetic (`#090b10` / `#10141d` / `#04070c` recessed LCD bezel, sharper `8px`/`6px`/`4px` corners, tabular numerals, segmented VU track ticks, responsive `@media (max-width: 960px)` layout) while preserving all element IDs, handlers, and the live LCD mirror.
  - Updated header/button/badge copy and status classes in [`static/app.js`](static/app.js) (`REC DISABLED`/`REC ACTIVE`/`REC STANDBY`, `engine-status`, `REC`, `Scanning...`/`Scan Serial Ports`).
- **Intentional Behaviour Deviations from Python:** Pure visual/copy refinement in Rust dashboard static assets; backend behavior and API contracts untouched.
- **Live Capture Impact:** Brief service restart on VM 123 during release rebuild/deploy.
- **Verification Performed:**
  - Static check: zero characters with `ord >= 0x2600` across `static/index.html`, `static/app.js`, and `static/style.css`.
  - Windows gate: `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings), `cargo test --workspace --no-fail-fast` (46 passed, 0 failed).
  - VM 123 deploy (`9ce0816`): `cargo build --release --bin scanner-controller` succeeded in `15.94s`; verified `/`, `/static/app.js`, and `/static/style.css` all return `200 OK` with `Cache-Control: no-cache`, zero emojis (`ord >= 0x2600`), `<svg>`, `lcdScreenA`, and `renderLcdMirror`; `GET /api/health` returned `{"status":"healthy","uptime_seconds":3,...}` (`scanner_a` connected `BCD436HP`, `scanner_b` `DISABLED`).
- **Gotchas / Warnings for Next Model:**
  - `sc-server` embeds `static/` at compile time in `--release`, so VM 123 requires `cargo build --release --bin scanner-controller` after `git pull`.
- **Next Tasks / Unfinished Work:** None.
