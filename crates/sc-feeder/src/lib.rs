//! `ScanScribe` transcription pipeline feeder (`DIRECTORY_DROP` and `HTTP_WEBHOOK`).
//!
//! Ported from [`feeder/scanscribe_feeder.py`](../../../../feeder/scanscribe_feeder.py) and
//! [`Plan/phase_4_engine_and_feeder.md`](../../../Plan/phase_4_engine_and_feeder.md) §2.
//! Enforces Prime Directive 1 ("Never lose a call") via atomic temp-file rename and
//! automatic fallback directory archival when primary inbox or webhook fails.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, NaiveDateTime, TimeZone};
use reqwest::blocking::multipart;
use sc_audio::encode_audio_for_dispatch;
pub use sc_audio::is_valid_mp3_payload;
use sc_core::{
    normalize_frequency, FeederMode, ScanScribeFeederConfig, ScannerConfig, ALLOWED_SAMPLE_RATES,
};
use sc_metadata::{format_template, sanitize_filename, ProScanMetadata};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors returned by `sc-feeder`.
#[derive(Debug, Error)]
pub enum FeederError {
    /// Audio encoding error.
    #[error("audio encoding error: {0}")]
    Audio(#[from] sc_audio::AudioError),

    /// I/O error writing to both primary and fallback directories.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Completed radio call payload ready for `ScanScribe` ingest.
///
/// Matches `CallTransmissionPayload` in `feeder/scanscribe_feeder.py:26-40` and
/// `rust/fixtures/feeder/golden_webhook_payload.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallTransmissionPayload {
    pub scanner_id: String,
    #[serde(default = "default_scanner_model")]
    pub scanner_model: String,
    #[serde(default = "default_system_name")]
    pub system_name: String,
    #[serde(default = "default_department_name")]
    pub department_name: String,
    #[serde(default = "default_channel_name")]
    pub channel_name: String,
    #[serde(default)]
    pub tgid: Option<String>,
    #[serde(default)]
    pub frequency: Option<f64>,
    #[serde(default = "default_modulation")]
    pub modulation: String,
    #[serde(default)]
    pub rssi: i32,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub duration_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_pcm: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tit2_template: Option<String>,
}

fn default_scanner_model() -> String {
    "BCD436HP".to_string()
}

fn default_system_name() -> String {
    "Trunk System".to_string()
}

fn default_department_name() -> String {
    "Dispatch".to_string()
}

fn default_channel_name() -> String {
    "Channel 1".to_string()
}

fn default_modulation() -> String {
    "NFM".to_string()
}

impl Default for CallTransmissionPayload {
    fn default() -> Self {
        Self {
            scanner_id: "scanner_a".to_string(),
            scanner_model: default_scanner_model(),
            system_name: default_system_name(),
            department_name: default_department_name(),
            channel_name: default_channel_name(),
            tgid: None,
            frequency: None,
            modulation: default_modulation(),
            rssi: 0,
            timestamp: 0.0,
            duration_seconds: 0.0,
            audio_pcm: None,
            filename_template: None,
            tit2_template: None,
        }
    }
}

/// Sidecar JSON metadata written alongside audio recordings in `DIRECTORY_DROP` mode.
///
/// Matches `rust/fixtures/feeder/golden_sidecar.json` and `feeder/scanscribe_feeder.py:284-308`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarMetadata {
    pub timestamp: f64,
    pub datetime: String,
    pub scanner_id: String,
    pub scanner_model: String,
    pub system_name: String,
    pub department_name: String,
    pub channel_name: String,
    pub tgid: Option<String>,
    pub talkgroup: Option<String>,
    pub talkgroup_id: Option<String>,
    pub talkgroup_num: Option<u64>,
    pub talkgroup_tag: String,
    pub talkgroup_name: String,
    pub talkgroup_group: String,
    pub group_name: String,
    pub frequency: Option<f64>,
    pub frequency_mhz: String,
    pub modulation: String,
    pub rssi: i32,
    pub duration: f64,
    pub duration_seconds: f64,
    pub audio_filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proscan_metadata: Option<ProScanMetadataDict>,
}

