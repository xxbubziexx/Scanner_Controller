# Scanner_Controller — Rust Conversion Master Plan

> **Status:** Draft v1 — 2026-10-11
> **Owner:** xxbubziexx
> **Scope:** Port the whole Python controller (~250 KB source, see §1) to a Rust Cargo workspace,
> reaching proven parity before the Python code is retired.

---

## 0. Guiding Strategy

1. **Strangler, not big-bang.** The Python app stays the **reference implementation** and keeps
   running live until the Rust build passes every parity gate below. Rust lives in `rust/`
   next to the Python code until cutover (Phase 7).
2. **Golden fixtures first.** Before porting a module, freeze its behaviour. Generate byte-level
   fixtures from the Python implementation (and from real ProScan files), then make Rust match
   them **byte for byte**.
3. **The reverse-engineering docs are the spec.** `RECORDING-METADATA-FORMAT.md`,
   `VOX-ALGORITHM.md`, `SERIAL-POLLING-AND-LAN-AUDIO.md`, `KEYPAD-AND-DISPLAY.md`,
   `DMA-VS-HP-ARCHITECTURE.md`, `AUDIO-IO-PIPELINE.md`, `FORMATTERS-AND-FILENAME-SCHEME.md`
   and `COMPORT-AND-SCANNER-SELECTION.md` beat the Python code. If they disagree, it's a bug:
   write it down in `ACTIVE_CONTEXT.md` and decide on purpose. Never quietly copy the Python behaviour.
4. **Pure first, I/O last.** Start with deterministic crates (config, metadata, parsers), which
   are easy to test. Then do hardware I/O (serial, audio), then orchestration, then UI.
5. **Keep the wire contract.** The Rust HTTP/WebSocket server must serve the existing
   `static/` web UI with no changes at first. That gives a free end-to-end parity check.

---

## 1. Current Python Inventory → Target Crate Mapping

| Python module | Size | Responsibility | Target crate |
|---|---|---|---|
| `config.py` + `config.json` | 8 KB | Pydantic models, enums, defaults, load/save | `sc-core` |
| `metadata/proscan_metadata.py` | 23 KB | `pros` chunk, ID3v2.3, RIFF LIST/INFO, 33 `%` formatters, filename sanitize | `sc-metadata` |
| `drivers/base_driver.py` | 6 KB | `ScannerStatus`, `BaseScannerDriver`, freq normalization | `sc-drivers` |
| `drivers/bcd996p2.py`, `classic_driver.py` | 19 KB | DMA / classic command dialects | `sc-drivers::dma`, `::classic` |
| `drivers/bcd436hp.py` | 19 KB | HomePatrol dialect (GSI XML, STS, keypad) | `sc-drivers::hp` |
| `drivers/driver_factory.py` | 2 KB | Model → driver dispatch (34 models) | `sc-drivers::factory` |
| `discovery/serial_detector.py`, `net_detector.py` | 8 KB | MDL/SI probing, UDP LAN discovery | `sc-discovery` |
| `audio/recorder.py` | 31 KB | Device enum, pre-roll ring, VOX/hang, loopback mixer, de-click ramps | `sc-audio` |
| `engine/sync_manager.py` | 22 KB | Poll loop, mutual exclusion, resample, dispatch | `sc-engine` |
| `feeder/scanscribe_feeder.py` | 19 KB | Directory drop / webhook, MP3 validation, sidecar JSON | `sc-feeder` |
| `server.py` + `static/` | 12 KB + 57 KB | FastAPI REST + `/ws` telemetry, web UI | `sc-server` (+ reuse `static/`) |
| `gui.py` | 76 KB | CustomTkinter desktop GUI | `sc-desktop` (Tauri, wraps `static/`) |
| `install.sh`, `*.service`, `*.spec`, `build_*.bat`, `Dockerfile` | — | Deployment | `xtask` + rewritten scripts |
| `tests/` (10 files) | 67 KB | pytest suite | Fixture generator + `cargo test` |

### Proposed workspace layout

