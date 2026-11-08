# Phase 0: Baseline, Fixtures & Scaffolding

> **Objective:** Freeze what "correct" means before writing Rust logic, and prove the riskiest toolchain assumptions early.
> **Prerequisites:** Rust toolchain (`rustup`, `cargo`, `rustc`) installed on the Windows dev machine.
> **Master plan:** [`rust_conversion_master_plan.md`](rust_conversion_master_plan.md) (Phase 0 section is the summary; this file is the detailed checklist).

---

## 1. Python Baseline
- [ ] `python -m pytest tests/` passes. Record the count in [`ACTIVE_CONTEXT.md`](../ACTIVE_CONTEXT.md) (last recorded: 59 passed).
- [ ] `git tag python-baseline-v1`.

## 2. Fixture Generator (`tools/gen_fixtures.py`)
Uses the **Python** modules to emit `rust/fixtures/`. Fixtures are read-only once committed. Regenerating one needs a written reason in `ACTIVE_CONTEXT.md`.

- [ ] `metadata/*.bin`: `build_pros_chunk`, `write_mp3_id3v23`, `write_wav_riff_info` for a matrix of `ProScanMetadata` inputs:
  empty fields, max-length 253, non-ASCII, odd/even payload length (pad byte), all-empty MP3 templates (no ID3, no `pros`), WAV with everything empty.
- [ ] `formatters.json`: `{template, meta, expected}` for all 33 `%` specifiers, date/time specifiers, path sanitization.
- [ ] `drivers/*.txt`: raw serial transcripts (request → response) from a real BCD996P2 and BCD436HP, plus the `ScannerStatus` JSON the Python driver produced.
- [ ] `audio/*.pcm` + `vox_expected.json`: PCM in → gate open/hang/close sample indices.
- [ ] `resample/*.pcm`: input/output pairs for 8k/16k/22.05k/44.1k/48k.
- [ ] `api/*.json`: recorded Python responses for every REST route (used by Phase 5 contract tests). Normalise timestamps.
- [ ] `feeder/`: golden sidecar JSON, webhook payload JSON, and directory-drop output for fixed inputs (used by Phase 4 byte-identity test).
- [ ] `proscan_samples/`: copy the real `*.wav` / `*.mp3` samples from the repo root (parse fixtures; originals stay untouched).

## 3. Toolchain Spikes (do these now, not later)
- [ ] **LAME cross-compile:** `mp3lame-encoder` builds for `aarch64-unknown-linux-gnu`. Fallbacks: `cross` docker image, or build on-device. Record the result.
- [ ] Pin exact crate versions in the workspace `Cargo.toml` and justify the MSRV.

## 4. Workspace Scaffold & CI
```
rust/
├── Cargo.toml        # [workspace], shared lints, pinned deps
├── crates/           # sc-core, sc-metadata (others added in later phases)
├── bins/             # scanner-controller (CLI, added later)
├── fixtures/
└── xtask/
```
```toml
[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
all = "warn"
pedantic = "warn"
```
- [ ] GitHub Actions on `windows-latest` + `ubuntu-latest`: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] `aarch64-unknown-linux-gnu` build check.

## Exit Gate
Fixtures committed, CI green on the empty workspace, Python suite green, LAME spike result recorded.
