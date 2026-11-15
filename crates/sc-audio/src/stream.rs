//! Per-scanner audio capture stream, session state machine, and multi-scanner recorder manager.
//!
//! Ported from `audio/recorder.py:334-757` while strictly enforcing `AGENTS.md` §2 real-time
//! callback rules (zero allocation, zero locks, zero logging, zero syscalls inside `cpal` callbacks).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use sc_core::{AudioChannelMode, ScannerStatus};

use crate::devices::{resolve_input_device, resolve_output_device};
use crate::loopback::{fill_rt_output_callback, LoopbackMixer, PanMode, RtLoopbackShared};
use crate::preroll::PreRollRingBuffer;

/// Returns current UNIX epoch timestamp in seconds as `f64`.
#[must_use]
pub fn now_unix_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Returns `true` if `channel_name` is empty or a generic placeholder such as `"Scanning..."`.
///
/// Ported from `audio/recorder.py:356-362`.
#[must_use]
pub fn is_generic_channel(channel_name: &str) -> bool {
    matches!(
        channel_name.trim().to_ascii_lowercase().as_str(),
        "" | "scanning"
            | "scanning..."
            | "search"
            | "custom search"
            | "service search"
            | "none"
            | "blank"
            | "active call"
            | "channel"
            | "active channel"
            | "unknown channel"
    )
}

/// Returns `true` if `system_name` is empty or a generic placeholder such as `"Scanning..."`.
///
/// Ported from `audio/recorder.py:365-367`.
#[must_use]
pub fn is_generic_system(system_name: &str) -> bool {
    matches!(
        system_name.trim().to_ascii_lowercase().as_str(),
        "" | "scanning"
            | "scanning..."
            | "search"
            | "custom search"
            | "service search"
            | "none"
            | "blank"
    )
}

/// Returns `true` if `new_status` contains richer or non-generic metadata than `old_status`.
///
/// Ported from `audio/recorder.py:370-407`.
#[must_use]
pub fn should_update_session_status(
    old_status: &ScannerStatus,
    new_status: &ScannerStatus,
) -> bool {
    let old_ch_generic = is_generic_channel(&old_status.channel_name);
    let new_ch_generic = is_generic_channel(&new_status.channel_name);
    if old_ch_generic && !new_ch_generic {
        return true;
    }

    let old_sys_generic = is_generic_system(&old_status.system_name);
    let new_sys_generic = is_generic_system(&new_status.system_name);
    if old_sys_generic && !new_sys_generic {
        return true;
    }

    if old_status.tgid.as_deref().unwrap_or("").is_empty()
        && new_status.tgid.as_deref().is_some_and(|s| !s.is_empty())
    {
        return true;
    }

    if old_status.unit_id.as_deref().unwrap_or("").is_empty()
        && new_status.unit_id.as_deref().is_some_and(|s| !s.is_empty())
    {
        return true;
    }

    if old_status.frequency.is_none() && new_status.frequency.is_some() {
        return true;
    }

    if old_status.dept_name.trim().is_empty() && !new_status.dept_name.trim().is_empty() {
        return true;
    }

    false
}

/// Returns `true` if `status` contains valid channel/system or frequency/TGID metadata.
///
/// Ported from `audio/recorder.py:410-414`.
#[must_use]
pub fn is_valid_metadata_status(status: &ScannerStatus) -> bool {
    !is_generic_channel(&status.channel_name)
        || !is_generic_system(&status.system_name)
        || status.tgid.as_deref().is_some_and(|s| !s.is_empty())
        || status.frequency.is_some()
}