```
rust/
├── Cargo.toml                 # [workspace], shared lints, pinned deps
├── crates/
│   ├── sc-core/               # config types, enums, ScannerStatus, errors
│   ├── sc-metadata/           # pros / ID3v2.3 / RIFF / formatters (no I/O deps)
│   ├── sc-drivers/            # Transport trait + DMA / HP / classic protocol
│   ├── sc-discovery/          # serial + LAN probing
│   ├── sc-audio/              # cpal capture/playback, ring buffers, VOX, encoders
│   ├── sc-engine/             # SyncManager actor, exclusion logic
│   ├── sc-feeder/             # inbox drop, webhook, sidecar JSON
│   ├── sc-server/             # axum REST + WebSocket, serves ../static
│   └── sc-desktop/            # Tauri shell (Phase 6)
├── bins/
│   └── scanner-controller/    # CLI entrypoint: `run`, `detect`, `probe-keypad`, `serve`
├── fixtures/                  # golden files generated in Phase 0 (git-tracked)
└── xtask/                     # build, cross-compile, package helpers
```

### Candidate dependencies (pin exact versions in Phase 0)

| Concern | Crate(s) | Notes |
|---|---|---|
| Async runtime | `tokio`, `tokio-util` (`CancellationToken`) | Orchestration, server, feeder |
| Serial | `serialport` or `tokio-serial` | Pick one in Phase 2 spike |
| Audio I/O | `cpal` | WASAPI (Win) / ALSA (Linux) |
| RT-safe queues | `rtrb` (SPSC ring) | Audio callback ↔ worker threads |
| Resampling | `rubato` | Replaces `resample_pcm_int16` |
| MP3 encode | `mp3lame-encoder` (LAME bindings) | Builds C code; LGPL. Check cross-compile in spike |
| WAV | `hound` (PCM only) — RIFF/LIST/`pros` written by hand in `sc-metadata` | |
| XML (HP GSI) | `quick-xml` | |
| HTTP server | `axum` + `tower-http` (static files) | |
| HTTP client | `reqwest` (rustls) | Webhook feeder |
| Serde | `serde`, `serde_json` | Must read today's `config.json` unchanged |
| Errors | `thiserror` (libs), `anyhow` (bins) | |
| Logging | `tracing`, `tracing-subscriber`, `tracing-journald` (Linux) | |
| Desktop | `tauri` v2 | Phase 6 |
| Testing | `insta` (snapshots), `proptest` | |

---

## 2. Phase Overview

```mermaid
flowchart LR
    P0["P0 Baseline and Fixtures"] --> P1["P1 Core and Metadata"]
    P1 --> P2["P2 Drivers and Discovery"]
    P1 --> P3["P3 Audio Pipeline"]
    P2 --> P4["P4 Engine and Feeder"]
    P3 --> P4
    P4 --> P5["P5 Headless Server - Shadow Run"]
    P5 --> P6["P6 Desktop App"]
    P5 --> P7["P7 Packaging and Cutover"]
    P6 --> P7
```

P2 and P3 can run in parallel once P1 is done.

### Detailed per-phase checklists

The phase sections below are summaries. The detailed checklists and exit gates live in one file per phase, using the **same P0–P7 numbering**. If a summary and a phase file disagree, the phase file wins; fix the other one.

| Phase | File |
|---|---|
| P0 | [`phase_0_baseline_and_scaffolding.md`](phase_0_baseline_and_scaffolding.md) |
| P1 | [`phase_1_core_and_metadata.md`](phase_1_core_and_metadata.md) |
| P2 | [`phase_2_drivers_and_discovery.md`](phase_2_drivers_and_discovery.md) |
| P3 | [`phase_3_audio_pipeline.md`](phase_3_audio_pipeline.md) |
| P4 | [`phase_4_engine_and_feeder.md`](phase_4_engine_and_feeder.md) |
| P5 | [`phase_5_server_and_shadow_run.md`](phase_5_server_and_shadow_run.md) |
| P6 | [`phase_6_desktop_app.md`](phase_6_desktop_app.md) |
| P7 | [`phase_7_packaging_and_cutover.md`](phase_7_packaging_and_cutover.md) |

---

## Phase 0 — Baseline, Fixtures & Scaffolding

**Goal:** Freeze what "correct" means before touching Rust.

