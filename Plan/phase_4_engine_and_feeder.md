# Phase 4: `sc-engine` & `sc-feeder`

> **Objective:** Mutual exclusion and dispatch with the same behaviour as Python, plus the one deliberate fix below.
> **Prerequisites:** [Phase 2](phase_2_drivers_and_discovery.md) and [Phase 3](phase_3_audio_pipeline.md) exit gates passed.
> Port of [`engine/sync_manager.py`](../engine/sync_manager.py) and [`feeder/scanscribe_feeder.py`](../feeder/scanscribe_feeder.py).

---

## 1. `sc-engine`
- [x] `SyncManager` as a tokio actor owning drivers and recorders. State is exposed through a `watch` channel (feeds `/api/state` and `/ws`).
- [x] `_should_exclude`: `TGID_ONLY` / `FREQ_ONLY` / `TGID_AND_FREQ`, frequency tolerance `< 0.001` MHz.
- [x] `PriorityMode`: `EQUAL_PEERS`, `SCANNER_A_PRIMARY`, `SCANNER_B_PRIMARY`.
- [x] `active_exclusions` with `release_at`, `hang_time_seconds`, `auto_release_seconds`. Event log capped at 100.
- [x] **Duplicate-abort:** the secondary recording is cancelled in memory **before** it is saved or dispatched. This is the only permitted discard (Prime Directive 1).
- [x] Hot config reload (`update_config`) without restarting audio streams unless devices changed.

### Deliberate behaviour deviation (decision required)
Python polls A and B with `asyncio.gather` and drops **both** results if either fails (`sync_manager.py:243-251`). The Rust plan polls each scanner in its own task with its own timeout, so one dead radio never stalls the other.
- [x] Record the decision and rationale in `ACTIVE_CONTEXT.md` (Intentional Deviations) **before** implementing.

## 2. `sc-feeder`
- [x] Dispatch worker pool: bounded channel + `spawn_blocking`, with back-pressure. A completed call is never silently dropped.
- [x] `DIRECTORY_DROP`: write temp file, then atomic rename into the inbox. Sidecar `.json` (ProScan-compatible).
- [x] Webhook: `reqwest` (rustls), `CallTransmissionPayload` JSON unchanged.
- [x] `is_valid_mp3_payload` pre-dispatch check. Failed dispatches are logged, retried, and finally written to the fallback directory.
- [x] Every spawned task is tracked and stops via `CancellationToken`.

## Exit Gate
1. Simulated two-scanner tests (`ReplayTransport` + synthetic PCM) cover **every** exclusion and priority mode.
2. Feeder output (audio bytes, sidecar JSON, webhook payload) is **byte-identical** to the Phase 0 `feeder/` fixtures.
3. Fault-injection tests: webhook down, inbox unwritable, one scanner failing. No call is lost and the healthy scanner keeps polling.
