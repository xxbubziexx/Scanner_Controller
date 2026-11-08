# Phase 2: `sc-drivers` & `sc-discovery`

> **Objective:** Talk to real scanners from Rust, with protocol logic testable without hardware.
> **Prerequisites:** [Phase 1](phase_1_core_and_metadata.md) exit gate passed. Can run in parallel with [Phase 3](phase_3_audio_pipeline.md).
> **Spec:** [`DMA-VS-HP-ARCHITECTURE.md`](../DMA-VS-HP-ARCHITECTURE.md), [`SERIAL-POLLING-AND-LAN-AUDIO.md`](../SERIAL-POLLING-AND-LAN-AUDIO.md), [`KEYPAD-AND-DISPLAY.md`](../KEYPAD-AND-DISPLAY.md), [`COMPORT-AND-SCANNER-SELECTION.md`](../COMPORT-AND-SCANNER-SELECTION.md).

---

## 1. Serial I/O Decision Spike (do first)
- [x] Compare `serialport` (blocking, one thread per scanner) with `tokio-serial` on Windows COM latency at the 150 ms poll interval. Record the choice and numbers in `ACTIVE_CONTEXT.md`.
- [x] The `Transport` trait shape depends on this result. Do **not** assume `async_trait` with `&mut self`. A blocking thread per port is the default if the spike is a tie.
- [x] One owner per port (AGENTS.md prime directive 4). Never open a port the live service may hold.

## 2. Transport Abstraction
- [x] `Transport` trait: `send_line`, `read_until_cr`, with timeouts.
- [x] `SerialTransport`, `UdpTransport` (LAN models: SDS200, BCD536HP), and `ReplayTransport` (feeds Phase 0 transcripts).

## 3. Pure Codecs + Thin I/O Shell
Each driver is a pure codec (build command bytes / parse response → `ScannerStatus`) plus a small I/O wrapper.
- [x] **DMA** (BCD996P2, 325P2, XT): `GLG`, `PWR`, `VOL`, `SQL`. `normalize_frequency` and `extract_section` ported exactly.
- [x] **HomePatrol** (BCD436HP, 536HP, SDS100/200): `STS` parser, `GSI` XML via `quick-xml`.
- [x] **Classic** (BC346XT and earlier).
- [x] **Factory:** model enum → driver for all 34 models.
- [x] **Control actions:** hold, skip, KEYPRESS lockout; port-open handshake; reconnect back-off (`should_try_connect`).

## 4. `sc-discovery`
- [x] Port enumeration with VID/PID filtering.
- [x] MDL/SI probe (`parse_mdl_response`, `parse_si_response`, `extract_port_digits` ordering).
- [x] UDP LAN probe (`parse_sus_response`).

## 5. CLI
- [x] Port `tools/probe_keypad.py` → `scanner-controller probe-keypad` (plus `detect`).

## Exit Gate
1. All transcripts in `rust/fixtures/drivers/` replay green on every dialect.
2. **Hardware (not-hardware-tested until done):** 30 min of polling on a real BCD996P2 **and** BCD436HP, status matching the Python driver side by side.
3. Unplug/replug recovers without lockups or deadlocks.
4. Serial decision recorded.
