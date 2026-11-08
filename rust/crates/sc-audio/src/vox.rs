//! `ProScan` VOX / audio-level squelch arithmetic and sample-accurate VOX gate.
//!
//! Ported from `VOX-ALGORITHM.md` §1–4 (`WaveIn.cs:855-1217`, `RecorderVOX.cs:43-140`).

use std::collections::VecDeque;

/// Default `ProScan` VOX threshold on the `0..=90` scale (`30` == `-60 dBFS`).
pub const DEFAULT_PROSCAN_VOX_THRESHOLD: i32 = 30;

/// Default `ProScan` VOX hang time in milliseconds (`2000 ms` per `RecorderVOX.cs:120`).
pub const DEFAULT_PROSCAN_VOX_HANG_MS: u64 = 2000;

/// Computes `ProScan`'s peak-equivalent RMS (`round(sqrt(mean(x^2) * 2.0))`) for a buffer of `i16` samples.
///
/// Provenance: `WaveIn.cs:989-993` (`VOX-ALGORITHM.md` §1).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn compute_peak_equiv_rms(samples: &[i16]) -> i32 {
    if samples.is_empty() {
        return 0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|&s| {
            let v = f64::from(s);
            v * v
        })
        .sum();
    let mean_sq = sum_sq / (samples.len() as f64);
    let scaled = mean_sq * 2.0;
    if scaled >= 0.0 {
        scaled.sqrt().round() as i32
    } else {
        0
    }
}

/// Converts a linear peak-equivalent RMS average (`0..=32767`) into `ProScan`'s `0..=90` VOX meter scale,
/// where `0` is `<= -90 dBFS` and `90` is `0 dBFS`.
///
/// Provenance: `WaveIn.cs:1083-1099` (`VOX-ALGORITHM.md` §2).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn rms_to_vox_meter(avg_rms: i32) -> i32 {
    let mut db = if avg_rms > 0 {
        (-20.0 * (32767.0 / f64::from(avg_rms)).log10()).round() as i32
    } else {
        -90
    };
    db = db.clamp(-90, 0);
    db + 90
}

/// Per-stream `ProScan` VOX engine implementing the exact history ring, 0.15s averaging window,
/// dBFS conversion (`0..=90`), 0-buffer attack, and hang-time latch from `VOX-ALGORITHM.md` §1–4.
///
/// Note (`VOX-ALGORITHM.md` §5): Unlike `ProScan`'s VB `Static` locals which accidentally shared state
/// across streams, each `ProScanVoxEngine` owns its own isolated per-channel history rings.
#[derive(Debug, Clone)]
pub struct ProScanVoxEngine {
    left_history: VecDeque<i32>,
    right_history: VecDeque<i32>,
    buffers_per_sec: usize,
    threshold_level: i32,
    hang_time_ms: u64,
    threshold_triggered: bool,
    is_latched: bool,
    elapsed_ms: u64,
    last_trigger_ms: Option<u64>,
}

impl ProScanVoxEngine {
    /// Creates a new `ProScanVoxEngine`.
    ///
    /// * `buffers_per_sec`: Expected callback buffer rate (`num3` in `WaveIn.cs:865`, e.g., `10` for 100ms buffers or `100` for 10ms buffers).
    /// * `threshold_level`: Threshold on the `0..=90` scale (`threshold_dBFS = threshold_level - 90`).
    /// * `hang_time_ms`: Latch hold duration after the last above-threshold buffer (`2000` ms in `ProScan`).
    #[must_use]
    pub fn new(buffers_per_sec: usize, threshold_level: i32, hang_time_ms: u64) -> Self {
        let bps = buffers_per_sec.max(1);
        Self {
            left_history: VecDeque::with_capacity(bps * 16),
            right_history: VecDeque::with_capacity(bps * 16),
            buffers_per_sec: bps,
            threshold_level: threshold_level.clamp(0, 90),
            hang_time_ms,
            threshold_triggered: false,
            is_latched: false,
            elapsed_ms: 0,
            last_trigger_ms: None,
        }
    }

    /// Updates the VOX threshold (`0..=90`).
    pub fn set_threshold_level(&mut self, level: i32) {
        self.threshold_level = level.clamp(0, 90);
    }

    /// Returns the current VOX threshold (`0..=90`).
    #[must_use]
    pub const fn threshold_level(&self) -> i32 {
        self.threshold_level
    }

    /// Returns `true` if the most recent buffer was at or above `threshold_level`.
    #[must_use]
    pub const fn is_threshold_triggered(&self) -> bool {
        self.threshold_triggered
    }

    /// Returns `true` if the VOX gate is currently latched open (including hang time).
    #[must_use]
    pub const fn is_latched(&self) -> bool {
        self.is_latched
    }

    /// Resets the VOX latch and trigger state (`RecorderVOX.cs:136-140`).
    pub fn reset(&mut self) {
        self.threshold_triggered = false;
        self.is_latched = false;
        self.last_trigger_ms = None;
    }

    /// Feeds a mono buffer of `i16` samples taking `buffer_duration_ms` milliseconds
    /// and returns `(vox_meter_0_to_90, is_latched)`.
    pub fn process_mono_buffer(&mut self, samples: &[i16], buffer_duration_ms: u64) -> (i32, bool) {
        let item = compute_peak_equiv_rms(samples);
        let meter = self.push_history_and_compute_vox(item, 0);
        self.evaluate_gate(meter, buffer_duration_ms);
        (meter, self.is_latched)
    }

