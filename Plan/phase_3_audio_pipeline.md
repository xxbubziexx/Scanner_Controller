# Phase 3: `sc-audio` (Real-Time Pipeline)

> **Objective:** Glitch-free capture, VOX, loopback and encoding under strict real-time rules.
> **Prerequisites:** [Phase 1](phase_1_core_and_metadata.md) exit gate passed. Can run in parallel with [Phase 2](phase_2_drivers_and_discovery.md).
> **Spec:** [`AUDIO-IO-PIPELINE.md`](../AUDIO-IO-PIPELINE.md), [`VOX-ALGORITHM.md`](../VOX-ALGORITHM.md). Port of [`audio/recorder.py`](../audio/recorder.py).

---

## 1. Real-Time Callback Rules (AGENTS.md §2)
Inside the `cpal` callback: no heap allocation, no `Mutex`/`RwLock`, no logging, no file/network/serial I/O, no blocking channel. Only lock-free SPSC (`rtrb`) and atomics. All other work runs on worker threads.

## 2. Capture & Devices
- [x] Enumeration matching the `/api/audio/devices` and `/api/audio/output_devices` JSON shapes.
- [x] cpal indices are **not** PortAudio indices. Resolve by stored device name (+ host API) and fall back to the index only for old configs.
- [x] `cpal` input stream → `rtrb` ring → per-scanner worker thread. Normalise `f32`/`i16` to signed 16-bit PCM.

## 3. Pre-Roll & VOX
- [x] `PreRollRingBuffer`: constant memory (0.5 s to 3.0 s), sample-accurate, chronological read-out on gate open.
- [x] VOX per `VOX-ALGORITHM.md` §1–4 (including its "Porting notes"): energy measurement, noise-floor history ring, threshold gate, attack/decay/hang.
- [x] Open/hang/close events match `vox_expected.json` sample for sample.

## 4. Resample & Mix
- [x] `rubato` replacing `resample_pcm_int16` (8 kHz to 48 kHz) within epsilon of the golden fixtures.
- [x] Loopback mixer: `STEREO_SPLIT` / `CENTER`, volume, 15 ms de-click gain ramps on duck/mute.

## 5. Encoders
- [x] WAV: PCM (`hound`) + hand-written chunks from `sc-metadata`.
- [x] MP3: `mp3lame-encoder` matching the Python `lameenc` bitrate/quality settings. (Cross-compile already proven in Phase 0.)
- [x] Output files parse with `sc-metadata`.

## 6. Optional / Post-Cutover
- LAN audio (RTSP over TCP + RTP/µ-law over UDP). Only in scope if the Python build supports it today. Otherwise track as a post-cutover feature (see Open Decisions in the master plan).

## Exit Gate
1. VOX and resample fixtures pass.
2. Real-time safety audit: no allocation, locks or blocking calls in any callback path.
3. 2-hour soak on Windows **and** Raspberry Pi: zero xruns logged.
4. Loopback is audibly click-free. MP3/WAV outputs pass `sc-metadata` parse and play in ProScan / ScanScribe.
