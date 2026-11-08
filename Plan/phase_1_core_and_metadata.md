# Phase 1: `sc-core` & `sc-metadata` (Pure Logic)

> **Objective:** Byte-exact metadata output and a config model that reads today's `config.json` unchanged. This is where mistakes hurt ScanScribe ingest most.
> **Prerequisites:** [Phase 0](phase_0_baseline_and_scaffolding.md) exit gate passed.
> **Spec (source of truth):** [`RECORDING-METADATA-FORMAT.md`](../RECORDING-METADATA-FORMAT.md), [`FORMATTERS-AND-FILENAME-SCHEME.md`](../FORMATTERS-AND-FILENAME-SCHEME.md).

---

## 1. `sc-core` (port [`config.py`](../config.py), [`drivers/base_driver.py`](../drivers/base_driver.py))
- [ ] `ScannerModel` (34 variants) and enums: `ConnectionMode`, `LockoutMethod`, `PriorityMode`, `ExclusionMode`, `AudioChannelMode`, `FeederMode`.
- [ ] `ScannerConfig`, `ScanScribeFeederConfig`, `SyncConfig`, `AudioLoopbackConfig`, `AppConfig` via `serde`.
- [ ] `ScannerStatus` as a plain `Clone` struct.
- [ ] Deserialize existing `config.json` unchanged: same keys, `SCREAMING_CASE` enum strings, `null` device indices, same defaults.
- [ ] Validators: sample rate, `audio_format` (`mp3`/`wav`), allowed baud rates per model.
- [ ] Device-name field added next to the index for the later cpal migration (old configs still load).

## 2. `sc-metadata` (port [`metadata/proscan_metadata.py`](../metadata/proscan_metadata.py), zero I/O)

### 2.1 `pros` chunk (spec §1)
- [ ] Framing: `"pros"` + `u32` LE payload length (payload only, excludes the 8 header bytes) + payload. One `0x00` pad byte if the length is odd, and the pad is counted.
- [ ] Payload: 22 `Key:Value\0` strings in the spec's exact order (`Scanner:` … `ScannerMode:`). Parse by consuming `length` bytes.
- [ ] Fixed-width, space-padded `NoTrim` fields: `Tone:` 24, `UID:` 64, `UID#:` 16. All other fields are trimmed.
- [ ] `EndingDate:` placeholder `00000000000000`, plus close-time backpatch of the four placeholder fields.
- [ ] `DMRSlot:` replaces `/` with `S`. `ScannerMode:` is cut at the first `,`.

### 2.2 ID3v2.3 (spec §2)
- [ ] Header `ID3` `03 00` flags `00` + syncsafe size (backpatched, `file length - 10`).
- [ ] Frames in order: `TSEE` (literal `ProScan`), `TRDA` (`yyyyMMddHHmmss-00000000000000`), `TIT2`, `TPE1`, `TPE2`, then the raw `pros` chunk (not an ID3 frame).
- [ ] **Gate:** the whole tag, and with it the `pros` chunk, is written only if at least one of the TIT2/TPE1/TPE2 templates is non-empty. Empty frames are written with size 2.
- [ ] Text is `clean_ascii_1`-filtered and truncated to 253 characters.

### 2.3 WAV RIFF `LIST`/`INFO` (spec §3–4)
- [ ] `RIFF`/`WAVE` + `LIST`/`INFO` (`IPRD`, `ICRD`, `INAM`, `IART`) + `pros`, written **unconditionally** (no template gate, unlike MP3).
- [ ] Append / re-tag behaviour per spec §4. Chunk offsets derive from the end of `pros`, not from the declared LIST size.
- [ ] Parser for reading `pros` from WAV and MP3.

### 2.4 Templates & filenames (spec §5)
- [ ] All 33 `%` specifiers, `format_streaming_metadata`, `sanitize_filename`, `sanitize_folder_path`, `resolve_collision_filename`.
- [ ] `clean_ascii_1` (max 253) as a dedicated, well-tested function.

> Any spec/code disagreement found while porting goes in `ACTIVE_CONTEXT.md` under *Open Questions*. Do not pick a side silently.

## Exit Gate
1. Every Phase 0 `metadata/*.bin` fixture matches byte for byte (`cargo test -p sc-metadata`). `formatters.json` passes fully.
2. Every file in `proscan_samples/` parses.
3. `proptest` round-trip (`build → parse == input`) passes 10,000 cases without panics.
4. `sc-core` loads the real `config.json` and re-serializes with identical keys.
5. `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` are clean. No `unwrap()` in library code. `cargo doc` builds.