    /// Feeds separate left and right channel buffers of `i16` samples and returns
    /// `(max_vox_meter_0_to_90, is_latched)`.
    pub fn process_stereo_buffers(
        &mut self,
        left: &[i16],
        right: &[i16],
        buffer_duration_ms: u64,
    ) -> (i32, bool) {
        let item_l = compute_peak_equiv_rms(left);
        let item_r = compute_peak_equiv_rms(right);
        let meter = self.push_history_and_compute_vox(item_l, item_r);
        self.evaluate_gate(meter, buffer_duration_ms);
        (meter, self.is_latched)
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn push_history_and_compute_vox(&mut self, item_left: i32, item_right: i32) -> i32 {
        let max_cap = self.buffers_per_sec * 1200; // 20 minutes per WaveIn.cs:995
        self.left_history.push_back(item_left);
        if self.left_history.len() > max_cap {
            self.left_history.pop_front();
        }
        self.right_history.push_back(item_right);
        if self.right_history.len() > max_cap {
            self.right_history.pop_front();
        }

        // Window length = round(buffers_per_sec * 0.15) + 1 (WaveIn.cs:1039)
        let window_items = ((self.buffers_per_sec as f64) * 0.15).round() as usize + 1;
        let left_vox = Self::window_vox_meter(&self.left_history, window_items);
        let right_vox = Self::window_vox_meter(&self.right_history, window_items);
        left_vox.max(right_vox)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn window_vox_meter(history: &VecDeque<i32>, window_items: usize) -> i32 {
        if history.is_empty() {
            return 0;
        }
        let take_count = history.len().min(window_items.max(1));
        let sum: f64 = history
            .iter()
            .rev()
            .take(take_count)
            .map(|&v| f64::from(v))
            .sum();
        let avg = (sum / (take_count as f64)).round() as i32;
        rms_to_vox_meter(avg)
    }

    fn evaluate_gate(&mut self, meter_value: i32, buffer_duration_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(buffer_duration_ms);
        if meter_value >= self.threshold_level {
            self.threshold_triggered = true;
            self.last_trigger_ms = Some(self.elapsed_ms);
            if !self.is_latched {
                self.is_latched = true;
            }
        } else {
            self.threshold_triggered = false;
        }

        if self.is_latched {
            if let Some(last_hit) = self.last_trigger_ms {
                if self.elapsed_ms.saturating_sub(last_hit) >= self.hang_time_ms {
                    self.is_latched = false;
                }
            }
        }
    }
}

/// Transition event emitted by [`VoxGate`] during sample-accurate stream processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoxEvent {
    /// VOX gate latched open at the given sample index.
    GateOpen { sample_index: usize },
    /// VOX gate unlatched (closed) at the given sample index after hang time expired.
    GateClose { sample_index: usize },
}

/// Sample-accurate VOX gate analyzer for mono 16-bit PCM streams.
///
/// Evaluates 10 ms blocks using `ProScan`'s peak-equivalent RMS and `0..=90` dBFS meter scale
/// (`VOX-ALGORITHM.md` §1–4), tracking exact sample indices of gate-open and gate-close transitions.
#[derive(Debug, Clone)]
pub struct VoxGate {
    sample_rate: u32,
    block_samples: usize,
    threshold_meter: i32,
    hang_samples: usize,
    latched: bool,
    samples_since_last_voice: usize,
    processed_samples: usize,
}

impl VoxGate {
    /// Creates a new `VoxGate` configured for `sample_rate` Hz, `threshold_meter` (`0..=90`),
    /// and `hang_time_ms` milliseconds.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn new(sample_rate: u32, threshold_meter: i32, hang_time_ms: u64) -> Self {
        let sr = sample_rate.max(100) as usize;
        // 10 ms block (e.g. 160 samples at 16 kHz) per VOX-ALGORITHM.md Porting Notes
        let block_samples = (sr / 100).max(1);
        let hang_samples = ((sr as u64 * hang_time_ms) / 1000) as usize;
        Self {
            sample_rate,
            block_samples,
            threshold_meter: threshold_meter.clamp(0, 90),
            hang_samples,
            latched: false,
            samples_since_last_voice: 0,
            processed_samples: 0,
        }
    }

    /// Creates a `VoxGate` with a sensible voice threshold (`65` on the `0..=90` scale, `-25 dBFS`)
    /// that rejects `-33 dBFS` background noise (`std = 500`) while latching immediately on active speech.
    #[must_use]
    pub fn with_default_voice_threshold(sample_rate: u32, hang_time_ms: u64) -> Self {
        Self::new(sample_rate, 65, hang_time_ms)
    }

    /// Returns the configured sample rate in Hz.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns `true` if the gate is currently open (latched).
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.latched
    }

    /// Processes a contiguous slice of `i16` PCM samples and returns all [`VoxEvent`] transitions
    /// with their exact sample offsets.
    #[must_use]
    pub fn process_samples(&mut self, samples: &[i16]) -> Vec<VoxEvent> {
        let mut events = Vec::new();
        for chunk in samples.chunks(self.block_samples) {
            let block_start = self.processed_samples;
            let chunk_len = chunk.len();
            let rms = compute_peak_equiv_rms(chunk);
            let meter = rms_to_vox_meter(rms);

            if meter >= self.threshold_meter {
                self.samples_since_last_voice = 0;
                if !self.latched {
                    self.latched = true;
                    events.push(VoxEvent::GateOpen {
                        sample_index: block_start,
                    });
                }
            } else if self.latched {
                self.samples_since_last_voice += chunk_len;
                if self.samples_since_last_voice >= self.hang_samples {
                    self.latched = false;
                    let close_sample = (block_start + chunk_len)
                        - (self.samples_since_last_voice - self.hang_samples);
                    events.push(VoxEvent::GateClose {
                        sample_index: close_sample,
                    });
                    self.samples_since_last_voice = 0;
                }
            }

            self.processed_samples += chunk_len;
        }
        events
    }
}