/// Exact 22-field `ProScan` dictionary embedded in sidecar JSON (`metadata/proscan_metadata.py:43-67`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProScanMetadataDict {
    #[serde(rename = "Scanner")]
    pub scanner: String,
    #[serde(rename = "EndingDate")]
    pub ending_date: String,
    #[serde(rename = "FavoriteName")]
    pub favorite_name: String,
    #[serde(rename = "SystemName")]
    pub system_name: String,
    #[serde(rename = "SiteName")]
    pub site_name: String,
    #[serde(rename = "DepartmentName")]
    pub department_name: String,
    #[serde(rename = "ChannelName")]
    pub channel_name: String,
    #[serde(rename = "Frequency")]
    pub frequency: String,
    #[serde(rename = "Modulation")]
    pub modulation: String,
    #[serde(rename = "Tone")]
    pub tone: String,
    #[serde(rename = "TGID")]
    pub tgid: String,
    #[serde(rename = "UID")]
    pub uid: String,
    #[serde(rename = "UID#")]
    pub uid_number: String,
    #[serde(rename = "FTOA")]
    pub ftoa: String,
    #[serde(rename = "FTOB")]
    pub ftob: String,
    #[serde(rename = "RSSI")]
    pub rssi: String,
    #[serde(rename = "AlertColor")]
    pub alert_color: String,
    #[serde(rename = "ServiceType")]
    pub service_type: String,
    #[serde(rename = "DisplayedSystemType")]
    pub displayed_system_type: String,
    #[serde(rename = "DigitalStatus")]
    pub digital_status: String,
    #[serde(rename = "DMRSlot")]
    pub dmr_slot: String,
    #[serde(rename = "ScannerMode")]
    pub scanner_mode: String,
}

impl From<&ProScanMetadata> for ProScanMetadataDict {
    fn from(m: &ProScanMetadata) -> Self {
        Self {
            scanner: m.scanner.clone(),
            ending_date: m.ending_date.clone(),
            favorite_name: m.favorite_name.clone(),
            system_name: m.system_name.clone(),
            site_name: m.site_name.clone(),
            department_name: m.department_name.clone(),
            channel_name: m.channel_name.clone(),
            frequency: m.frequency.clone(),
            modulation: m.modulation.clone(),
            tone: m.tone.clone(),
            tgid: m.tgid.clone(),
            uid: m.uid.clone(),
            uid_number: m.uid_number.clone(),
            ftoa: m.ftoa.clone(),
            ftob: m.ftob.clone(),
            rssi: m.rssi.clone(),
            alert_color: m.alert_color.clone(),
            service_type: m.service_type.clone(),
            displayed_system_type: m.displayed_system_type.clone(),
            digital_status: m.digital_status.clone(),
            dmr_slot: m.dmr_slot.clone(),
            scanner_mode: m.scanner_mode.clone(),
        }
    }
}

/// Result returned by [`ScanScribeFeeder::dispatch_call`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// Resolves a collision-free filename stem inside `target_dir` (`{base}` or `{base}_1`, `{base}_2`, ...).
///
/// Ported from `metadata/proscan_metadata.py:resolve_collision_filename`.
#[must_use]
pub fn resolve_collision_filename(target_dir: &Path, filename_base: &str, ext: &str) -> String {
    let base = if filename_base.trim().is_empty() {
        "recording"
    } else {
        filename_base.trim()
    };
    let clean_ext = ext.trim().trim_start_matches('.');
    let initial = target_dir.join(format!("{base}.{clean_ext}"));
    if !initial.exists() {
        return base.to_string();
    }
    let mut counter = 1u32;
    loop {
        let candidate_stem = format!("{base}_{counter}");
        let candidate_path = target_dir.join(format!("{candidate_stem}.{clean_ext}"));
        if !candidate_path.exists() {
            return candidate_stem;
        }
        counter = counter.saturating_add(1);
    }
}

/// Converts a UNIX epoch timestamp (`f64`) into a local [`NaiveDateTime`].
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn unix_to_local_datetime(timestamp: f64) -> NaiveDateTime {
    let secs = timestamp.floor() as i64;
    let nanos = ((timestamp - timestamp.floor()) * 1_000_000_000.0).round() as u32;
    if let chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) =
        Local.timestamp_opt(secs, nanos.min(999_999_999))
    {
        dt.naive_local()
    } else {
        Local::now().naive_local()
    }
}

/// Writes `bytes` to a temporary `.tmp` file in the same directory as `dest` and atomically
/// renames it to `dest` (`Plan/phase_4_engine_and_feeder.md` §2).
fn atomic_write_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = dest.with_extension(format!(
        "{}.tmp",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, dest)
}

