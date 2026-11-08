//! Real-time audio loopback mixer, dual-channel ring buffer, 15 ms de-click gain ramps,
//! and lock-free `rtrb` SPSC playback callback bridge.
//!
//! Ported from `AUDIO-IO-PIPELINE.md` §6 (`PlaybackProcesser.cs:400-418, 801-817, 1119`,
//! `Common_PS_RF.cs:254`) and `audio/recorder.py:98-332`.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::resample::resample_f32_linear;

/// Computes `ProScan`'s exact output volume multiplier (`PlaybackProcesser.cs:1119`).
///
/// * `0` -> `0.0` (true mute)
/// * `20` -> `0.316227766` (`-10 dB`)
/// * `30` -> `1.0` (`0 dB` unity default)
/// * `40` -> `3.16227766` (`+10 dB`)
/// * `60` -> `31.6227766` (`+30 dB`)
#[must_use]
pub fn scale_out_volume(value: i32) -> f64 {
    let clamped = value.clamp(0, 60);
    if clamped == 0 {
        return 0.0;
    }
    10.0_f64.powf((f64::from(clamped) - 30.0) / 20.0)
}

/// Applies `ProScan`'s gain multiplier and hard-clamps to 16-bit signed integer range (`Common_PS_RF.cs:254`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn limit_short_1(input: f64, level: f64) -> i16 {
    let scaled = (input * level).round();
    scaled.clamp(-32768.0, 32767.0) as i16
}

/// Stereo panning mode for dual-scanner loopback monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PanMode {
    /// Scanner A on Left channel (`0`), Scanner B on Right channel (`1`).
    StereoSplit = 0,
    /// Both scanners mixed to Center (`(A + B) * 0.707` on both Left and Right).
    Center = 1,
}

impl PanMode {
    /// Parses `"STEREO_SPLIT"` or `"CENTER"` (case-insensitive, defaults to [`PanMode::StereoSplit`]).
    #[must_use]
    pub fn from_str_name(name: &str) -> Self {
        if name.trim().eq_ignore_ascii_case("CENTER") {
            Self::Center
        } else {
            Self::StereoSplit
        }
    }

    /// Returns the wire string representation (`"STEREO_SPLIT"` or `"CENTER"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StereoSplit => "STEREO_SPLIT",
            Self::Center => "CENTER",
        }
    }

    #[must_use]
    const fn from_u8(val: u8) -> Self {
        if val == 1 {
            Self::Center
        } else {
            Self::StereoSplit
        }
    }
}

/// Click-free dual-channel ring buffer for time-aligned Scanner A (`0`) and Scanner B (`1`) audio.
///
/// Port of `AudioRingBuffer` from `audio/recorder.py:98-203`.
/// Recovers from consumer underruns by zero-padding deficits without entering a permanent
/// muting lockout (`test_underrun_recovery_no_lockout`).
#[derive(Debug, Clone)]
pub struct AudioRingBuffer {
    capacity: usize,
    channels: usize,
    buffer: Vec<Vec<f32>>,
    write_idx: Vec<usize>,
    read_idx: Vec<usize>,
    available_ch: Vec<usize>,
    min_prebuffer: usize,
    is_buffering_ch: Vec<bool>,
}

impl AudioRingBuffer {
    /// Creates a new `AudioRingBuffer` with `capacity` frames per channel, `channels` channels,
    /// and `min_prebuffer` initial pre-buffering threshold.
    #[must_use]
    pub fn new(capacity: usize, channels: usize, min_prebuffer: usize) -> Self {
        let cap = capacity.max(1);
        let ch = channels.max(1);
        Self {
            capacity: cap,
            channels: ch,
            buffer: vec![vec![0.0_f32; cap]; ch],
            write_idx: vec![0; ch],
            read_idx: vec![0; ch],
            available_ch: vec![0; ch],
            min_prebuffer,
            is_buffering_ch: vec![true; ch],
        }
    }

    /// Returns the maximum available samples across all channels.
    #[must_use]
    pub fn available(&self) -> usize {
        self.available_ch.iter().copied().max().unwrap_or(0)
    }

