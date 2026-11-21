//! Dual-scanner mutual exclusion engine (`SyncManager`), independent scanner polling,
//! and bounded async `ScanScribe` dispatch worker pool.
//!
//! Ported from [`engine/sync_manager.py`](../../../../engine/sync_manager.py) and
//! [`Plan/phase_4_engine_and_feeder.md`](../../../Plan/phase_4_engine_and_feeder.md).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
pub use sc_audio::resample_pcm_int16;
use sc_audio::{CompletedCall, DispatchCallback, ScannerAudioRecorder};
use sc_core::{
    AppConfig, ExclusionMode, PriorityMode, ScannerConfig, ScannerModel, ScannerStatus,
    ALLOWED_SAMPLE_RATES,
};
use sc_drivers::{
    create_hardware_driver, driver_family_for_model, poll_slot_with_autodetect, AnyScannerDriver,
    HardwareTransport, Transport,
};
use sc_feeder::{CallTransmissionPayload, DispatchResult, ScanScribeFeeder};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// Returns current UNIX epoch time in seconds.
#[must_use]
pub fn now_unix_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Formats a UNIX epoch timestamp (`f64`) as local `"%H:%M:%S"`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn format_hms_local(timestamp: f64) -> String {
    let secs = timestamp.floor() as i64;
    if let chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) =
        Local.timestamp_opt(secs, 0)
    {
        dt.format("%H:%M:%S").to_string()
    } else {
        "00:00:00".to_string()
    }
}

/// Record of a mutual-exclusion skip event.
///
/// Matches `ExclusionEvent.to_dict()` in `engine/sync_manager.py:14-36`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExclusionEvent {
    pub timestamp: f64,
    pub time_str: String,
    pub primary_id: String,
    pub target_id: String,
    pub tgid: Option<String>,
    pub freq: Option<f64>,
    pub channel: String,
    pub reason: String,
    pub message: String,
}

impl ExclusionEvent {
    /// Creates a new `ExclusionEvent` stamped with `timestamp`.
    #[must_use]
    pub fn with_timestamp(
        timestamp: f64,
        primary_id: impl Into<String>,
        target_id: impl Into<String>,
        tgid: Option<String>,
        freq: Option<f64>,
        channel: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let primary_id = primary_id.into();
        let target_id = target_id.into();
        let channel = channel.into();
        let reason = reason.into();
        let time_str = format_hms_local(timestamp);
        let tgid_or_freq = tgid.as_deref().filter(|s| !s.is_empty()).map_or_else(
            || freq.map_or_else(|| "None".to_string(), |f| f.to_string()),
            ToString::to_string,
        );
        let message = format!(
            "[{time_str}] EXCLUDED: {} skipped TGID {tgid_or_freq} ({channel}) because {} is active.",
            target_id.to_ascii_uppercase(),
            primary_id.to_ascii_uppercase()
        );
        Self {
            timestamp,
            time_str,
            primary_id,
            target_id,
            tgid,
            freq,
            channel,
            reason,
            message,
        }
    }
}

/// Active hold entry in `active_exclusions` (`engine/sync_manager.py:68-69, 386-390`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveExclusionHold {
    pub holder: String,
    pub release_at: f64,
    pub channel: String,
    #[serde(skip)]
    pub created_at: f64,
}

/// Full engine state snapshot broadcast over `/api/state` and `/ws` (`engine/sync_manager.py:410-419`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineStateSnapshot {
    pub scanner_a: ScannerStatus,
    pub scanner_b: ScannerStatus,
    pub config: AppConfig,
    pub active_exclusions: HashMap<String, ActiveExclusionHold>,
    pub event_log: Vec<ExclusionEvent>,
    pub audio_levels: HashMap<String, u8>,
    pub audio_level: u8,
}

/// Pure function checking whether `status1` and `status2` collide under `mode`
/// (`engine/sync_manager.py:223-237`).
#[must_use]
pub fn should_exclude(
    mode: ExclusionMode,
    status1: &ScannerStatus,
    status2: &ScannerStatus,
) -> bool {
    if matches!(mode, ExclusionMode::TgidAndFreq | ExclusionMode::TgidOnly) {
        if let (Some(t1), Some(t2)) = (
            status1.tgid.as_deref().filter(|s| !s.is_empty()),
            status2.tgid.as_deref().filter(|s| !s.is_empty()),
        ) {
            if t1 == t2 {
                return true;
            }
        }
    }

    if matches!(mode, ExclusionMode::TgidAndFreq | ExclusionMode::FreqOnly) {
        if let (Some(f1), Some(f2)) = (status1.frequency, status2.frequency) {
            if (f1 - f2).abs() < 0.001 {
                return true;
            }
        }
    }

    false
}

fn exclusion_key(status: &ScannerStatus) -> Option<String> {
    if let Some(tgid) = status.tgid.as_deref().filter(|s| !s.is_empty()) {
        return Some(tgid.to_string());
    }
    status.frequency.map(|f| format!("{f:.4}"))
}

/// Hardware or recording actions produced by a single pure mutual-exclusion evaluation tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncCycleActions {
    /// Scanners (`"scanner_a"` / `"scanner_b"`) whose in-flight recording session must be aborted immediately.
    pub abort_sessions: Vec<String>,
    /// Scanners (`"scanner_a"` / `"scanner_b"`) that must be sent a hardware `send_skip()` command.
    pub send_skips: Vec<String>,
    /// Scanners that should have `on_squelch_open` triggered on the audio recorder.
    pub squelch_opens: Vec<String>,
    /// Scanners that should have `on_squelch_close` triggered on the audio recorder.
    pub squelch_closes: Vec<String>,
    /// Loopback mute state (`(scanner_id, mute)`).
    pub loopback_mutes: Vec<(String, bool)>,
}

/// Pure, deterministic mutual-exclusion state machine (`engine/sync_manager.py:253-399`).
#[derive(Debug, Clone)]
pub struct ExclusionStateMachine {
    pub active_exclusions: HashMap<String, ActiveExclusionHold>,
    pub event_log: Vec<ExclusionEvent>,
    prev_receiving_a: bool,
    prev_receiving_b: bool,
    last_skip_time: HashMap<String, f64>,
}

impl Default for ExclusionStateMachine {
    fn default() -> Self {
        let mut last_skip_time = HashMap::new();
        last_skip_time.insert("scanner_a".to_string(), 0.0);
        last_skip_time.insert("scanner_b".to_string(), 0.0);
        Self {
            active_exclusions: HashMap::new(),
            event_log: Vec::new(),
            prev_receiving_a: false,
            prev_receiving_b: false,
            last_skip_time,
        }
    }
}