/// Packages completed radio transmissions with `ProScan` metadata and dispatches them to `ScanScribe`.
///
/// Ported from `ScanScribeFeeder` (`feeder/scanscribe_feeder.py:111-422`).
#[derive(Debug, Clone)]
pub struct ScanScribeFeeder {
    pub config: ScanScribeFeederConfig,
    pub scanner_configs: HashMap<String, ScannerConfig>,
    fallback_dir_override: Option<PathBuf>,
}

impl ScanScribeFeeder {
    /// Creates a new `ScanScribeFeeder`.
    #[must_use]
    pub fn new(
        config: ScanScribeFeederConfig,
        scanner_configs: HashMap<String, ScannerConfig>,
    ) -> Self {
        Self {
            config,
            scanner_configs,
            fallback_dir_override: None,
        }
    }

    /// Overrides the fallback directory (useful for isolated unit/fault-injection tests).
    pub fn set_fallback_dir_override(&mut self, path: Option<PathBuf>) {
        self.fallback_dir_override = path;
    }

    /// Updates feeder configuration.
    pub fn update_config(&mut self, new_config: ScanScribeFeederConfig) {
        self.config = new_config;
    }

    /// Updates per-scanner configurations used for template resolution.
    pub fn update_scanner_configs(&mut self, scanner_configs: HashMap<String, ScannerConfig>) {
        self.scanner_configs = scanner_configs;
    }