/// Real-time safe (`AGENTS.md` §2) input callback demuxer and SPSC producer.
///
/// * **Zero heap allocations**
/// * **Zero `Mutex` / `RwLock`**
/// * **Zero logging or syscalls**
/// * Demuxes interleaved input frames according to `channel_mode`, computes the RMS VU meter
///   percentage into `live_level_pct`, and pushes mono `f32` samples into `producer`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn push_rt_input_callback(
    interleaved: &[f32],
    channels: usize,
    channel_mode: AudioChannelMode,
    producer: &mut rtrb::Producer<f32>,
    live_level_pct: &AtomicU8,
    overflow_count: &AtomicU64,
) {
    if channels == 0 || interleaved.is_empty() {
        return;
    }

    let mut sum_sq = 0.0_f32;
    let mut frame_count = 0usize;

    for frame in interleaved.chunks_exact(channels) {
        let mono = if channels > 1 {
            match channel_mode {
                AudioChannelMode::Left => frame[0],
                AudioChannelMode::Right => frame[1],
                AudioChannelMode::Mono => 0.5 * (frame[0] + frame[1]),
            }
        } else {
            frame[0]
        };

        sum_sq += mono * mono;
        frame_count += 1;

        if producer.push(mono).is_err() {
            overflow_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    if frame_count > 0 {
        let rms = (sum_sq / (frame_count as f32)).sqrt();
        let pct = ((rms * 250.0) as u32).min(100) as u8;
        live_level_pct.store(pct, Ordering::Relaxed);
    }
}

/// Tracks an ongoing transmission's audio buffer and radio metadata snapshot.
///
/// Ported from `ActiveCallSession` in `audio/recorder.py:334-354`.
#[derive(Debug, Clone)]
pub struct ActiveCallSession {
    pub scanner_id: String,
    pub start_time: f64,
    pub last_active_time: f64,
    pub status: ScannerStatus,
    pub audio_chunks: Vec<Vec<u8>>,
    pub closed: bool,
}

impl ActiveCallSession {
    /// Creates a new `ActiveCallSession` initialized with `pre_buffer` PCM bytes.
    #[must_use]
    pub fn new(
        scanner_id: impl Into<String>,
        status_snapshot: ScannerStatus,
        pre_buffer: Vec<u8>,
    ) -> Self {
        let now = now_unix_secs();
        let audio_chunks = if pre_buffer.is_empty() {
            Vec::new()
        } else {
            vec![pre_buffer]
        };
        Self {
            scanner_id: scanner_id.into(),
            start_time: now,
            last_active_time: now,
            status: status_snapshot,
            audio_chunks,
            closed: false,
        }
    }

    /// Appends raw 16-bit little-endian PCM bytes to the active call session and refreshes `last_active_time`.
    pub fn append(&mut self, pcm_bytes: &[u8]) {
        if !pcm_bytes.is_empty() {
            self.audio_chunks.push(pcm_bytes.to_vec());
        }
        self.last_active_time = now_unix_secs();
    }

    /// Concatenates and returns all buffered PCM bytes for this call session.
    #[must_use]
    pub fn get_full_pcm(&self) -> Vec<u8> {
        let total_len: usize = self.audio_chunks.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(total_len);
        for chunk in &self.audio_chunks {
            out.extend_from_slice(chunk);
        }
        out
    }

    /// Returns the transmission duration in seconds (`last_active_time - start_time`).
    #[must_use]
    pub fn duration(&self) -> f64 {
        (self.last_active_time - self.start_time).max(0.0)
    }
}

/// Completed transmission payload emitted when a recording session's hang time expires.
#[derive(Debug, Clone)]
pub struct CompletedCall {
    pub scanner_id: String,
    pub status: ScannerStatus,
    pub pcm_bytes: Vec<u8>,
    pub duration_seconds: f64,
    pub sample_rate: u32,
}

/// Callback signature invoked when a call transmission finishes hang time and is ready for dispatch.
pub type DispatchCallback = Arc<dyn Fn(CompletedCall) + Send + Sync>;

/// Internal mutable recording state shared between the per-scanner worker thread and control calls.
/// Never touched by the real-time `cpal` callback thread.
struct StreamSessionState {
    pre_buffer: PreRollRingBuffer,
    active_session: Option<ActiveCallSession>,
    pending_release: Option<ActiveCallSession>,
    recording_enabled: bool,
}

/// Dedicated audio capture stream and session state machine for a single scanner.
///
/// Ported from `SingleScannerAudioStream` (`audio/recorder.py:417-636`).
pub struct SingleScannerAudioStream {
    pub scanner_id: String,
    pub device_index: Option<u32>,
    pub device_name: Option<String>,
    pub channel_mode: AudioChannelMode,
    pub sample_rate: u32,
    pub hang_time_seconds: f64,
    pub min_duration_seconds: f64,
    pub enabled: bool,
    dispatch_callback: Option<DispatchCallback>,
    state: Arc<Mutex<StreamSessionState>>,
    live_level_pct: Arc<AtomicU8>,
    xrun_count: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
}

impl SingleScannerAudioStream {
    /// Creates a new `SingleScannerAudioStream`.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn new(
        scanner_id: impl Into<String>,
        device_index: Option<u32>,
        device_name: Option<String>,
        channel_mode: AudioChannelMode,
        sample_rate: u32,
        hang_time_seconds: f64,
        min_duration_seconds: f64,
        enabled: bool,
        dispatch_callback: Option<DispatchCallback>,
    ) -> Self {
        let sr = sample_rate.max(8000);
        let pre_cap = (f64::from(sr) * 2.0) as usize;
        Self {
            scanner_id: scanner_id.into(),
            device_index,
            device_name,
            channel_mode,
            sample_rate: sr,
            hang_time_seconds,
            min_duration_seconds,
            enabled,
            dispatch_callback,
            state: Arc::new(Mutex::new(StreamSessionState {
                pre_buffer: PreRollRingBuffer::new(pre_cap),
                active_session: None,
                pending_release: None,
                recording_enabled: true,
            })),
            live_level_pct: Arc::new(AtomicU8::new(0)),
            xrun_count: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
        }
    }

    /// Sets whether recording is enabled for this scanner stream.
    pub fn set_recording_enabled(&self, enabled: bool) {
        if let Ok(mut guard) = self.state.lock() {
            guard.recording_enabled = enabled;
        }
    }

    /// Returns the live VU meter percentage (`0..=100`).
    #[must_use]
    pub fn live_level_pct(&self) -> u8 {
        if self.enabled && self.running.load(Ordering::Relaxed) {
            self.live_level_pct.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    /// Returns the number of callback buffer overflows or stream errors observed.
    #[must_use]
    pub fn xrun_count(&self) -> u64 {
        self.xrun_count.load(Ordering::Relaxed)
    }

    /// Returns `true` if an `ActiveCallSession` is currently open.
    #[must_use]
    pub fn has_active_session(&self) -> bool {
        self.state.lock().is_ok_and(|g| g.active_session.is_some())
    }

    /// Returns `true` if a session is waiting in its hang-time window (`pending_release`).
    #[must_use]
    pub fn has_pending_release(&self) -> bool {
        self.state.lock().is_ok_and(|g| g.pending_release.is_some())
    }

    /// Feeds raw 16-bit little-endian PCM bytes directly into the pre-roll buffer and active session
    /// (used by deterministic tests and worker threads).
    pub fn feed_pcm_bytes(&self, pcm_bytes: &[u8]) {
        if pcm_bytes.len() < 2 {
            return;
        }
        let mut samples = Vec::with_capacity(pcm_bytes.len() / 2);
        for pair in pcm_bytes.chunks_exact(2) {
            samples.push(i16::from_le_bytes([pair[0], pair[1]]));
        }
        if let Ok(mut guard) = self.state.lock() {
            if !guard.recording_enabled {
                return;
            }
            guard.pre_buffer.write(&samples);
            if let Some(session) = &mut guard.active_session {
                session.append(pcm_bytes);
            }
        }
    }

    /// Starts the hardware `cpal` input stream and its dedicated worker thread.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn start(
        &mut self,
        loopback_mixer: Option<Arc<Mutex<LoopbackMixer>>>,
        mut rt_loopback_producer: Option<rtrb::Producer<f32>>,
    ) {
        if self.running.load(Ordering::Relaxed) || !self.enabled {
            return;
        }

        let Some(device) = resolve_input_device(self.device_name.as_deref(), self.device_index)
        else {
            tracing::warn!(
                scanner_id = %self.scanner_id,
                "No audio input device available; capture stream dormant"
            );
            return;
        };

        let Ok(default_cfg) = device.default_input_config() else {
            tracing::warn!(
                scanner_id = %self.scanner_id,
                "Could not query default input config; capture stream dormant"
            );
            return;
        };

        let hw_rate = default_cfg.sample_rate().0;
        let hw_channels = default_cfg.channels().clamp(1, 2);
        let sample_format = default_cfg.sample_format();
        self.sample_rate = hw_rate;

        if let Ok(mut guard) = self.state.lock() {
            guard.pre_buffer = PreRollRingBuffer::new((f64::from(hw_rate) * 2.0) as usize);
        }

        let ring_capacity = (hw_rate as usize) * 2;
        let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(ring_capacity);

        let running_flag = Arc::clone(&self.running);
        running_flag.store(true, Ordering::SeqCst);

        let level_cb = Arc::clone(&self.live_level_pct);
        let xrun_cb = Arc::clone(&self.xrun_count);
        let err_xrun = Arc::clone(&self.xrun_count);
        let ch_mode = self.channel_mode;
        let ch_usize = usize::from(hw_channels);
        let dev_name = self.device_name.clone();
        let dev_idx = self.device_index;

        // Spawn dedicated worker thread to own the cpal::Stream (!Send on WASAPI) and drain the SPSC ring
        let worker_running = Arc::clone(&self.running);
        let worker_state = Arc::clone(&self.state);
        let scanner_id_clone = self.scanner_id.clone();
        let handle = thread::Builder::new()
            .name(format!("sc-audio-{}", self.scanner_id))
            .spawn(move || {
                let Some(thread_dev) = resolve_input_device(dev_name.as_deref(), dev_idx) else {
                    worker_running.store(false, Ordering::SeqCst);
                    return;
                };
                let stream_cfg = StreamConfig {
                    channels: hw_channels,
                    sample_rate: cpal::SampleRate(hw_rate),
                    buffer_size: cpal::BufferSize::Default,
                };
                let stream_res = match sample_format {
                    SampleFormat::F32 => thread_dev.build_input_stream(
                        &stream_cfg,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            push_rt_input_callback(
                                data,
                                ch_usize,
                                ch_mode,
                                &mut producer,
                                &level_cb,
                                &xrun_cb,
                            );
                        },
                        move |_err| {
                            err_xrun.fetch_add(1, Ordering::Relaxed);
                        },
                        None,
                    ),
                    SampleFormat::I16 => {
                        let mut f32_scratch = [0.0_f32; 4096];
                        thread_dev.build_input_stream(
                            &stream_cfg,
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                for chunk in data.chunks(f32_scratch.len()) {
                                    for (dst, &src) in f32_scratch.iter_mut().zip(chunk.iter()) {
                                        *dst = f32::from(src) / 32768.0;
                                    }
                                    push_rt_input_callback(
                                        &f32_scratch[..chunk.len()],
                                        ch_usize,
                                        ch_mode,
                                        &mut producer,
                                        &level_cb,
                                        &xrun_cb,
                                    );
                                }
                            },
                            move |_err| {
                                err_xrun.fetch_add(1, Ordering::Relaxed);
                            },
                            None,
                        )
                    }
                    _ => {
                        worker_running.store(false, Ordering::SeqCst);
                        return;
                    }
                };

                let Ok(stream) = stream_res else {
                    worker_running.store(false, Ordering::SeqCst);
                    return;
                };
                if stream.play().is_err() {
                    worker_running.store(false, Ordering::SeqCst);
                    return;
                }
                let _stream_guard = stream;

                let mut f32_batch = Vec::with_capacity(1024);
                let mut i16_batch = Vec::with_capacity(1024);
                let mut pcm_bytes = Vec::with_capacity(2048);

                while worker_running.load(Ordering::Relaxed) {
                    let avail = consumer.slots();
                    if avail == 0 {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }

                    f32_batch.clear();
                    i16_batch.clear();
                    pcm_bytes.clear();

                    let to_pop = avail.min(2048);
                    for _ in 0..to_pop {
                        if let Ok(sample) = consumer.pop() {
                            f32_batch.push(sample);
                            let clamped = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                            i16_batch.push(clamped);
                            pcm_bytes.extend_from_slice(&clamped.to_le_bytes());
                        }
                    }

                    if let Some(mixer_arc) = &loopback_mixer {
                        if let Ok(mut mixer) = mixer_arc.lock() {
                            let processed =
                                mixer.push_audio(&scanner_id_clone, &f32_batch, hw_rate);
                            if let Some(rt_prod) = &mut rt_loopback_producer {
                                for &s in &processed {
                                    let _ = rt_prod.push(s);
                                }
                            }
                        }
                    }

                    if let Ok(mut guard) = worker_state.lock() {
                        if guard.recording_enabled {
                            guard.pre_buffer.write(&i16_batch);
                            if let Some(session) = &mut guard.active_session {
                                session.append(&pcm_bytes);
                            }
                        }
                    }
                }
            });

        if let Ok(h) = handle {
            self.worker_handle = Some(h);
            tracing::info!(
                scanner_id = %self.scanner_id,
                sample_rate = hw_rate,
                channel_mode = ?self.channel_mode,
                "Audio capture stream started"
            );
        } else {
            self.running.store(false, Ordering::SeqCst);
        }
    }

    /// Stops the hardware stream and joins the worker thread cleanly.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_handle.take() {
            // Worker exits within 5ms once running is false
            let _ = handle.join();
        }
        self.live_level_pct.store(0, Ordering::Relaxed);
    }

    /// Handles squelch open for this scanner (`audio/recorder.py:551-583`).
    pub fn on_squelch_open(&self, status_snapshot: &ScannerStatus) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if !guard.recording_enabled {
            return;
        }

        if let Some(mut session) = guard.pending_release.take() {
            if should_update_session_status(&session.status, status_snapshot) {
                session.status = status_snapshot.clone();
            }
            guard.active_session = Some(session);
            return;
        }

        if let Some(active) = &mut guard.active_session {
            if should_update_session_status(&active.status, status_snapshot) {
                active.status = status_snapshot.clone();
            }
        } else {
            let pre_roll = guard.pre_buffer.get_preroll_bytes();
            let session =
                ActiveCallSession::new(self.scanner_id.clone(), status_snapshot.clone(), pre_roll);
            guard.active_session = Some(session);
            tracing::info!(
                scanner_id = %self.scanner_id,
                channel = %status_snapshot.channel_name,
                tgid = ?status_snapshot.tgid,
                freq = ?status_snapshot.frequency,
                "Squelch Open -> Started recording"
            );
        }
    }

    /// Transitions an active call session into its hang-time window (`audio/recorder.py:584-590`).
    pub fn on_squelch_close(&self, _status_snapshot: &ScannerStatus) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if let Some(mut session) = guard.active_session.take() {
            session.last_active_time = now_unix_secs();
            guard.pending_release = Some(session);
        }
    }

    /// Immediately cancels and discards any ongoing or pending recording session due to mutual exclusion
    /// (`audio/recorder.py:591-602`).
    pub fn abort_session(&self) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if guard.active_session.is_some() || guard.pending_release.is_some() {
            tracing::info!(
                scanner_id = %self.scanner_id,
                "Recording session aborted due to mutual exclusion"
            );
        }
        guard.active_session = None;
        guard.pending_release = None;
    }

    /// Checks whether a pending session's hang time has elapsed and dispatches it if complete
    /// (`audio/recorder.py:603-635`).
    pub fn check_hang_time(&self) {
        let to_dispatch = {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            if let Some(pending) = &guard.pending_release {
                if now_unix_secs() - pending.last_active_time >= self.hang_time_seconds {
                    guard.pending_release.take()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(session) = to_dispatch {
            self.finalize_and_dispatch(session);
        }
    }

    fn finalize_and_dispatch(&self, session: ActiveCallSession) {
        let pcm = session.get_full_pcm();
        let dur = session.duration();

        if dur < self.min_duration_seconds || pcm.len() < 1000 {
            tracing::debug!(
                scanner_id = %self.scanner_id,
                duration = dur,
                bytes = pcm.len(),
                "Discarded brief transmission"
            );
            return;
        }

        tracing::info!(
            scanner_id = %self.scanner_id,
            duration = dur,
            bytes = pcm.len(),
            sample_rate = self.sample_rate,
            "Transmission complete; dispatching to ScanScribe"
        );

        if let Some(cb) = &self.dispatch_callback {
            cb(CompletedCall {
                scanner_id: session.scanner_id,
                status: session.status,
                pcm_bytes: pcm,
                duration_seconds: dur,
                sample_rate: self.sample_rate,
            });
        }
    }
}

impl Drop for SingleScannerAudioStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Multi-scanner audio recorder and real-time playback loopback monitor manager.
///
/// Ported from `ScannerAudioRecorder` (`audio/recorder.py:638-757`).
pub struct ScannerAudioRecorder {
    pub sample_rate: u32,
    pub hang_time_seconds: f64,
    pub min_duration_seconds: f64,
    dispatch_callback: Option<DispatchCallback>,
    pub streams: HashMap<String, SingleScannerAudioStream>,
    pub loopback_mixer: Arc<Mutex<LoopbackMixer>>,
    output_device_index: Option<u32>,
    output_device_name: Option<String>,
    output_running: Arc<AtomicBool>,
    output_handle: Option<JoinHandle<()>>,
    rt_loopback_shared: Arc<RtLoopbackShared>,
    pub recording_enabled: bool,
    running: bool,
}

impl ScannerAudioRecorder {
    /// Creates a new `ScannerAudioRecorder`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sample_rate: u32,
        hang_time_seconds: f64,
        min_duration_seconds: f64,
        dispatch_callback: Option<DispatchCallback>,
        loopback_output_device: Option<u32>,
        loopback_output_name: Option<String>,
        loopback_enabled: bool,
        loopback_volume: f32,
        loopback_pan_mode: &str,
    ) -> Self {
        let pan = PanMode::from_str_name(loopback_pan_mode);
        let sr = sample_rate.max(8000);
        Self {
            sample_rate: sr,
            hang_time_seconds,
            min_duration_seconds,
            dispatch_callback,
            streams: HashMap::new(),
            loopback_mixer: Arc::new(Mutex::new(LoopbackMixer::new(
                loopback_enabled,
                loopback_volume,
                pan,
                44_100,
            ))),
            output_device_index: loopback_output_device,
            output_device_name: loopback_output_name,
            output_running: Arc::new(AtomicBool::new(false)),
            output_handle: None,
            rt_loopback_shared: RtLoopbackShared::new(pan, 2048),
            recording_enabled: true,
            running: false,
        }
    }

    /// Configures or updates a scanner's capture stream (`scanner_a` or `scanner_b`).
    pub fn configure_scanner(
        &mut self,
        scanner_id: &str,
        device_index: Option<u32>,
        device_name: Option<String>,
        channel_mode: AudioChannelMode,
        enabled: bool,
    ) {
        if let Some(mut old) = self.streams.remove(scanner_id) {
            old.stop();
        }
        let stream = SingleScannerAudioStream::new(
            scanner_id.to_string(),
            device_index,
            device_name,
            channel_mode,
            self.sample_rate,
            self.hang_time_seconds,
            self.min_duration_seconds,
            enabled,
            self.dispatch_callback.clone(),
        );
        stream.set_recording_enabled(self.recording_enabled);
        self.streams.insert(scanner_id.to_string(), stream);
    }

    /// Updates loopback monitor configuration.
    pub fn configure_loopback(
        &mut self,
        output_device_index: Option<u32>,
        output_device_name: Option<String>,
        enabled: bool,
        volume: f32,
        pan_mode: &str,
    ) {
        let pan = PanMode::from_str_name(pan_mode);
        self.output_device_index = output_device_index;
        self.output_device_name = output_device_name;
        self.rt_loopback_shared
            .pan_mode
            .store(pan as u8, Ordering::Relaxed);
        if let Ok(mut mixer) = self.loopback_mixer.lock() {
            mixer.enabled = enabled;
            mixer.volume = volume.clamp(0.0, 1.0);
            mixer.pan_mode = pan;
        }
    }

    /// Mutes or unmutes a scanner in the loopback monitor with a 15 ms de-click gain ramp.
    pub fn mute_scanner(&self, scanner_id: &str, mute: bool) {
        if let Ok(mut mixer) = self.loopback_mixer.lock() {
            mixer.mute_scanner(scanner_id, mute);
        }
    }

    /// Enables or disables call recording across all scanner streams.
    pub fn set_recording_enabled(&mut self, enabled: bool) {
        self.recording_enabled = enabled;
        for stream in self.streams.values() {
            stream.set_recording_enabled(enabled);
        }
    }

    /// Starts hardware loopback output and per-scanner capture streams.
    pub fn start(&mut self) {
        if self.running {
            return;
        }
        self.running = true;

        let (prod_a, mut cons_a) = rtrb::RingBuffer::<f32>::new(96_000);
        let (prod_b, mut cons_b) = rtrb::RingBuffer::<f32>::new(96_000);

        // Start loopback output stream if enabled and an output device is present
        let loopback_enabled = self.loopback_mixer.lock().is_ok_and(|m| m.enabled);
        if loopback_enabled {
            if let Some(out_dev) =
                resolve_output_device(self.output_device_name.as_deref(), self.output_device_index)
            {
                if let Ok(def_out) = out_dev.default_output_config() {
                    let out_rate = def_out.sample_rate().0;
                    let out_ch = def_out.channels().clamp(1, 2);
                    if let Ok(mut mixer) = self.loopback_mixer.lock() {
                        mixer.samplerate = out_rate;
                    }
                    let shared = Arc::clone(&self.rt_loopback_shared);
                    let out_name = self.output_device_name.clone();
                    let out_idx = self.output_device_index;
                    let out_run = Arc::clone(&self.output_running);
                    out_run.store(true, Ordering::SeqCst);

                    if let Ok(h) = thread::Builder::new()
                        .name("sc-audio-loopback".to_string())
                        .spawn(move || {
                            let Some(thread_out_dev) =
                                resolve_output_device(out_name.as_deref(), out_idx)
                            else {
                                out_run.store(false, Ordering::SeqCst);
                                return;
                            };
                            let cfg = StreamConfig {
                                channels: out_ch,
                                sample_rate: cpal::SampleRate(out_rate),
                                buffer_size: cpal::BufferSize::Default,
                            };
                            let ch_usize = usize::from(out_ch);
                            if let Ok(stream) = thread_out_dev.build_output_stream(
                                &cfg,
                                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                    fill_rt_output_callback(
                                        data,
                                        ch_usize,
                                        &mut cons_a,
                                        &mut cons_b,
                                        &shared,
                                    );
                                },
                                move |_err| {},
                                None,
                            ) {
                                if stream.play().is_ok() {
                                    let _stream_guard = stream;
                                    while out_run.load(Ordering::Relaxed) {
                                        thread::sleep(Duration::from_millis(10));
                                    }
                                }
                            }
                            out_run.store(false, Ordering::SeqCst);
                        })
                    {
                        self.output_handle = Some(h);
                    } else {
                        self.output_running.store(false, Ordering::SeqCst);
                    }
                }
            }
        }

        let mut opt_prod_a = Some(prod_a);
        let mut opt_prod_b = Some(prod_b);
        for (id, stream) in &mut self.streams {
            let prod = if id == "scanner_a" {
                opt_prod_a.take()
            } else {
                opt_prod_b.take()
            };
            stream.start(Some(Arc::clone(&self.loopback_mixer)), prod);
        }
    }

    /// Stops all capture and playback streams cleanly.
    pub fn stop(&mut self) {
        self.running = false;
        self.output_running.store(false, Ordering::SeqCst);
        if let Some(h) = self.output_handle.take() {
            let _ = h.join();
        }
        for stream in self.streams.values_mut() {
            stream.stop();
        }
    }

    /// Notifies `scanner_id` that squelch opened with `status_snapshot`.
    pub fn on_squelch_open(&self, scanner_id: &str, status_snapshot: &ScannerStatus) {
        if let Some(stream) = self.streams.get(scanner_id) {
            stream.on_squelch_open(status_snapshot);
        }
    }

    /// Notifies `scanner_id` that squelch closed with `status_snapshot`.
    pub fn on_squelch_close(&self, scanner_id: &str, status_snapshot: &ScannerStatus) {
        if let Some(stream) = self.streams.get(scanner_id) {
            stream.on_squelch_close(status_snapshot);
        }
    }

    /// Aborts any active or pending recording session on `scanner_id` due to mutual exclusion.
    pub fn abort_session(&self, scanner_id: &str) {
        if let Some(stream) = self.streams.get(scanner_id) {
            stream.abort_session();
        }
    }

    /// Checks hang-time expirations across all configured scanner streams.
    pub fn check_hang_time_expirations(&self) {
        for stream in self.streams.values() {
            stream.check_hang_time();
        }
    }

    /// Returns a map of `scanner_id -> live_level_pct` (`0..=100`).
    #[must_use]
    pub fn get_levels(&self) -> HashMap<String, u8> {
        self.streams
            .iter()
            .map(|(id, s)| (id.clone(), s.live_level_pct()))
            .collect()
    }

    /// Returns the maximum live VU level (`0..=100`) across all streams.
    #[must_use]
    pub fn live_level_pct(&self) -> u8 {
        self.streams
            .values()
            .map(SingleScannerAudioStream::live_level_pct)
            .max()
            .unwrap_or(0)
    }
}

impl Drop for ScannerAudioRecorder {
    fn drop(&mut self) {
        self.stop();
    }
}