    /// Returns `true` if all channels are still in their initial pre-buffering state.
    #[must_use]
    pub fn is_buffering(&self) -> bool {
        self.is_buffering_ch.iter().all(|&b| b)
    }

    /// Explicitly sets the buffering state for all channels.
    pub fn set_is_buffering(&mut self, val: bool) {
        self.is_buffering_ch.fill(val);
    }

    /// Writes mono samples to a single channel (`0` = `scanner_a`, `1` = `scanner_b`).
    pub fn write_channel(&mut self, ch: usize, data: &[f32]) {
        if data.is_empty() || ch >= self.channels {
            return;
        }
        self.write_ch_internal(ch, data);
    }

    /// Writes stereo `[left, right]` frames across channels `0` and `1`.
    pub fn write_stereo_frames(&mut self, frames: &[[f32; 2]]) {
        if frames.is_empty() {
            return;
        }
        let mut ch0 = Vec::with_capacity(frames.len());
        let mut ch1 = Vec::with_capacity(frames.len());
        for frame in frames {
            ch0.push(frame[0]);
            ch1.push(frame[1]);
        }
        self.write_ch_internal(0, &ch0);
        if self.channels > 1 {
            self.write_ch_internal(1, &ch1);
        }
    }

    fn write_ch_internal(&mut self, ch: usize, data: &[f32]) {
        let n = data.len();
        if n == 0 {
            return;
        }
        if n >= self.capacity {
            let tail = &data[n - self.capacity..];
            self.buffer[ch].copy_from_slice(tail);
            self.write_idx[ch] = 0;
            self.read_idx[ch] = 0;
            self.available_ch[ch] = self.capacity;
            if self.is_buffering_ch[ch] && self.available_ch[ch] >= self.min_prebuffer {
                self.is_buffering_ch[ch] = false;
            }
            return;
        }

        let mut avail = self.available_ch[ch];
        let w_idx = self.write_idx[ch];
        if n > self.capacity - avail {
            let excess = n - (self.capacity - avail);
            self.read_idx[ch] = (self.read_idx[ch] + excess) % self.capacity;
            avail -= excess;
        }

        let end = w_idx + n;
        if end <= self.capacity {
            self.buffer[ch][w_idx..end].copy_from_slice(data);
        } else {
            let first = self.capacity - w_idx;
            self.buffer[ch][w_idx..].copy_from_slice(&data[..first]);
            let rem = end % self.capacity;
            self.buffer[ch][..rem].copy_from_slice(&data[first..]);
        }
        self.write_idx[ch] = end % self.capacity;
        self.available_ch[ch] = avail + n;
        if self.is_buffering_ch[ch] && self.available_ch[ch] >= self.min_prebuffer {
            self.is_buffering_ch[ch] = false;
        }
    }

    /// Reads time-aligned stereo frames for `outdata`, zero-padding on underrun and mixing
    /// according to `pan_mode`.
    #[allow(clippy::needless_range_loop)]
    pub fn read_stereo_frames(&mut self, outdata: &mut [[f32; 2]], pan_mode: PanMode) {
        let frames = outdata.len();
        for frame in outdata.iter_mut() {
            *frame = [0.0, 0.0];
        }
        if frames == 0 {
            return;
        }

        let num_ch = self.channels.min(2);
        for ch in 0..num_ch {
            let avail = self.available_ch[ch];
            if self.is_buffering_ch[ch] || avail == 0 {
                continue;
            }
            let to_read = frames.min(avail);
            let r_idx = self.read_idx[ch];
            let end = r_idx + to_read;
            if end <= self.capacity {
                for (i, &sample) in self.buffer[ch][r_idx..end].iter().enumerate() {
                    outdata[i][ch] = sample;
                }
            } else {
                let first = self.capacity - r_idx;
                for (i, &sample) in self.buffer[ch][r_idx..].iter().enumerate() {
                    outdata[i][ch] = sample;
                }
                let rem = end % self.capacity;
                for (i, &sample) in self.buffer[ch][..rem].iter().enumerate() {
                    outdata[first + i][ch] = sample;
                }
            }
            self.read_idx[ch] = end % self.capacity;
            self.available_ch[ch] -= to_read;
        }

        if pan_mode == PanMode::Center && num_ch >= 2 {
            for frame in outdata.iter_mut() {
                let mixed = ((frame[0] + frame[1]) * 0.707).clamp(-1.0, 1.0);
                frame[0] = mixed;
                frame[1] = mixed;
            }
        }
    }
}