    /// Resolves `(filename_template, tit2_template)` for `payload.scanner_id`.
    ///
    /// Priority (`feeder/scanscribe_feeder.py:127-156`):
    /// 1. Payload explicit override (if provided and non-empty)
    /// 2. Per-scanner configuration from `self.scanner_configs[payload.scanner_id]`
    /// 3. Feeder configuration default
    /// 4. Built-in default (`"%DT - %S - %C (%TG)"` / `"%C (%TG)"`)
    #[must_use]
    pub fn get_scanner_templates(&self, payload: &CallTransmissionPayload) -> (String, String) {
        let sc_cfg = self.scanner_configs.get(&payload.scanner_id);

        let filename_tmpl = payload
            .filename_template
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                sc_cfg.and_then(|c| c.filename_template.as_deref().filter(|s| !s.is_empty()))
            })
            .or_else(|| {
                let s = self.config.filename_template.as_str();
                (!s.is_empty()).then_some(s)
            })
            .unwrap_or("%DT - %S - %C (%TG)")
            .to_string();

        let tit2_tmpl = payload
            .tit2_template
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| sc_cfg.and_then(|c| c.tit2_template.as_deref().filter(|s| !s.is_empty())))
            .or_else(|| {
                let s = self.config.tit2_template.as_str();
                (!s.is_empty()).then_some(s)
            })
            .unwrap_or(if filename_tmpl.is_empty() {
                "%C (%TG)"
            } else {
                filename_tmpl.as_str()
            })
            .to_string();

        (filename_tmpl, tit2_tmpl)
    }

    /// Builds [`ProScanMetadata`] from a [`CallTransmissionPayload`] (`feeder/scanscribe_feeder.py:158-186`).
    #[must_use]
    pub fn build_proscan_metadata(&self, payload: &CallTransmissionPayload) -> ProScanMetadata {
        let ts = if payload.timestamp > 0.0 {
            payload.timestamp
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64())
        };
        let dt = unix_to_local_datetime(ts);
        let ending_date = dt.format("%Y%m%d%H%M%S").to_string();

        let norm_freq = normalize_frequency(payload.frequency);
        let mut chan = payload.channel_name.trim().to_string();
        let sys_name = payload.system_name.trim().to_string();
        let dept_name = payload.department_name.trim().to_string();
        let tgid_val = payload.tgid.as_deref().unwrap_or("").trim().to_string();
        let freq_str = norm_freq.map_or_else(String::new, |f| format!("{f:.4}"));

        // Fallback if channel name is generic or empty (feeder/scanscribe_feeder.py:170-174)
        let chan_lower = chan.to_ascii_lowercase();
        if chan.is_empty()
            || matches!(
                chan_lower.as_str(),
                "scanning"
                    | "scanning..."
                    | "active call"
                    | "channel"
                    | "unknown channel"
                    | "active channel"
            )
        {
            if let Some(f) = norm_freq {
                chan = format!("{f:.4} MHz");
            } else if !tgid_val.is_empty() {
                chan = format!("TGID {tgid_val}");
            }
        }

        ProScanMetadata {
            scanner: if payload.scanner_model.trim().is_empty() {
                "Scanner".to_string()
            } else {
                payload.scanner_model.clone()
            },
            ending_date,
            system_name: sys_name,
            department_name: dept_name,
            channel_name: chan,
            frequency: freq_str,
            modulation: if payload.modulation.trim().is_empty() {
                "NFM".to_string()
            } else {
                payload.modulation.clone()
            },
            tgid: tgid_val,
            rssi: payload.rssi.to_string(),
            ..ProScanMetadata::default()
        }
    }

    /// Generates `ProScan`-tagged audio bytes (MP3 or WAV) and returns `(bytes, extension)`.
    ///
    /// # Errors
    ///
    /// Returns [`FeederError::Audio`] if `self.config.sample_rate` is not in [`ALLOWED_SAMPLE_RATES`].
    pub fn generate_audio_bytes(
        &self,
        payload: &CallTransmissionPayload,
        meta: &ProScanMetadata,
        tit2_template_override: Option<&str>,
    ) -> Result<(Vec<u8>, &'static str), FeederError> {
        let sample_rate = if self.config.sample_rate == 0 {
            16_000
        } else {
            self.config.sample_rate
        };
        if !ALLOWED_SAMPLE_RATES.contains(&sample_rate) {
            return Err(FeederError::Audio(
                sc_audio::AudioError::UnsupportedSampleRate(sample_rate),
            ));
        }

        let resolved_tit2;
        let tit2_template = if let Some(t) = tit2_template_override.filter(|s| !s.is_empty()) {
            t
        } else {
            let (_, t) = self.get_scanner_templates(payload);
            resolved_tit2 = t;
            &resolved_tit2
        };

        let (bytes, ext) = encode_audio_for_dispatch(
            payload.audio_pcm.as_deref(),
            &self.config.audio_format,
            sample_rate,
            meta,
            tit2_template,
            "%S",
        )?;
        Ok((bytes, ext))
    }

    /// Resolves the target inbox directory, guarding against bare drive roots such as `A:\` or `C:\`
    /// (`feeder/scanscribe_feeder.py:234-252`).
    #[must_use]
    pub fn resolve_inbox_directory(&self) -> PathBuf {
        let raw = self
            .config
            .inbox_directory
            .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
        if raw.is_empty() {
            return std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("recordings");
        }

        let target = PathBuf::from(raw);
        let raw_trimmed = raw.trim_end_matches(['\\', '/']);
        // Detect bare Windows drive root like "A:" or "/"
        let is_drive_root =
            raw_trimmed.is_empty() || (raw_trimmed.len() == 2 && raw_trimmed.ends_with(':'));
        if is_drive_root {
            let root = PathBuf::from(format!("{raw_trimmed}\\"));
            let candidate = root
                .join("Scanscribe_Projects")
                .join("scanscribe")
                .join("inbox");
            if candidate.exists() {
                return candidate;
            }
            return root.join("recordings");
        }

        target
    }

    fn resolve_fallback_directory(&self) -> PathBuf {
        if let Some(override_dir) = &self.fallback_dir_override {
            return override_dir.clone();
        }
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("recordings")
    }

    /// Builds the [`SidecarMetadata`] record for a call transmission (`feeder/scanscribe_feeder.py:284-308`).
    #[must_use]
    pub fn build_sidecar_metadata(
        payload: &CallTransmissionPayload,
        meta: &ProScanMetadata,
        dt: NaiveDateTime,
        audio_filename: String,
        include_proscan_dict: bool,
    ) -> SidecarMetadata {
        let tg_val = (!meta.tgid.is_empty()).then(|| meta.tgid.clone());
        let tg_num = meta.tgid.parse::<u64>().ok();
        let norm_freq = normalize_frequency(payload.frequency);
        let freq_mhz = norm_freq.map_or_else(String::new, |f| format!("{f:.4}"));

        SidecarMetadata {
            timestamp: payload.timestamp,
            datetime: dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            scanner_id: payload.scanner_id.clone(),
            scanner_model: payload.scanner_model.clone(),
            system_name: meta.system_name.clone(),
            department_name: meta.department_name.clone(),
            channel_name: meta.channel_name.clone(),
            tgid: tg_val.clone(),
            talkgroup: tg_val.clone(),
            talkgroup_id: tg_val,
            talkgroup_num: tg_num,
            talkgroup_tag: meta.channel_name.clone(),
            talkgroup_name: meta.channel_name.clone(),
            talkgroup_group: meta.department_name.clone(),
            group_name: meta.department_name.clone(),
            frequency: norm_freq,
            frequency_mhz: freq_mhz,
            modulation: payload.modulation.clone(),
            rssi: payload.rssi,
            duration: payload.duration_seconds,
            duration_seconds: payload.duration_seconds,
            audio_filename,
            proscan_metadata: include_proscan_dict.then(|| ProScanMetadataDict::from(meta)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_drop_to_dir(
        &self,
        dir: &Path,
        filename_base: &str,
        ext: &str,
        audio_bytes: &[u8],
        payload: &CallTransmissionPayload,
        meta: &ProScanMetadata,
        dt: NaiveDateTime,
    ) -> std::io::Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(dir)?;
        let final_stem = resolve_collision_filename(dir, filename_base, ext);
        let audio_filename = format!("{final_stem}.{ext}");
        let audio_path = dir.join(&audio_filename);
        let sidecar_path = dir.join(format!("{final_stem}.json"));

        atomic_write_file(&audio_path, audio_bytes)?;

        if self.config.enable_sidecar_json {
            let sidecar = Self::build_sidecar_metadata(payload, meta, dt, audio_filename, true);
            let json_bytes = serde_json::to_vec_pretty(&sidecar)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            atomic_write_file(&sidecar_path, &json_bytes)?;
        }

        Ok((audio_path, sidecar_path))
    }

    /// Dispatches a completed radio call to `ScanScribe` based on the configured [`FeederMode`].
    ///
    /// Guarantees Prime Directive 1 ("Never lose a call"): if the primary inbox directory or
    /// HTTP webhook fails, automatically falls back to saving the recording + sidecar in the
    /// local fallback directory (`./recordings`).
    #[allow(clippy::too_many_lines)]
    pub fn dispatch_call(&self, mut payload: CallTransmissionPayload) -> DispatchResult {
        if !self.config.enable_feeder {
            return DispatchResult {
                status: "disabled".to_string(),
                mode: None,
                audio_file: None,
                sidecar_file: None,
                message: Some("ScanScribe Feeder is disabled in config".to_string()),
                warning: None,
                error: None,
                response: None,
            };
        }

        if payload.timestamp <= 0.0 {
            payload.timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64());
        }
        payload.frequency = normalize_frequency(payload.frequency);

        let dt = unix_to_local_datetime(payload.timestamp);
        let dt_local = Local.from_local_datetime(&dt).single();
        let meta = self.build_proscan_metadata(&payload);
        let (filename_template, tit2_template) = self.get_scanner_templates(&payload);
        let formatted_stem = format_template(&filename_template, &meta, dt_local);
        let filename_base = sanitize_filename(&formatted_stem);

        let (audio_bytes, ext) =
            match self.generate_audio_bytes(&payload, &meta, Some(&tit2_template)) {
                Ok(res) => res,
                Err(err) => {
                    return DispatchResult {
                        status: "error".to_string(),
                        mode: None,
                        audio_file: None,
                        sidecar_file: None,
                        message: None,
                        warning: None,
                        error: Some(err.to_string()),
                        response: None,
                    };
                }
            };

        match self.config.feeder_mode {
            FeederMode::DirectoryDrop => {
                let target_dir = self.resolve_inbox_directory();
                match self.write_drop_to_dir(
                    &target_dir,
                    &filename_base,
                    ext,
                    &audio_bytes,
                    &payload,
                    &meta,
                    dt,
                ) {
                    Ok((audio_path, sidecar_path)) => {
                        tracing::info!(
                            scanner_id = %payload.scanner_id,
                            audio_file = %audio_path.display(),
                            "ScanScribe Feeder: Dropped audio call to inbox"
                        );
                        DispatchResult {
                            status: "success".to_string(),
                            mode: Some("DIRECTORY_DROP".to_string()),
                            audio_file: Some(audio_path.to_string_lossy().into_owned()),
                            sidecar_file: Some(sidecar_path.to_string_lossy().into_owned()),
                            message: None,
                            warning: None,
                            error: None,
                            response: None,
                        }
                    }
                    Err(primary_err) => {
                        tracing::error!(
                            scanner_id = %payload.scanner_id,
                            target_dir = %target_dir.display(),
                            error = %primary_err,
                            "ScanScribe Feeder directory drop error; attempting local fallback"
                        );
                        let fallback_dir = self.resolve_fallback_directory();
                        match self.write_drop_to_dir(
                            &fallback_dir,
                            &filename_base,
                            ext,
                            &audio_bytes,
                            &payload,
                            &meta,
                            dt,
                        ) {
                            Ok((fb_audio, fb_sidecar)) => {
                                tracing::warn!(
                                    scanner_id = %payload.scanner_id,
                                    audio_file = %fb_audio.display(),
                                    "ScanScribe Feeder: Safely saved audio call to fallback inbox"
                                );
                                DispatchResult {
                                    status: "success".to_string(),
                                    mode: Some("DIRECTORY_DROP_FALLBACK".to_string()),
                                    audio_file: Some(fb_audio.to_string_lossy().into_owned()),
                                    sidecar_file: Some(fb_sidecar.to_string_lossy().into_owned()),
                                    message: None,
                                    warning: Some(format!(
                                        "Primary inbox failed ({primary_err}); saved to fallback."
                                    )),
                                    error: None,
                                    response: None,
                                }
                            }
                            Err(fb_err) => DispatchResult {
                                status: "error".to_string(),
                                mode: Some("DIRECTORY_DROP".to_string()),
                                audio_file: None,
                                sidecar_file: None,
                                message: None,
                                warning: None,
                                error: Some(format!("{primary_err} (fallback: {fb_err})")),
                                response: None,
                            },
                        }
                    }
                }
            }
            FeederMode::HttpWebhook => {
                let sidecar = Self::build_sidecar_metadata(
                    &payload,
                    &meta,
                    dt,
                    format!("{filename_base}.{ext}"),
                    false,
                );
                let metadata_json = serde_json::to_string(&sidecar).unwrap_or_default();

                let client = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(3))
                    .build();

                let mut last_err = String::new();
                if let Ok(http) = client {
                    // Retry up to 2 attempts before falling back to local disk
                    for _attempt in 0..2 {
                        let meta_part = multipart::Part::text(metadata_json.clone())
                            .mime_str("application/json");
                        let file_part = multipart::Part::bytes(audio_bytes.clone())
                            .file_name(format!("{filename_base}.{ext}"))
                            .mime_str(&format!("audio/{ext}"));

                        if let (Ok(mp), Ok(fp)) = (meta_part, file_part) {
                            let form = multipart::Form::new().part("metadata", mp).part("file", fp);
                            match http.post(&self.config.webhook_url).multipart(form).send() {
                                Ok(resp) if resp.status().is_success() => {
                                    let body = resp.text().unwrap_or_default();
                                    tracing::info!(
                                        scanner_id = %payload.scanner_id,
                                        webhook_url = %self.config.webhook_url,
                                        "ScanScribe Feeder: Posted call to Webhook"
                                    );
                                    return DispatchResult {
                                        status: "success".to_string(),
                                        mode: Some("HTTP_WEBHOOK".to_string()),
                                        audio_file: None,
                                        sidecar_file: None,
                                        message: None,
                                        warning: None,
                                        error: None,
                                        response: Some(body),
                                    };
                                }
                                Ok(resp) => {
                                    last_err = format!("HTTP status {}", resp.status());
                                }
                                Err(err) => {
                                    last_err = err.to_string();
                                }
                            }
                        }
                    }
                } else {
                    last_err = "Failed to build HTTP client".to_string();
                }

                // Prime Directive 1: Webhook failed -> archive call to fallback directory so it is never lost!
                tracing::warn!(
                    scanner_id = %payload.scanner_id,
                    webhook_url = %self.config.webhook_url,
                    error = %last_err,
                    "ScanScribe Feeder HTTP Webhook dispatch failed; saving call to fallback inbox"
                );
                let fallback_dir = self.resolve_fallback_directory();
                if let Ok((fb_audio, fb_sidecar)) = self.write_drop_to_dir(
                    &fallback_dir,
                    &filename_base,
                    ext,
                    &audio_bytes,
                    &payload,
                    &meta,
                    dt,
                ) {
                    DispatchResult {
                        status: "error".to_string(),
                        mode: Some("HTTP_WEBHOOK".to_string()),
                        audio_file: Some(fb_audio.to_string_lossy().into_owned()),
                        sidecar_file: Some(fb_sidecar.to_string_lossy().into_owned()),
                        message: None,
                        warning: Some(format!(
                            "Webhook failed ({last_err}); archived to fallback {}",
                            fb_audio.display()
                        )),
                        error: Some(last_err),
                        response: None,
                    }
                } else {
                    DispatchResult {
                        status: "error".to_string(),
                        mode: Some("HTTP_WEBHOOK".to_string()),
                        audio_file: None,
                        sidecar_file: None,
                        message: None,
                        warning: None,
                        error: Some(last_err),
                        response: None,
                    }
                }
            }
            FeederMode::LiveStream => DispatchResult {
                status: "ignored".to_string(),
                mode: Some("LIVE_STREAM".to_string()),
                audio_file: None,
                sidecar_file: None,
                message: None,
                warning: None,
                error: None,
                response: None,
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use sc_core::{ScanScribeFeederConfig, ScannerModel};
    use std::path::PathBuf;

    fn feeder_fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("feeder")
    }

    #[test]
    fn test_golden_feeder_fixtures_byte_parity() {
        let dir = feeder_fixtures_dir();
        let golden_webhook_str =
            std::fs::read_to_string(dir.join("golden_webhook_payload.json")).unwrap();
        let golden_sidecar_str = std::fs::read_to_string(dir.join("golden_sidecar.json")).unwrap();

        // 1. Deserialize golden_webhook_payload.json into CallTransmissionPayload and re-serialize
        let payload: CallTransmissionPayload = serde_json::from_str(&golden_webhook_str).unwrap();
        assert_eq!(payload.scanner_id, "scanner_a");
        assert_eq!(payload.scanner_model, "BCD436HP");
        assert_eq!(payload.system_name, "Metropolitan P25 Trunk");
        assert_eq!(payload.department_name, "Fire & Rescue");
        assert_eq!(payload.channel_name, "Dispatch North");
        assert_eq!(payload.tgid.as_deref(), Some("10401"));
        assert_eq!(payload.frequency, Some(851.25));
        assert_eq!(payload.rssi, 4);
        assert_eq!(payload.timestamp, 1_790_184_600.0);
        assert_eq!(payload.duration_seconds, 4.5);

        let reserialized_webhook: serde_json::Value = serde_json::to_value(&payload).unwrap();
        let expected_webhook: serde_json::Value =
            serde_json::from_str(&golden_webhook_str).unwrap();
        assert_eq!(reserialized_webhook, expected_webhook);

        // 2. Build SidecarMetadata from payload and compare against golden_sidecar.json
        let feeder = ScanScribeFeeder::new(ScanScribeFeederConfig::default(), HashMap::new());
        let meta = feeder.build_proscan_metadata(&payload);
        let dt = NaiveDateTime::parse_from_str("2026-09-23 15:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let sidecar = ScanScribeFeeder::build_sidecar_metadata(
            &payload,
            &meta,
            dt,
            "20260923_153000_Dispatch_North.mp3".to_string(),
            false,
        );

        let actual_sidecar_val: serde_json::Value = serde_json::to_value(&sidecar).unwrap();
        let expected_sidecar_val: serde_json::Value =
            serde_json::from_str(&golden_sidecar_str).unwrap();
        assert_eq!(actual_sidecar_val, expected_sidecar_val);
    }

    #[test]
    fn test_directory_drop_and_per_scanner_templates() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sc_feeder_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let cfg = ScanScribeFeederConfig {
            enable_feeder: true,
            feeder_mode: FeederMode::DirectoryDrop,
            inbox_directory: temp_dir.to_string_lossy().into_owned(),
            audio_format: "WAV".to_string(),
            enable_sidecar_json: true,
            ..ScanScribeFeederConfig::default()
        };

        let mut sc_configs = HashMap::new();
        let mut cfg_a = ScannerConfig {
            id: "scanner_a".to_string(),
            name: "Scanner A".to_string(),
            model: ScannerModel::BCD436HP,
            filename_template: Some("%ST_%C_%TG".to_string()),
            tit2_template: Some("%C [%TG]".to_string()),
            ..sc_core::AppConfig::default().scanner_a
        };
        cfg_a.model = ScannerModel::BCD436HP;
        sc_configs.insert("scanner_a".to_string(), cfg_a);

        let feeder = ScanScribeFeeder::new(cfg, sc_configs);
        let payload = CallTransmissionPayload {
            scanner_id: "scanner_a".to_string(),
            scanner_model: "BCD436HP".to_string(),
            system_name: "Metro P25".to_string(),
            department_name: "Fire".to_string(),
            channel_name: "Fire Main".to_string(),
            tgid: Some("101".to_string()),
            frequency: Some(1_557_975.0), // unscaled frequency -> normalized to 155.7975
            duration_seconds: 2.0,
            audio_pcm: Some(vec![0_u8; 3200]),
            ..CallTransmissionPayload::default()
        };

        let res = feeder.dispatch_call(payload);
        assert_eq!(res.status, "success");
        assert_eq!(res.mode.as_deref(), Some("DIRECTORY_DROP"));

        let audio_path = PathBuf::from(res.audio_file.unwrap());
        let sidecar_path = PathBuf::from(res.sidecar_file.unwrap());
        assert!(audio_path.exists());
        assert!(sidecar_path.exists());
        assert!(audio_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("BCD436HP_Fire Main_101"));

        let wav_bytes = std::fs::read(&audio_path).unwrap();
        assert!(wav_bytes
            .windows(b"Fire Main [101]".len())
            .any(|w| w == b"Fire Main [101]"));

        let sidecar_str = std::fs::read_to_string(&sidecar_path).unwrap();
        let sidecar_val: serde_json::Value = serde_json::from_str(&sidecar_str).unwrap();
        assert_eq!(sidecar_val["frequency_mhz"], "155.7975");
        assert_eq!(sidecar_val["proscan_metadata"]["TGID"], "101");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fault_injection_unwritable_inbox_and_webhook_down_never_lose_call() {
        let fallback_dir = std::env::temp_dir().join(format!(
            "sc_feeder_fallback_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&fallback_dir).unwrap();

        // 1. Unwritable/invalid primary inbox path -> falls back to fallback_dir
        let cfg_drop = ScanScribeFeederConfig {
            enable_feeder: true,
            feeder_mode: FeederMode::DirectoryDrop,
            inbox_directory: "Z:\\nonexistent_invalid_drive_99999\\inbox".to_string(),
            audio_format: "WAV".to_string(),
            enable_sidecar_json: true,
            ..ScanScribeFeederConfig::default()
        };
        let mut feeder = ScanScribeFeeder::new(cfg_drop, HashMap::new());
        feeder.set_fallback_dir_override(Some(fallback_dir.clone()));

        let payload = CallTransmissionPayload {
            scanner_id: "scanner_a".to_string(),
            scanner_model: "BCD436HP".to_string(),
            system_name: "Metro P25".to_string(),
            channel_name: "Dispatch 1".to_string(),
            tgid: Some("10401".to_string()),
            audio_pcm: Some(vec![0_u8; 3200]),
            ..CallTransmissionPayload::default()
        };

        let res_drop = feeder.dispatch_call(payload.clone());
        assert_eq!(res_drop.status, "success");
        assert_eq!(res_drop.mode.as_deref(), Some("DIRECTORY_DROP_FALLBACK"));
        assert!(PathBuf::from(res_drop.audio_file.unwrap()).exists());

        // 2. Webhook offline (closed port 127.0.0.1:9) -> archives call to fallback_dir
        let cfg_wh = ScanScribeFeederConfig {
            enable_feeder: true,
            feeder_mode: FeederMode::HttpWebhook,
            webhook_url: "http://127.0.0.1:9/api/ingest".to_string(),
            audio_format: "WAV".to_string(),
            enable_sidecar_json: true,
            ..ScanScribeFeederConfig::default()
        };
        let mut feeder_wh = ScanScribeFeeder::new(cfg_wh, HashMap::new());
        feeder_wh.set_fallback_dir_override(Some(fallback_dir.clone()));

        let res_wh = feeder_wh.dispatch_call(payload);
        assert_eq!(res_wh.status, "error");
        assert_eq!(res_wh.mode.as_deref(), Some("HTTP_WEBHOOK"));
        // Verify Prime Directive 1: call was still archived to fallback_dir!
        let fb_audio = PathBuf::from(res_wh.audio_file.unwrap());
        assert!(fb_audio.exists());

        let _ = std::fs::remove_dir_all(&fallback_dir);
    }
}