impl ExclusionStateMachine {
    /// Creates a new `ExclusionStateMachine`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an [`ExclusionEvent`] and caps `event_log` at 100 entries (`engine/sync_manager.py:217-220`).
    pub fn log_event(&mut self, event: ExclusionEvent) {
        tracing::warn!(message = %event.message, "Mutual exclusion skip triggered");
        self.event_log.push(event);
        if self.event_log.len() > 100 {
            self.event_log.remove(0);
        }
    }

    /// Evaluates one poll tick given the latest `status_a` and `status_b` (which may come from
    /// independent scanner poll tasks) and returns all side-effect actions to execute.
    #[allow(clippy::too_many_lines)]
    pub fn step(
        &mut self,
        config: &AppConfig,
        status_a: &ScannerStatus,
        status_b: &ScannerStatus,
        now: f64,
    ) -> SyncCycleActions {
        let mut actions = SyncCycleActions::default();
        let mut currently_excluded: HashSet<String> = HashSet::new();

        // 1. Cleanup expired active exclusions (and enforce auto_release_seconds ceiling)
        let auto_release = config.sync.auto_release_seconds.max(1.0);
        self.active_exclusions.retain(|k, hold| {
            let expired = now > hold.release_at || (now - hold.created_at) > auto_release;
            if expired {
                tracing::info!(key = %k, "Exclusion expired");
            }
            !expired
        });

        // 2. Process Mutual Exclusion Logic if Sync Enabled
        if config.sync.enable_sync {
            let p_mode = config.sync.priority_mode;
            let p_mode_str = match p_mode {
                PriorityMode::EqualPeers => "EQUAL_PEERS",
                PriorityMode::PrimarySecondary => "PRIMARY_SECONDARY",
                PriorityMode::DepartmentPriority => "DEPARTMENT_PRIORITY",
            };

            if status_a.receiving && status_b.receiving {
                // Case A: Both scanners landed on the same active call simultaneously
                if should_exclude(config.sync.exclusion_mode, status_a, status_b) {
                    let (primary_holder, target_id, victim_status) = if p_mode
                        == PriorityMode::PrimarySecondary
                        && config.sync.primary_scanner_id == "scanner_b"
                    {
                        ("scanner_b", "scanner_a", status_a)
                    } else {
                        ("scanner_a", "scanner_b", status_b)
                    };

                    currently_excluded.insert(target_id.to_string());
                    actions.abort_sessions.push(target_id.to_string());

                    let last_skip = self.last_skip_time.get(target_id).copied().unwrap_or(0.0);
                    if now - last_skip >= 1.0 {
                        self.last_skip_time.insert(target_id.to_string(), now);
                        actions.send_skips.push(target_id.to_string());
                        self.log_event(ExclusionEvent::with_timestamp(
                            now,
                            primary_holder,
                            target_id,
                            victim_status.tgid.clone(),
                            victim_status.frequency,
                            victim_status.channel_name.clone(),
                            format!("Mutual exclusion conflict under {p_mode_str}"),
                        ));
                    }
                }
            } else if status_b.receiving {
                // Case B: Scanner B landed on a call held in active_exclusions by Scanner A
                if let Some(key) = exclusion_key(status_b) {
                    if self
                        .active_exclusions
                        .get(&key)
                        .is_some_and(|h| h.holder == "scanner_a")
                    {
                        let target_id = "scanner_b";
                        currently_excluded.insert(target_id.to_string());
                        actions.abort_sessions.push(target_id.to_string());

                        let last_skip = self.last_skip_time.get(target_id).copied().unwrap_or(0.0);
                        if now - last_skip >= 1.0 {
                            self.last_skip_time.insert(target_id.to_string(), now);
                            actions.send_skips.push(target_id.to_string());
                            self.log_event(ExclusionEvent::with_timestamp(
                                now,
                                "scanner_a",
                                target_id,
                                status_b.tgid.clone(),
                                status_b.frequency,
                                status_b.channel_name.clone(),
                                "TGID active in Scanner A exclusion registry",
                            ));
                        }
                    }
                }
            } else if status_a.receiving {
                // Case C: Scanner A landed on a call held in active_exclusions by Scanner B
                if let Some(key) = exclusion_key(status_a) {
                    if self
                        .active_exclusions
                        .get(&key)
                        .is_some_and(|h| h.holder == "scanner_b")
                    {
                        if p_mode == PriorityMode::PrimarySecondary
                            && config.sync.primary_scanner_id == "scanner_a"
                        {
                            let target_id = "scanner_b";
                            currently_excluded.insert(target_id.to_string());
                            actions.abort_sessions.push(target_id.to_string());

                            let last_skip =
                                self.last_skip_time.get(target_id).copied().unwrap_or(0.0);
                            if now - last_skip >= 1.0 {
                                self.last_skip_time.insert(target_id.to_string(), now);
                                actions.send_skips.push(target_id.to_string());
                                self.log_event(ExclusionEvent::with_timestamp(
                                    now,
                                    "scanner_a",
                                    target_id,
                                    status_a.tgid.clone(),
                                    status_a.frequency,
                                    status_a.channel_name.clone(),
                                    "Scanner A (Primary) pre-empted Scanner B",
                                ));
                            }
                        } else {
                            let target_id = "scanner_a";
                            currently_excluded.insert(target_id.to_string());
                            actions.abort_sessions.push(target_id.to_string());

                            let last_skip =
                                self.last_skip_time.get(target_id).copied().unwrap_or(0.0);
                            if now - last_skip >= 1.0 {
                                self.last_skip_time.insert(target_id.to_string(), now);
                                actions.send_skips.push(target_id.to_string());
                                self.log_event(ExclusionEvent::with_timestamp(
                                    now,
                                    "scanner_b",
                                    target_id,
                                    status_a.tgid.clone(),
                                    status_a.frequency,
                                    status_a.channel_name.clone(),
                                    "TGID active in Scanner B exclusion registry",
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 3. Audio recording state machine transitions (suppressed if excluded)
        let a_excluded = currently_excluded.contains("scanner_a");
        if status_a.receiving && !a_excluded {
            actions.squelch_opens.push("scanner_a".to_string());
        } else if !status_a.receiving && self.prev_receiving_a {
            actions.squelch_closes.push("scanner_a".to_string());
        }
        self.prev_receiving_a = status_a.receiving && !a_excluded;

        let b_excluded = currently_excluded.contains("scanner_b");
        if status_b.receiving && !b_excluded {
            actions.squelch_opens.push("scanner_b".to_string());
        } else if !status_b.receiving && self.prev_receiving_b {
            actions.squelch_closes.push("scanner_b".to_string());
        }
        self.prev_receiving_b = status_b.receiving && !b_excluded;

        // 4. Loopback mutes
        actions
            .loopback_mutes
            .push(("scanner_a".to_string(), a_excluded || status_a.avoided));
        actions
            .loopback_mutes
            .push(("scanner_b".to_string(), b_excluded || status_b.avoided));

        // 5. Update exclusion holds for active non-excluded receiving scanners
        if status_a.receiving && !a_excluded {
            if let Some(key) = exclusion_key(status_a) {
                let created_at = self
                    .active_exclusions
                    .get(&key)
                    .filter(|h| h.holder == "scanner_a")
                    .map_or(now, |h| h.created_at);
                self.active_exclusions.insert(
                    key,
                    ActiveExclusionHold {
                        holder: "scanner_a".to_string(),
                        release_at: now + config.sync.hang_time_seconds,
                        channel: status_a.channel_name.clone(),
                        created_at,
                    },
                );
            }
        }

        if status_b.receiving && !b_excluded {
            if let Some(key) = exclusion_key(status_b) {
                let created_at = self
                    .active_exclusions
                    .get(&key)
                    .filter(|h| h.holder == "scanner_b")
                    .map_or(now, |h| h.created_at);
                self.active_exclusions.insert(
                    key,
                    ActiveExclusionHold {
                        holder: "scanner_b".to_string(),
                        release_at: now + config.sync.hang_time_seconds,
                        channel: status_b.channel_name.clone(),
                        created_at,
                    },
                );
            }
        }

        actions
    }
}

/// Resamples a [`CompletedCall`] from its capture sample rate to `feeder.sample_rate`
/// and constructs the [`CallTransmissionPayload`] (`engine/sync_manager.py:143-175`).
#[must_use]
pub fn build_dispatch_payload(
    config: &AppConfig,
    call: &CompletedCall,
    now_ts: f64,
) -> CallTransmissionPayload {
    let in_rate = if call.sample_rate > 0 {
        call.sample_rate
    } else {
        44_100
    };
    let mut target_rate = if config.feeder.sample_rate > 0 {
        config.feeder.sample_rate
    } else {
        16_000
    };
    if !ALLOWED_SAMPLE_RATES.contains(&target_rate) {
        tracing::warn!(
            requested_rate = target_rate,
            "feeder.sample_rate not in ALLOWED_SAMPLE_RATES; falling back to 16000 Hz"
        );
        target_rate = 16_000;
    }

    let dispatch_pcm = resample_pcm_int16(&call.pcm_bytes, in_rate, target_rate)
        .unwrap_or_else(|_| call.pcm_bytes.clone());

    let sc_cfg = if call.scanner_id == "scanner_a" {
        &config.scanner_a
    } else {
        &config.scanner_b
    };

    CallTransmissionPayload {
        scanner_id: call.scanner_id.clone(),
        scanner_model: call.status.model.clone(),
        system_name: call.status.system_name.clone(),
        department_name: call.status.dept_name.clone(),
        channel_name: call.status.channel_name.clone(),
        tgid: call.status.tgid.clone(),
        frequency: call.status.frequency,
        modulation: call.status.modulation.clone(),
        rssi: call.status.rssi,
        timestamp: now_ts,
        duration_seconds: call.duration_seconds,
        audio_pcm: Some(dispatch_pcm),
        filename_template: sc_cfg.filename_template.clone(),
        tit2_template: sc_cfg.tit2_template.clone(),
    }
}

type DriverFactory<T> = Arc<dyn Fn(ScannerConfig) -> AnyScannerDriver<T> + Send + Sync>;

fn mark_status_disabled(status: &mut ScannerStatus) {
    status.connected = false;
    status.receiving = false;
    status.avoided = false;
    status.rssi = 0;
    status.tgid = None;
    status.frequency = None;
    status.unit_id = None;
    status.tone = None;
    status.system_name.clear();
    status.dept_name.clear();
    status.channel_name.clear();
    status.display_lines.clear();
    status.status_line = "DISABLED".to_string();
}

fn apply_detected_model(
    cfg_arc: &Arc<Mutex<AppConfig>>,
    feeder_arc: &Arc<Mutex<ScanScribeFeeder>>,
    scanner_id: &str,
    detected: ScannerModel,
) {
    let updated_map = if let Ok(mut cfg) = cfg_arc.lock() {
        if scanner_id == "scanner_a" {
            cfg.scanner_a.model = detected;
        } else {
            cfg.scanner_b.model = detected;
        }
        let mut map = HashMap::new();
        map.insert("scanner_a".to_string(), cfg.scanner_a.clone());
        map.insert("scanner_b".to_string(), cfg.scanner_b.clone());
        Some(map)
    } else {
        None
    };

    if let Some(map) = updated_map {
        if let Ok(mut fdr) = feeder_arc.lock() {
            fdr.update_scanner_configs(map);
        }
    }
}

fn scanner_driver_settings_changed(old: &ScannerConfig, new: &ScannerConfig) -> bool {
    old.enabled != new.enabled
        || old.connection_mode != new.connection_mode
        || old.port != new.port
        || old.baud_rate != new.baud_rate
        || old.ip_host != new.ip_host
        || old.ip_port != new.ip_port
        || driver_family_for_model(old.model) != driver_family_for_model(new.model)
}

/// Dual-scanner synchronization and recording engine (`engine/sync_manager.py:58-419`).
///
/// Supports both generic transports (such as [`sc_drivers::ReplayTransport`] for deterministic
/// two-scanner simulations and fault-injection tests) and hardware serial/LAN transports.
pub struct SyncManager<T: Transport = HardwareTransport> {
    config: Arc<Mutex<AppConfig>>,
    pub scanner_a: Arc<Mutex<Option<AnyScannerDriver<T>>>>,
    pub scanner_b: Arc<Mutex<Option<AnyScannerDriver<T>>>>,
    driver_builder: Option<DriverFactory<T>>,
    status_a: Arc<Mutex<ScannerStatus>>,
    status_b: Arc<Mutex<ScannerStatus>>,
    state_machine: Arc<Mutex<ExclusionStateMachine>>,
    pub feeder: Arc<Mutex<ScanScribeFeeder>>,
    pub audio_recorder: Arc<Mutex<ScannerAudioRecorder>>,
    dispatch_tx: mpsc::Sender<CompletedCall>,
    dispatch_rx: Option<mpsc::Receiver<CompletedCall>>,
    state_tx: watch::Sender<EngineStateSnapshot>,
    state_rx: watch::Receiver<EngineStateSnapshot>,
    cancel_token: CancellationToken,
    tasks: JoinSet<()>,
}

impl SyncManager<HardwareTransport> {
    /// Creates a new hardware-backed `SyncManager` from `config`.
    #[must_use]
    pub fn new_hardware(config: AppConfig) -> Self {
        let drv_a = create_hardware_driver(config.scanner_a.clone());
        let drv_b = create_hardware_driver(config.scanner_b.clone());
        let mut mgr = Self::with_drivers(config, Some(drv_a), Some(drv_b));
        mgr.driver_builder = Some(Arc::new(create_hardware_driver));
        mgr
    }
}

impl<T: Transport + 'static> SyncManager<T> {
    /// Creates a new `SyncManager` with explicit driver instances (e.g., `ReplayTransport` in tests).
    #[must_use]
    pub fn with_drivers(
        config: AppConfig,
        driver_a: Option<AnyScannerDriver<T>>,
        driver_b: Option<AnyScannerDriver<T>>,
    ) -> Self {
        let mut initial_status_a = driver_a.as_ref().map_or_else(
            || {
                ScannerStatus::new(
                    "scanner_a",
                    &config.scanner_a.name,
                    config.scanner_a.model.as_str(),
                )
            },
            |d| d.status().clone(),
        );
        if !config.scanner_a.enabled {
            mark_status_disabled(&mut initial_status_a);
        }

        let mut initial_status_b = driver_b.as_ref().map_or_else(
            || {
                ScannerStatus::new(
                    "scanner_b",
                    &config.scanner_b.name,
                    config.scanner_b.model.as_str(),
                )
            },
            |d| d.status().clone(),
        );
        if !config.scanner_b.enabled {
            mark_status_disabled(&mut initial_status_b);
        }

        let mut sc_configs = HashMap::new();
        sc_configs.insert("scanner_a".to_string(), config.scanner_a.clone());
        sc_configs.insert("scanner_b".to_string(), config.scanner_b.clone());
        let feeder = Arc::new(Mutex::new(ScanScribeFeeder::new(
            config.feeder.clone(),
            sc_configs,
        )));

        let (dispatch_tx, dispatch_rx) = mpsc::channel::<CompletedCall>(64);
        let tx_clone = dispatch_tx.clone();
        let feeder_for_cb = Arc::clone(&feeder);
        let cfg_arc = Arc::new(Mutex::new(config.clone()));
        let cfg_for_cb = Arc::clone(&cfg_arc);

        let dispatch_cb: DispatchCallback = Arc::new(move |call: CompletedCall| {
            // Non-blocking enqueue to bounded dispatch channel; if full or closed, fallback to thread
            if tx_clone.try_send(call.clone()).is_err() {
                let feeder_inner = Arc::clone(&feeder_for_cb);
                let cfg_inner = Arc::clone(&cfg_for_cb);
                std::thread::spawn(move || {
                    if let (Ok(cfg_guard), Ok(fdr_guard)) = (cfg_inner.lock(), feeder_inner.lock())
                    {
                        let payload = build_dispatch_payload(&cfg_guard, &call, now_unix_secs());
                        let _ = fdr_guard.dispatch_call(payload);
                    }
                });
            }
        });

        #[allow(clippy::cast_possible_truncation, clippy::unnecessary_cast)]
        let mut recorder = ScannerAudioRecorder::new(
            config.feeder.sample_rate,
            config.sync.hang_time_seconds,
            0.4,
            Some(dispatch_cb),
            config.audio_loopback.output_device_index,
            config.audio_loopback.output_device_name.clone(),
            config.audio_loopback.enabled,
            config.audio_loopback.volume as f32,
            &config.audio_loopback.pan_mode,
        );
        Self::apply_audio_stream_config(&mut recorder, &config);

        let initial_snapshot = EngineStateSnapshot {
            scanner_a: initial_status_a.clone(),
            scanner_b: initial_status_b.clone(),
            config,
            active_exclusions: HashMap::new(),
            event_log: Vec::new(),
            audio_levels: recorder.get_levels(),
            audio_level: recorder.live_level_pct(),
        };
        let (state_tx, state_rx) = watch::channel(initial_snapshot);

        Self {
            config: cfg_arc,
            scanner_a: Arc::new(Mutex::new(driver_a)),
            scanner_b: Arc::new(Mutex::new(driver_b)),
            driver_builder: None,
            status_a: Arc::new(Mutex::new(initial_status_a)),
            status_b: Arc::new(Mutex::new(initial_status_b)),
            state_machine: Arc::new(Mutex::new(ExclusionStateMachine::new())),
            feeder,
            audio_recorder: Arc::new(Mutex::new(recorder)),
            dispatch_tx,
            dispatch_rx: Some(dispatch_rx),
            state_tx,
            state_rx,
            cancel_token: CancellationToken::new(),
            tasks: JoinSet::new(),
        }
    }

    fn apply_audio_stream_config(recorder: &mut ScannerAudioRecorder, config: &AppConfig) {
        let enabled_a = config.scanner_a.enabled && config.scanner_a.audio_enabled;
        recorder.configure_scanner(
            "scanner_a",
            config.scanner_a.audio_device_index,
            config.scanner_a.audio_device_name.clone(),
            config.scanner_a.audio_channel,
            enabled_a,
        );
        recorder.mute_scanner("scanner_a", !enabled_a);

        let enabled_b = config.scanner_b.enabled && config.scanner_b.audio_enabled;
        recorder.configure_scanner(
            "scanner_b",
            config.scanner_b.audio_device_index,
            config.scanner_b.audio_device_name.clone(),
            config.scanner_b.audio_channel,
            enabled_b,
        );
        recorder.mute_scanner("scanner_b", !enabled_b);

        #[allow(clippy::cast_possible_truncation, clippy::unnecessary_cast)]
        recorder.configure_loopback(
            config.audio_loopback.output_device_index,
            config.audio_loopback.output_device_name.clone(),
            config.audio_loopback.enabled,
            config.audio_loopback.volume as f32,
            &config.audio_loopback.pan_mode,
        );
        recorder.set_recording_enabled(config.feeder.enable_feeder);
    }

    /// Returns a `watch::Receiver` that receives every [`EngineStateSnapshot`] update.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<EngineStateSnapshot> {
        self.state_rx.clone()
    }

    /// Computes and returns the current [`EngineStateSnapshot`].
    #[must_use]
    pub fn get_full_state(&self) -> EngineStateSnapshot {
        let cfg = self.config.lock().map(|c| c.clone()).unwrap_or_default();
        let sa = self.status_a.lock().map_or_else(
            |_| ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP"),
            |s| s.clone(),
        );
        let sb = self.status_b.lock().map_or_else(
            |_| ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2"),
            |s| s.clone(),
        );
        let (active_exclusions, event_log) = self
            .state_machine
            .lock()
            .map(|sm| {
                let tail_start = sm.event_log.len().saturating_sub(20);
                (
                    sm.active_exclusions.clone(),
                    sm.event_log[tail_start..].to_vec(),
                )
            })
            .unwrap_or_default();
        let (audio_levels, audio_level) = self
            .audio_recorder
            .lock()
            .map(|r| (r.get_levels(), r.live_level_pct()))
            .unwrap_or_default();

        EngineStateSnapshot {
            scanner_a: sa,
            scanner_b: sb,
            config: cfg,
            active_exclusions,
            event_log,
            audio_levels,
            audio_level,
        }
    }

    /// Globally toggles call recording on or off (`engine/sync_manager.py:130-135`).
    pub fn set_recording_enabled(&self, enabled: bool) {
        if let Ok(mut cfg) = self.config.lock() {
            cfg.feeder.enable_feeder = enabled;
        }
        if let Ok(mut fdr) = self.feeder.lock() {
            fdr.config.enable_feeder = enabled;
        }
        if let Ok(mut rec) = self.audio_recorder.lock() {
            rec.set_recording_enabled(enabled);
        }
        tracing::info!(enabled, "Master call audio recording toggled");
    }

    /// Hot-reloads configuration, updating hardware drivers cleanly if ports/enable flags changed
    /// and restarting audio streams only if audio devices, channels, loopback, or enable flags changed.
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
    pub fn update_config(&self, new_config: AppConfig) {
        let (rebuild_primary, rebuild_secondary, audio_restart_needed) =
            self.config.lock().map_or((true, true, true), |old| {
                let da = scanner_driver_settings_changed(&old.scanner_a, &new_config.scanner_a);
                let db = scanner_driver_settings_changed(&old.scanner_b, &new_config.scanner_b);
                let audio = old.scanner_a.audio_device_index
                    != new_config.scanner_a.audio_device_index
                    || old.scanner_a.audio_device_name != new_config.scanner_a.audio_device_name
                    || old.scanner_a.audio_channel != new_config.scanner_a.audio_channel
                    || (old.scanner_a.enabled && old.scanner_a.audio_enabled)
                        != (new_config.scanner_a.enabled && new_config.scanner_a.audio_enabled)
                    || old.scanner_b.audio_device_index != new_config.scanner_b.audio_device_index
                    || old.scanner_b.audio_device_name != new_config.scanner_b.audio_device_name
                    || old.scanner_b.audio_channel != new_config.scanner_b.audio_channel
                    || (old.scanner_b.enabled && old.scanner_b.audio_enabled)
                        != (new_config.scanner_b.enabled && new_config.scanner_b.audio_enabled)
                    || old.audio_loopback.enabled != new_config.audio_loopback.enabled
                    || old.audio_loopback.output_device_index
                        != new_config.audio_loopback.output_device_index
                    || old.audio_loopback.output_device_name
                        != new_config.audio_loopback.output_device_name;
                (da, db, audio)
            });

        if let Ok(mut cfg) = self.config.lock() {
            *cfg = new_config.clone();
        }

        if rebuild_primary {
            if let Ok(mut guard_a) = self.scanner_a.lock() {
                if let Some(old_drv) = guard_a.as_mut() {
                    old_drv.disconnect();
                }
                if let Some(ref build) = self.driver_builder {
                    let fresh = build(new_config.scanner_a.clone());
                    let st = fresh.status().clone();
                    *guard_a = Some(fresh);
                    if let Ok(mut sa) = self.status_a.lock() {
                        *sa = st;
                    }
                }
            }
            if !new_config.scanner_a.enabled {
                if let Ok(mut sa) = self.status_a.lock() {
                    mark_status_disabled(&mut sa);
                }
            }
        }

        if rebuild_secondary {
            if let Ok(mut guard_b) = self.scanner_b.lock() {
                if let Some(old_drv) = guard_b.as_mut() {
                    old_drv.disconnect();
                }
                if let Some(ref build) = self.driver_builder {
                    let fresh = build(new_config.scanner_b.clone());
                    let st = fresh.status().clone();
                    *guard_b = Some(fresh);
                    if let Ok(mut sb) = self.status_b.lock() {
                        *sb = st;
                    }
                }
            }
            if !new_config.scanner_b.enabled {
                if let Ok(mut sb) = self.status_b.lock() {
                    mark_status_disabled(&mut sb);
                }
            }
        }

        if let Ok(mut fdr) = self.feeder.lock() {
            fdr.update_config(new_config.feeder.clone());
            let mut map = HashMap::new();
            map.insert("scanner_a".to_string(), new_config.scanner_a.clone());
            map.insert("scanner_b".to_string(), new_config.scanner_b.clone());
            fdr.update_scanner_configs(map);
        }
        if let Ok(mut rec) = self.audio_recorder.lock() {
            rec.hang_time_seconds = new_config.sync.hang_time_seconds;
            #[allow(clippy::cast_possible_truncation, clippy::unnecessary_cast)]
            rec.configure_loopback(
                new_config.audio_loopback.output_device_index,
                new_config.audio_loopback.output_device_name.clone(),
                new_config.audio_loopback.enabled,
                new_config.audio_loopback.volume as f32,
                &new_config.audio_loopback.pan_mode,
            );
            rec.set_recording_enabled(new_config.feeder.enable_feeder);
            if audio_restart_needed {
                Self::apply_audio_stream_config(&mut rec, &new_config);
                rec.restart_if_running();
            }
        }
        let _ = self.state_tx.send(self.get_full_state());
    }

    /// Synchronously executes a single poll + mutual-exclusion evaluation cycle at `now`.
    ///
    /// **Intentional Deviation from Python (`ACTIVE_CONTEXT.md` §3.4):**
    /// Polls `scanner_a` and `scanner_b` independently so an error or timeout on one scanner
    /// never discards the status or stalls mutual-exclusion/recording for the healthy scanner.
    #[must_use]
    pub fn poll_once_sync(&self, now: f64) -> EngineStateSnapshot {
        let (enabled_a, enabled_b) = self
            .config
            .lock()
            .map_or((true, true), |c| (c.scanner_a.enabled, c.scanner_b.enabled));

        // 1. Poll Scanner A independently if enabled
        if !enabled_a {
            if let Ok(mut sa) = self.status_a.lock() {
                mark_status_disabled(&mut sa);
            }
        } else if let Ok(mut drv_a_guard) = self.scanner_a.lock() {
            if let Some((st_a, detected_a)) = poll_slot_with_autodetect(&mut drv_a_guard) {
                if let Ok(mut sa) = self.status_a.lock() {
                    *sa = st_a;
                }
                if let Some(detected) = detected_a {
                    apply_detected_model(&self.config, &self.feeder, "scanner_a", detected);
                }
            }
        }

        // 2. Poll Scanner B independently if enabled
        if !enabled_b {
            if let Ok(mut sb) = self.status_b.lock() {
                mark_status_disabled(&mut sb);
            }
        } else if let Ok(mut drv_b_guard) = self.scanner_b.lock() {
            if let Some((st_b, detected_b)) = poll_slot_with_autodetect(&mut drv_b_guard) {
                if let Ok(mut sb) = self.status_b.lock() {
                    *sb = st_b;
                }
                if let Some(detected) = detected_b {
                    apply_detected_model(&self.config, &self.feeder, "scanner_b", detected);
                }
            }
        }

        let cfg = self.config.lock().map(|c| c.clone()).unwrap_or_default();
        let sa = self.status_a.lock().map_or_else(
            |_| ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP"),
            |s| s.clone(),
        );
        let sb = self.status_b.lock().map_or_else(
            |_| ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2"),
            |s| s.clone(),
        );

        let actions = self
            .state_machine
            .lock()
            .map(|mut sm| sm.step(&cfg, &sa, &sb, now))
            .unwrap_or_default();

        // 3. Apply recording aborts first (Duplicate-abort before any save/dispatch)
        if let Ok(rec) = self.audio_recorder.lock() {
            for target_id in &actions.abort_sessions {
                rec.abort_session(target_id);
            }
        }

        // 4. Send hardware skips
        for target_id in &actions.send_skips {
            if target_id == "scanner_a" {
                if let Ok(mut g) = self.scanner_a.lock() {
                    if let Some(drv) = g.as_mut() {
                        let _ = drv.send_skip();
                    }
                }
            } else if target_id == "scanner_b" {
                if let Ok(mut g) = self.scanner_b.lock() {
                    if let Some(drv) = g.as_mut() {
                        let _ = drv.send_skip();
                    }
                }
            }
        }

        // 5. Apply squelch open/close, hang-time expirations, and loopback mutes
        if let Ok(rec) = self.audio_recorder.lock() {
            for id in &actions.squelch_opens {
                let st = if id == "scanner_a" { &sa } else { &sb };
                rec.on_squelch_open(id, st);
            }
            for id in &actions.squelch_closes {
                let st = if id == "scanner_a" { &sa } else { &sb };
                rec.on_squelch_close(id, st);
            }
            rec.check_hang_time_expirations();
            for (id, mute) in &actions.loopback_mutes {
                rec.mute_scanner(id, *mute);
            }
        }

        let snapshot = self.get_full_state();
        let _ = self.state_tx.send(snapshot.clone());
        snapshot
    }

    /// Manually triggers a hardware skip on `scanner_id` (`POST /api/skip/{scanner_id}`).
    #[must_use]
    pub fn manual_skip(&self, scanner_id: &str) -> bool {
        if scanner_id == "scanner_a" {
            if let Ok(mut g) = self.scanner_a.lock() {
                if let Some(drv) = g.as_mut() {
                    return drv.send_skip();
                }
            }
        } else if scanner_id == "scanner_b" {
            if let Ok(mut g) = self.scanner_b.lock() {
                if let Some(drv) = g.as_mut() {
                    return drv.send_skip();
                }
            }
        }
        false
    }

    /// Synchronously resamples and dispatches a completed call (exposed for deterministic testing).
    #[must_use]
    pub fn dispatch_completed_call_sync(&self, call: &CompletedCall) -> DispatchResult {
        let cfg = self.config.lock().map(|c| c.clone()).unwrap_or_default();
        let payload = build_dispatch_payload(&cfg, call, now_unix_secs());
        if let Ok(fdr) = self.feeder.lock() {
            fdr.dispatch_call(payload)
        } else {
            DispatchResult {
                status: "error".to_string(),
                mode: None,
                audio_file: None,
                sidecar_file: None,
                message: None,
                warning: None,
                error: Some("Feeder mutex poisoned".to_string()),
                response: None,
            }
        }
    }

    /// Enqueues a completed call onto the async background dispatch worker pool without blocking.
    pub fn submit_completed_call(&self, call: CompletedCall) {
        if self.dispatch_tx.try_send(call.clone()).is_err() {
            let feeder = Arc::clone(&self.feeder);
            let cfg_arc = Arc::clone(&self.config);
            tokio::task::spawn_blocking(move || {
                if let (Ok(cfg), Ok(fdr)) = (cfg_arc.lock(), feeder.lock()) {
                    let payload = build_dispatch_payload(&cfg, &call, now_unix_secs());
                    let _ = fdr.dispatch_call(payload);
                }
            });
        }
    }

    /// Starts the background dispatch worker pool and independent scanner poll tasks,
    /// tracked via `JoinSet` and controlled by `CancellationToken` (`AGENTS.md` §2).
    #[allow(clippy::too_many_lines, clippy::similar_names)]
    pub fn start(&mut self) {
        self.cancel_token = CancellationToken::new();

        if let Ok(mut rec) = self.audio_recorder.lock() {
            rec.start();
        }

        // 1. Spawn bounded dispatch worker task
        if let Some(mut rx) = self.dispatch_rx.take() {
            let token = self.cancel_token.clone();
            let feeder = Arc::clone(&self.feeder);
            let cfg_arc = Arc::clone(&self.config);
            self.tasks.spawn(async move {
                loop {
                    tokio::select! {
                        () = token.cancelled() => {
                            // Drain any remaining completed calls before exit (Prime Directive 1: never lose a call)
                            while let Ok(call) = rx.try_recv() {
                                let fdr_c = Arc::clone(&feeder);
                                let cfg_c = Arc::clone(&cfg_arc);
                                let _ = tokio::task::spawn_blocking(move || {
                                    if let (Ok(cfg), Ok(fdr)) = (cfg_c.lock(), fdr_c.lock()) {
                                        let payload = build_dispatch_payload(&cfg, &call, now_unix_secs());
                                        let _ = fdr.dispatch_call(payload);
                                    }
                                }).await;
                            }
                            break;
                        }
                        Some(call) = rx.recv() => {
                            let fdr_c = Arc::clone(&feeder);
                            let cfg_c = Arc::clone(&cfg_arc);
                            let _ = tokio::task::spawn_blocking(move || {
                                if let (Ok(cfg), Ok(fdr)) = (cfg_c.lock(), fdr_c.lock()) {
                                    let payload = build_dispatch_payload(&cfg, &call, now_unix_secs());
                                    let _ = fdr.dispatch_call(payload);
                                }
                            }).await;
                        }
                    }
                }
            });
        }

        // 2. Spawn independent Scanner A poll task + Scanner B poll task + coordinator loop
        let token = self.cancel_token.clone();
        let cfg_arc = Arc::clone(&self.config);
        let scanner_a = Arc::clone(&self.scanner_a);
        let scanner_b = Arc::clone(&self.scanner_b);
        let status_a = Arc::clone(&self.status_a);
        let status_b = Arc::clone(&self.status_b);
        let sm_arc = Arc::clone(&self.state_machine);
        let rec_arc = Arc::clone(&self.audio_recorder);
        let fdr_arc = Arc::clone(&self.feeder);
        let state_tx = self.state_tx.clone();

        self.tasks.spawn(async move {
            loop {
                let poll_ms = cfg_arc
                    .lock()
                    .map_or(150, |c| {
                        c.scanner_a.poll_interval_ms.min(c.scanner_b.poll_interval_ms)
                    })
                    .max(20);

                tokio::select! {
                    () = token.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_millis(poll_ms)) => {
                        let (enabled_a, enabled_b) = cfg_arc
                            .lock()
                            .map_or((true, true), |c| (c.scanner_a.enabled, c.scanner_b.enabled));

                        // Poll Scanner A and Scanner B on independent blocking threads
                        let drv_a_c = Arc::clone(&scanner_a);
                        let st_a_c = Arc::clone(&status_a);
                        let cfg_a_c = Arc::clone(&cfg_arc);
                        let fdr_a_c = Arc::clone(&fdr_arc);
                        let task_a = tokio::task::spawn_blocking(move || {
                            if !enabled_a {
                                if let Ok(mut out) = st_a_c.lock() {
                                    mark_status_disabled(&mut out);
                                }
                            } else if let Ok(mut g) = drv_a_c.lock() {
                                if let Some((s, detected_opt)) = poll_slot_with_autodetect(&mut g) {
                                    if let Ok(mut out) = st_a_c.lock() {
                                        *out = s;
                                    }
                                    if let Some(detected) = detected_opt {
                                        apply_detected_model(&cfg_a_c, &fdr_a_c, "scanner_a", detected);
                                    }
                                }
                            }
                        });

                        let drv_b_c = Arc::clone(&scanner_b);
                        let st_b_c = Arc::clone(&status_b);
                        let cfg_b_c = Arc::clone(&cfg_arc);
                        let fdr_b_c = Arc::clone(&fdr_arc);
                        let task_b = tokio::task::spawn_blocking(move || {
                            if !enabled_b {
                                if let Ok(mut out) = st_b_c.lock() {
                                    mark_status_disabled(&mut out);
                                }
                            } else if let Ok(mut g) = drv_b_c.lock() {
                                if let Some((s, detected_opt)) = poll_slot_with_autodetect(&mut g) {
                                    if let Ok(mut out) = st_b_c.lock() {
                                        *out = s;
                                    }
                                    if let Some(detected) = detected_opt {
                                        apply_detected_model(&cfg_b_c, &fdr_b_c, "scanner_b", detected);
                                    }
                                }
                            }
                        });

                        let _ = tokio::join!(task_a, task_b);

                        let now = now_unix_secs();
                        let cfg = cfg_arc.lock().map(|c| c.clone()).unwrap_or_default();
                        let sa = status_a.lock().map_or_else(
                            |_| ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP"),
                            |s| s.clone(),
                        );
                        let sb = status_b.lock().map_or_else(
                            |_| ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2"),
                            |s| s.clone(),
                        );

                        let actions = sm_arc.lock().map(|mut sm| sm.step(&cfg, &sa, &sb, now)).unwrap_or_default();

                        if let Ok(rec) = rec_arc.lock() {
                            for target_id in &actions.abort_sessions {
                                rec.abort_session(target_id);
                            }
                        }

                        for target_id in &actions.send_skips {
                            if target_id == "scanner_a" {
                                let drv_a_skip = Arc::clone(&scanner_a);
                                let _ = tokio::task::spawn_blocking(move || {
                                    if let Ok(mut g) = drv_a_skip.lock() {
                                        if let Some(drv) = g.as_mut() {
                                            let _ = drv.send_skip();
                                        }
                                    }
                                }).await;
                            } else if target_id == "scanner_b" {
                                let drv_b_skip = Arc::clone(&scanner_b);
                                let _ = tokio::task::spawn_blocking(move || {
                                    if let Ok(mut g) = drv_b_skip.lock() {
                                        if let Some(drv) = g.as_mut() {
                                            let _ = drv.send_skip();
                                        }
                                    }
                                }).await;
                            }
                        }

                        let (audio_levels, audio_level) = if let Ok(rec) = rec_arc.lock() {
                            for id in &actions.squelch_opens {
                                let st = if id == "scanner_a" { &sa } else { &sb };
                                rec.on_squelch_open(id, st);
                            }
                            for id in &actions.squelch_closes {
                                let st = if id == "scanner_a" { &sa } else { &sb };
                                rec.on_squelch_close(id, st);
                            }
                            rec.check_hang_time_expirations();
                            for (id, mute) in &actions.loopback_mutes {
                                rec.mute_scanner(id, *mute);
                            }
                            (rec.get_levels(), rec.live_level_pct())
                        } else {
                            (HashMap::new(), 0)
                        };

                        let (active_exclusions, event_log) = sm_arc.lock().map(|sm| {
                            let tail = sm.event_log.len().saturating_sub(20);
                            (sm.active_exclusions.clone(), sm.event_log[tail..].to_vec())
                        }).unwrap_or_default();

                        let _ = state_tx.send(EngineStateSnapshot {
                            scanner_a: sa,
                            scanner_b: sb,
                            config: cfg,
                            active_exclusions,
                            event_log,
                            audio_levels,
                            audio_level,
                        });
                    }
                }
            }
        });
    }

    /// Gracefully stops all background tasks, flushes in-flight dispatches, and disconnects drivers.
    pub async fn stop(&mut self) {
        self.cancel_token.cancel();
        while self.tasks.join_next().await.is_some() {}
        if let Ok(mut rec) = self.audio_recorder.lock() {
            rec.stop();
        }
        if let Ok(mut g) = self.scanner_a.lock() {
            if let Some(drv) = g.as_mut() {
                drv.disconnect();
            }
        }
        if let Ok(mut g) = self.scanner_b.lock() {
            if let Some(drv) = g.as_mut() {
                drv.disconnect();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use sc_core::{ExclusionMode, PriorityMode, ScannerModel};
    use sc_drivers::{create_driver_for_model, ReplayTransport};

    fn make_dma_active_transcript(tgid: &str, sys: &str, dept: &str, chan: &str) -> String {
        format!(
            "MDL,BCD996P2\rPWR,220,00851250\rGLG,{tgid},NFM,0,0,{sys},{dept},{chan},1,0,NONE,NONE,4\rSTS,011000,HOLD,{sys}        ,{dept}        ,{chan}        ,S0:----------   ,G:----------    \r"
        )
    }

    fn make_dma_conv_transcript(freq_str: &str, sys: &str, dept: &str, chan: &str) -> String {
        format!(
            "MDL,BCD996P2\rPWR,180,01557975\rGLG,{freq_str},NFM,0,0,{sys},{dept},{chan},1,0,NONE,NONE,3\rSTS,011000,SCAN,{sys}        ,{dept}        ,{chan}        ,S0:----------   ,G:----------    \r"
        )
    }

    #[test]
    fn test_two_scanner_mutual_exclusion_all_modes_and_duplicate_abort() {
        let mut config = AppConfig::default();
        config.scanner_a.model = ScannerModel::BCD996P2;
        config.scanner_b.model = ScannerModel::BCD996P2;
        config.sync.enable_sync = true;
        config.sync.exclusion_mode = ExclusionMode::TgidAndFreq;
        config.sync.priority_mode = PriorityMode::EqualPeers;
        config.sync.hang_time_seconds = 2.0;

        let resp_same_tgid =
            make_dma_active_transcript("10401", "Metro P25", "Fire", "Fire Dispatch");
        let resp_conv_same_freq =
            make_dma_conv_transcript("01557975", "St Francois", "EMS", "EMS 1");

        let transport_a = ReplayTransport::from_responses(vec![
            resp_same_tgid.clone(),
            resp_conv_same_freq.clone(),
            "KEY,SCAN,P,OK".to_string(),
        ]);
        let transport_b = ReplayTransport::from_responses(vec![
            resp_same_tgid.clone(),
            "KEY,SCAN,P,OK".to_string(),
            resp_conv_same_freq.clone(),
        ]);

        let drv_a = create_driver_for_model(config.scanner_a.clone(), transport_a);
        let drv_b = create_driver_for_model(config.scanner_b.clone(), transport_b);

        let engine = SyncManager::with_drivers(config.clone(), Some(drv_a), Some(drv_b));

        // Simulate Scanner B already having an active recording session before collision
        let dummy_status_b = ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2");
        engine
            .audio_recorder
            .lock()
            .unwrap()
            .on_squelch_open("scanner_b", &dummy_status_b);
        assert!(engine.audio_recorder.lock().unwrap().streams["scanner_b"].has_active_session());

        // Tick 1 (t = 100.0): Both land on TGID 10401 -> Scanner B is skipped & its session is duplicate-aborted!
        let snap1 = engine.poll_once_sync(100.0);
        assert!(snap1.scanner_a.receiving);
        assert!(snap1.scanner_b.receiving);
        assert_eq!(snap1.event_log.len(), 1);
        assert_eq!(snap1.event_log[0].primary_id, "scanner_a");
        assert_eq!(snap1.event_log[0].target_id, "scanner_b");
        assert_eq!(snap1.event_log[0].tgid.as_deref(), Some("10401"));

        // Verify duplicate-abort cancelled Scanner B's recording while Scanner A started recording
        assert!(engine.audio_recorder.lock().unwrap().streams["scanner_a"].has_active_session());
        assert!(!engine.audio_recorder.lock().unwrap().streams["scanner_b"].has_active_session());

        // Verify active_exclusions holds 10401 for scanner_a
        assert_eq!(snap1.active_exclusions["10401"].holder, "scanner_a");

        // Tick 2 (t = 102.0): Test FREQ_ONLY mode on 155.7975 MHz with PRIMARY_SECONDARY (Scanner B primary)
        let mut cfg2 = config;
        cfg2.sync.exclusion_mode = ExclusionMode::FreqOnly;
        cfg2.sync.priority_mode = PriorityMode::PrimarySecondary;
        cfg2.sync.primary_scanner_id = "scanner_b".to_string();
        engine.update_config(cfg2);

        let snap2 = engine.poll_once_sync(102.0);
        assert_eq!(snap2.event_log.len(), 2);
        assert_eq!(snap2.event_log[1].primary_id, "scanner_b");
        assert_eq!(snap2.event_log[1].target_id, "scanner_a");
        assert!((snap2.event_log[1].freq.unwrap() - 155.7975).abs() < 0.0001);
    }

    #[test]
    fn test_fault_injection_one_scanner_disconnected_does_not_stall_healthy_scanner() {
        let mut config = AppConfig::default();
        config.scanner_a.model = ScannerModel::BCD996P2;
        config.scanner_b.model = ScannerModel::BCD996P2;

        // Scanner A is disconnected (fail_connect = true),
        // while Scanner B is connected and receiving an active call!
        let transport_a = ReplayTransport::new_disconnected(true);
        let transport_b = ReplayTransport::from_responses(vec![make_dma_active_transcript(
            "20202",
            "County P25",
            "Sheriff",
            "Sheriff Dispatch",
        )]);

        let drv_a = create_driver_for_model(config.scanner_a.clone(), transport_a);
        let drv_b = create_driver_for_model(config.scanner_b.clone(), transport_b);
        let engine = SyncManager::with_drivers(config, Some(drv_a), Some(drv_b));

        let snap = engine.poll_once_sync(500.0);
        assert!(!snap.scanner_a.connected);
        assert!(snap.scanner_b.connected);
        assert!(snap.scanner_b.receiving);
        assert_eq!(snap.scanner_b.tgid.as_deref(), Some("20202"));
        // Verify Scanner B opened squelch and started recording normally despite Scanner A failing!
        assert!(engine.audio_recorder.lock().unwrap().streams["scanner_b"].has_active_session());
    }

    #[test]
    fn test_disabled_scanner_status_line_and_skip_polling() {
        let mut config = AppConfig::default();
        config.scanner_b.enabled = false;

        let transport_a = ReplayTransport::from_responses(vec![make_dma_active_transcript(
            "10401",
            "County P25",
            "Fire",
            "Fire Dispatch",
        )]);
        let transport_b = ReplayTransport::new_disconnected(false);

        let driver_a = create_driver_for_model(config.scanner_a.clone(), transport_a);
        let driver_b = create_driver_for_model(config.scanner_b.clone(), transport_b);

        let engine = SyncManager::with_drivers(config, Some(driver_a), Some(driver_b));
        let snap = engine.poll_once_sync(100.0);

        assert!(snap.scanner_a.connected);
        assert!(!snap.scanner_b.connected);
        assert_eq!(snap.scanner_b.status_line, "DISABLED");
        assert_eq!(snap.scanner_b.system_name, "");
        assert_eq!(snap.scanner_b.channel_name, "");
    }

    #[test]
    fn test_hardware_samplerate_resampling_to_feeder_rate() {
        let mut config = AppConfig::default();
        config.feeder.sample_rate = 16_000;

        let mut status = ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP");
        status.channel_name = "Dispatch 1".to_string();
        status.tgid = Some("10401".to_string());

        // 1.0 second of 48000 Hz 16-bit mono PCM = 48000 samples = 96000 bytes
        let input_pcm = vec![0_u8; 96_000];
        let call = CompletedCall {
            scanner_id: "scanner_a".to_string(),
            status,
            pcm_bytes: input_pcm,
            duration_seconds: 1.0,
            sample_rate: 48_000,
        };

        let payload = build_dispatch_payload(&config, &call, 1_790_184_600.0);
        let out_pcm = payload.audio_pcm.unwrap();
        // 1.0 second of 16000 Hz 16-bit mono PCM = 16000 samples = 32000 bytes
        assert_eq!(out_pcm.len(), 32_000);
    }
}