/// Deterministic loopback mixer and 15 ms de-click gain ramp processor.
///
/// Port of `AudioLoopbackMonitor` from `audio/recorder.py:205-332`.
#[derive(Debug, Clone)]
pub struct LoopbackMixer {
    pub enabled: bool,
    pub volume: f32,
    pub pan_mode: PanMode,
    pub samplerate: u32,
    pub ring_buffer: AudioRingBuffer,
    current_gain_a: f32,
    target_gain_a: f32,
    current_gain_b: f32,
    target_gain_b: f32,
    ramp_samples: usize,
}

impl LoopbackMixer {
    /// Creates a new `LoopbackMixer` at `samplerate` Hz (default `44100` Hz, 15 ms ramp = 661 samples).
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn new(enabled: bool, volume: f32, pan_mode: PanMode, samplerate: u32) -> Self {
        let sr = samplerate.max(8000);
        let ramp_samples = (f64::from(sr) * 0.015) as usize;
        Self {
            enabled,
            volume: volume.clamp(0.0, 1.0),
            pan_mode,
            samplerate: sr,
            ring_buffer: AudioRingBuffer::new(96_000, 2, 2048),
            current_gain_a: 1.0,
            target_gain_a: 1.0,
            current_gain_b: 1.0,
            target_gain_b: 1.0,
            ramp_samples: ramp_samples.max(1),
        }
    }

    /// Returns the number of samples in a 15 ms de-click gain ramp (`661` at `44100` Hz).
    #[must_use]
    pub const fn ramp_samples(&self) -> usize {
        self.ramp_samples
    }

    /// Returns the current target gain (`0.0` muted, `1.0` unmuted) for `scanner_id`.
    #[must_use]
    pub fn target_gain(&self, scanner_id: &str) -> f32 {
        if scanner_id == "scanner_a" {
            self.target_gain_a
        } else {
            self.target_gain_b
        }
    }

    /// Returns the current instantaneous gain (`0.0..=1.0`) for `scanner_id`.
    #[must_use]
    pub fn current_gain(&self, scanner_id: &str) -> f32 {
        if scanner_id == "scanner_a" {
            self.current_gain_a
        } else {
            self.current_gain_b
        }
    }

    /// Sets the target gain for `scanner_id` to `0.0` (`mute = true`) or `1.0` (`mute = false`).
    /// Gain ramps smoothly over 15 ms (`ramp_samples`) on subsequent `push_audio` calls.
    pub fn mute_scanner(&mut self, scanner_id: &str, mute: bool) {
        let target = if mute { 0.0 } else { 1.0 };
        if scanner_id == "scanner_a" {
            self.target_gain_a = target;
        } else {
            self.target_gain_b = target;
        }
    }

    /// Pushes mono `f32` audio from `scanner_id` captured at `in_rate` Hz into the loopback mixer,
    /// applying sample-rate conversion (if needed), volume scaling, and 15 ms de-click gain ramping.
    ///
    /// Returns the processed samples written to the channel (or empty if muted/disabled).
    #[allow(clippy::cast_precision_loss)]
    pub fn push_audio(&mut self, scanner_id: &str, mono: &[f32], in_rate: u32) -> Vec<f32> {
        if !self.enabled || mono.is_empty() {
            return Vec::new();
        }

        let (start_g, target_g) = if scanner_id == "scanner_a" {
            (self.current_gain_a, self.target_gain_a)
        } else {
            (self.current_gain_b, self.target_gain_b)
        };

        // If fully muted and not ramping, drop audio without processing (audio/recorder.py:300)
        if start_g == 0.0 && target_g == 0.0 {
            return Vec::new();
        }

        let mut scaled: Vec<f32> = if in_rate == self.samplerate {
            mono.iter().map(|&s| s * self.volume).collect()
        } else {
            let mut resampled = resample_f32_linear(mono, in_rate, self.samplerate);
            for s in &mut resampled {
                *s *= self.volume;
            }
            resampled
        };

        if scaled.is_empty() {
            return Vec::new();
        }

        // Smooth de-click gain ramping (15ms crossfade on mute/unmute/duck)
        if (start_g - target_g).abs() > f32::EPSILON {
            let ramp_len = self.ramp_samples.max(1) as f32;
            let step = (target_g - start_g) / ramp_len;
            let min_g = start_g.min(target_g);
            let max_g = start_g.max(target_g);
            let mut last_g = start_g;
            for (idx, sample) in scaled.iter_mut().enumerate() {
                let g = (start_g + ((idx + 1) as f32) * step).clamp(min_g, max_g);
                *sample *= g;
                last_g = g;
            }
            if scanner_id == "scanner_a" {
                self.current_gain_a = last_g;
            } else {
                self.current_gain_b = last_g;
            }
        } else if start_g < 1.0 {
            for sample in &mut scaled {
                *sample *= start_g;
            }
        }

        let ch_idx = usize::from(scanner_id != "scanner_a");
        self.ring_buffer.write_channel(ch_idx, &scaled);
        scaled
    }
}