- [x] Tag the current Python state: `git tag python-baseline-v1`.
- [x] Get `python -m pytest tests/` green and record the pass count in `ACTIVE_CONTEXT.md`.
- [x] Write `tools/gen_fixtures.py`, which uses the **Python** modules to emit `rust/fixtures/`:
  - `metadata/*.bin`: `build_pros_chunk`, `write_mp3_id3v23`, `write_wav_riff_info` output for a
    matrix of `ProScanMetadata` inputs (empty fields, max-length 253, non-ASCII, every template).
  - `formatters.json`: `{template, meta, expected}` for all 33 specifiers plus edge cases.
  - `drivers/*.txt`: recorded raw serial transcripts (request → response) from a real 996P2 and 436HP,
    plus the parsed `ScannerStatus` JSON the Python driver produced.
  - `audio/*.pcm` + `vox_expected.json`: PCM input → VOX gate open/close sample indices.
  - `resample/*.pcm`: input/output pairs for 8k/16k/22.05k/44.1k/48k.
- [x] Copy the real ProScan sample recordings in the repo root (`*.wav`, `*.mp3`) into
  `rust/fixtures/proscan_samples/` as **parse** fixtures, i.e. third-party ground truth.
- [x] Scaffold the `rust/` workspace with shared `[workspace.lints]` (`clippy::unwrap_used = "deny"` etc.).
- [x] CI (GitHub Actions): `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on
  `windows-latest` + `ubuntu-latest`; cross-check `aarch64-unknown-linux-gnu` build.
- [x] Write `AGENTS.md` + `ACTIVE_CONTEXT.md` (done with this plan).

**Exit gate:** fixtures committed, CI green on an empty workspace, Python suite green.

---

## Phase 1 — `sc-core` + `sc-metadata` (pure logic)

**Goal:** Byte-exact metadata output. This is the module where mistakes hurt ScanScribe ingest the most.

- [x] `sc-core`: port every Pydantic model/enum (`ConnectionMode`, `ScannerModel` (34 variants),
  `LockoutMethod`, `PriorityMode`, `ExclusionMode`, `AudioChannelMode`, `FeederMode`, `ScannerConfig`,
  `ScanScribeFeederConfig`, `SyncConfig`, `AudioLoopbackConfig`, `AppConfig`) with `serde`.
  - Must deserialize the **existing** `config.json` unchanged (same keys, `SCREAMING_CASE` enum strings,
    `null` device indices). Keep validators (`sample_rate`, `audio_format`, allowed baud rates per model).
  - `ScannerStatus` as a plain `Clone` struct (replaces `.clone()` / `.to_dict()`).
- [x] `sc-metadata`:
  - `pros` chunk build + parse (`RECORDING-METADATA-FORMAT.md` §1), including length backpatch semantics.
  - ID3v2.3 writer: syncsafe ints, text frames (§2).
  - RIFF `LIST`/`INFO` writer (§3), append/re-tag behaviour (§4).
  - Template engine for all 33 `%` specifiers (§5, `FORMATTERS-AND-FILENAME-SCHEME.md`),
    `format_streaming_metadata`, `sanitize_filename`, `sanitize_folder_path`, `resolve_collision_filename`.
  - `clean_ascii_1` (max 253) as a dedicated, well-tested function.
- [x] Tests: every fixture from Phase 0 is checked with `assert_eq!(bytes, fixture)`. Add `proptest`
  round-trip (`build → parse == input`) and parse every real ProScan sample file.

**Exit gate:** 100% fixture parity, zero `unwrap()` in non-test code, `cargo doc` builds.

---

## Phase 2 — `sc-drivers` + `sc-discovery`

**Goal:** Talk to real scanners from Rust, with protocol logic testable without hardware.

- [x] Define a `Transport` trait (`send_line`, `read_until_cr`, timeouts) with three implementations:
  `SerialTransport`, `UdpTransport` (LAN models), and `ReplayTransport` (fixture transcripts) for tests.
- [x] Split each driver into a **pure codec** (build command bytes / parse response → `ScannerStatus`)
  and a thin I/O shell. All parsing is unit tested against Phase 0 transcripts.
- [x] Port dialects: DMA (`BCD996P2`, 325P2, XT), HP (`BCD436HP`, 536HP, SDS100/200, incl. GSI XML via
  `quick-xml`), classic. Port `normalize_frequency` and `extract_section` exactly.
- [x] Control actions: skip / hold / KEYPRESS lockout (`KEYPAD-AND-DISPLAY.md`, `SERIAL-POLLING...` §1.5).
- [x] Port-open handshake (§1.6), reconnect back-off (`should_try_connect`).
- [x] `sc-discovery`: enumerate ports, MDL/SI probe (`parse_mdl_response`, `parse_si_response`,
  `extract_port_digits` ordering), UDP LAN probe (`parse_sus_response`).
- [x] Spike: `serialport` (blocking, one thread per scanner) vs `tokio-serial`. Pick based on Windows
  COM latency at 150 ms poll interval and write the choice down in `ACTIVE_CONTEXT.md`.
- [x] Port `tools/probe_keypad.py` → `scanner-controller probe-keypad` subcommand.

**Exit gate:** replay tests green, plus a manual hardware test on a 996P2 **and** a 436HP: 30 min of polling,
status matches the Python driver side by side, no port lockups after unplug/replug.

---

## Phase 3 — `sc-audio` (real-time pipeline)

**Goal:** Glitch-free capture, VOX, loopback and encoding, under strict real-time rules.

- [x] Device enumeration matching `/api/audio/devices` and `/api/audio/output_devices` JSON shapes.
  > [!NOTE] cpal device indices are **not** PortAudio indices. Plan a migration: store device **name**
  > (+ host API) in config, and fall back to index only when reading old configs.
- [x] Capture via `cpal` input stream → `rtrb` SPSC ring → per-scanner worker thread.
  **Audio callback rules:** no allocation, no locks, no logging, no syscalls (see `AGENTS.md` §2).
- [x] `PreRollRingBuffer`: sample-accurate circular buffer (port exactly, incl. chronological read-out).
- [x] VOX / squelch: energy measurement, noise-floor history ring, threshold gate,
  attack/decay/hang (`VOX-ALGORITHM.md` §1–4, "Porting notes"). Check against `vox_expected.json`.
- [x] Loopback mixer: `STEREO_SPLIT` / `CENTER`, volume, 15 ms de-click gain ramps on duck/mute.
- [x] Resampler (`rubato`) replacing `resample_pcm_int16`; verify against fixtures within tolerance.
- [x] Encoders: WAV (`hound` PCM + hand-rolled chunks from `sc-metadata`), MP3 (`mp3lame-encoder`),
  matching the Python `lameenc` bitrate/quality settings.
- [ ] Optional / stretch: LAN audio (RTSP over TCP + RTP/µ-law over UDP, `SERIAL-POLLING...` Target 2),
  only if the Python build supports it today; otherwise track as a post-cutover feature.

**Exit gate:** 2-hour soak on Windows and Raspberry Pi: zero xruns logged, VOX fixtures pass,
loopback audibly click-free, MP3/WAV outputs pass `sc-metadata` parse + play in ProScan / ScanScribe.

---

## Phase 4 — `sc-engine` + `sc-feeder`

**Goal:** Mutual exclusion and dispatch, with the same behaviour as Python and fixes for known weak spots.

- [x] `SyncManager` as a tokio actor: owns drivers, receives `ScannerStatus` updates via channels,
  exposes state through a `watch` channel (feeds `/api/state` + `/ws`).
- [x] Port `_should_exclude` (TGID / FREQ / both, `< 0.001` MHz tolerance), `active_exclusions` with
  `release_at`, `hang_time_seconds`, `auto_release_seconds`, `PriorityMode` (`EQUAL_PEERS` / primary),
  event log capped at 100.
- [x] **Behaviour fix to decide on purpose:** Python polls A and B with `asyncio.gather` and drops *both*
  results if either fails (`sync_manager.py:243-251`). Rust should poll each scanner on its own so one
  dead radio never stalls the other. Record the decision in `ACTIVE_CONTEXT.md`.
- [x] Duplicate-abort: cancel the secondary recording **before** it is saved/dispatched.
- [x] Dispatch worker pool (replaces `ThreadPoolExecutor`) built on `spawn_blocking` / bounded channel
  with back-pressure. Never drop a completed call silently.
- [x] `sc-feeder`: `DIRECTORY_DROP` (atomic write: temp file + rename into inbox), webhook POST
  (`CallTransmissionPayload` JSON shape unchanged), `is_valid_mp3_payload`, sidecar `.json`, fallback paths.
- [x] Hot config reload (`update_config`) without restarting audio streams unless devices changed.

**Exit gate:** simulated two-scanner tests (ReplayTransport + synthetic PCM) cover every exclusion mode;
feeder output is byte-identical to Python for the same inputs.

---

## Phase 5 — `sc-server` + headless daemon (Shadow Run)

**Goal:** Drop-in replacement for `server.py`, with the existing web UI working unchanged.

- [ ] `axum` routes, keeping paths, methods and JSON shapes identical:
  `GET /`, `/favicon.ico`, `/api/state`, `POST /api/config`, `POST /api/scanner/{id}/model`,
  `POST /api/skip/{id}`, `/api/audio/devices`, `/api/audio/output_devices`, `/api/ports/available`,
  `/api/ports/detect`, `/api/scanners/discover_lan`, `/api/models/catalog`, `POST /api/feeder/config`,
  `POST /api/feeder/dispatch_test`, `WS /ws`.
- [ ] Serve `static/` (embed with `include_dir`/`rust-embed` for single-binary deploys).
- [ ] Contract tests: record Python responses into `fixtures/api/*.json` and diff them against Rust (ignoring timestamps).
- [ ] Graceful shutdown: `CancellationToken` → stop polling → flush in-flight recordings → close ports.
- [ ] systemd unit (`Type=notify` optional), `tracing-journald` logging.
- [ ] **Shadow run:** a serial port can only be opened by one process, so the two stacks can't run
  at the same time on the same radios. Alternate Python and Rust on the live Pi (e.g. daily) for ≥ 7 days,
  each writing to its *own* inbox. Compare calls/hour, durations, exclusion events and metadata.
  If a spare scanner pair is available, run them truly side by side instead.

**Exit gate:** web UI fully functional on Rust backend; shadow-run discrepancies explained or fixed.

---

## Phase 6 — Desktop App (`sc-desktop`)

**Goal:** Replace the 76 KB CustomTkinter `gui.py`.

- [ ] **Recommended:** Tauri v2 shell embedding the engine in-process and rendering the existing `static/`
  UI, so there is one UI codebase for desktop and web.
- [ ] Alternative (open decision): `egui` native UI, if you'd rather have GPU-drawn VU meters / no webview.
- [ ] Bring over GUI-only features from `gui.py` that `static/` lacks (audit `gui.py` first and list them in this plan).
- [ ] Tray icon, start-minimized, single-instance lock, Windows installer (MSI/NSIS via Tauri bundler).

**Exit gate:** every `gui.py` feature has a matching feature in the new UI, verified by a checklist.

---

## Phase 7 — Packaging, Cutover & Python Retirement

- [ ] Rewrite `install.sh`: download/copy the prebuilt binary, create user/groups (`dialout`/`uucp`, `audio`),
  inbox dir, systemd unit. Keep all current flags (`--system`, `--local`, `--dir`, `--user`, `--service`,
  `--no-service`, `--start`, `--port`, `--inbox`, `--no-deps`, `-y`).
- [ ] Release artifacts: `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
  (Pi), optional `armv7`. Update `Dockerfile` to a multi-stage Rust build.
- [ ] Config migration: Rust reads old `config.json`; on first save, writes device names next to indices.
- [ ] Cutover on the live node, keeping Python ready for a one-command rollback for 14 days.
- [ ] Move Python into `legacy/` (or delete it, tagged), drop `requirements.txt`, `*.spec`, `build_*.bat`.
- [ ] Update `README.md`; keep all reverse-engineering `.md` docs (they remain the spec).

**Exit gate:** 14 days on Rust in production without a rollback.

---

## 3. Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| Metadata byte drift breaks ScanScribe ingest | High | Phase 1 byte-exact fixtures; parse real ProScan files |
| cpal device indexing ≠ PortAudio | Med | Store device names; migration path in Phase 3/7 |
| LAME bindings fail to cross-compile for aarch64 | Med | Spike in Phase 0/3; fallback: build on-device or `cross` docker image |
| Windows COM port quirks (USB CDC timeouts) | Med | Hardware gate in Phase 2; `serialport` vs `tokio-serial` spike |
| Behaviour "fixes" silently change semantics | Med | Every intentional deviation logged in `ACTIVE_CONTEXT.md` |
| Scope creep (LAN audio, new features mid-port) | Med | No new features until Phase 7 exit; track in backlog |
| Rust learning curve slows progress | Low–Med | Pure crates first (P1) to build familiarity before RT audio |

---

## 4. Open Decisions

1. **Desktop UI:** Tauri reusing `static/` (recommended) vs `egui`.
2. **Serial I/O model:** blocking thread-per-scanner vs `tokio-serial`.
3. **Repo layout after cutover:** keep `rust/` subdir or promote the workspace to repo root.
4. **LAN/RTSP audio:** in scope for parity, or post-cutover feature?
5. **Python freeze policy:** bug fixes only during the port (recommended), or keep adding features in both?