/// Shared atomic flags between the worker threads and the real-time `cpal` output callback.
#[derive(Debug)]
pub struct RtLoopbackShared {
    pub buffering_a: AtomicBool,
    pub buffering_b: AtomicBool,
    pub pan_mode: AtomicU8,
    pub min_prebuffer: usize,
}

impl RtLoopbackShared {
    /// Creates new shared atomic state for real-time loopback playback.
    #[must_use]
    pub fn new(pan_mode: PanMode, min_prebuffer: usize) -> Arc<Self> {
        Arc::new(Self {
            buffering_a: AtomicBool::new(true),
            buffering_b: AtomicBool::new(true),
            pan_mode: AtomicU8::new(pan_mode as u8),
            min_prebuffer,
        })
    }
}

/// Real-time safe (`AGENTS.md` §2) output callback filler.
///
/// * **Zero heap allocations**
/// * **Zero `Mutex` / `RwLock`**
/// * **Zero logging or syscalls**
/// * Uses only lock-free `rtrb::Consumer<f32>` and `Ordering::Relaxed` atomics.
pub fn fill_rt_output_callback(
    out_interleaved: &mut [f32],
    out_channels: usize,
    consumer_a: &mut rtrb::Consumer<f32>,
    consumer_b: &mut rtrb::Consumer<f32>,
    shared: &RtLoopbackShared,
) {
    for sample in out_interleaved.iter_mut() {
        *sample = 0.0;
    }
    if out_channels == 0 || out_interleaved.is_empty() {
        return;
    }

    let avail_a = consumer_a.slots();
    if shared.buffering_a.load(Ordering::Relaxed) && avail_a >= shared.min_prebuffer {
        shared.buffering_a.store(false, Ordering::Relaxed);
    }
    let avail_b = consumer_b.slots();
    if shared.buffering_b.load(Ordering::Relaxed) && avail_b >= shared.min_prebuffer {
        shared.buffering_b.store(false, Ordering::Relaxed);
    }

    let read_a = !shared.buffering_a.load(Ordering::Relaxed);
    let read_b = !shared.buffering_b.load(Ordering::Relaxed);
    let pan = PanMode::from_u8(shared.pan_mode.load(Ordering::Relaxed));

    for frame in out_interleaved.chunks_exact_mut(out_channels) {
        let sa = if read_a {
            consumer_a.pop().unwrap_or(0.0)
        } else {
            0.0
        };
        let sb = if read_b {
            consumer_b.pop().unwrap_or(0.0)
        } else {
            0.0
        };

        if out_channels >= 2 {
            match pan {
                PanMode::StereoSplit => {
                    frame[0] = sa;
                    frame[1] = sb;
                }
                PanMode::Center => {
                    let mixed = ((sa + sb) * 0.707).clamp(-1.0, 1.0);
                    frame[0] = mixed;
                    frame[1] = mixed;
                }
            }
        } else {
            frame[0] = ((sa + sb) * 0.707).clamp(-1.0, 1.0);
        }
    }
}
