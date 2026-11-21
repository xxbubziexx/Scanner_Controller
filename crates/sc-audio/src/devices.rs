//! Audio device enumeration and name-first device resolution via `cpal`.
//!
//! Ported from `audio/recorder.py:12-52` and `Plan/phase_3_audio_pipeline.md` §2.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

const DEVICE_CACHE_TTL: Duration = Duration::from_secs(5);
static INPUT_DEVICE_CACHE: Mutex<Option<(Instant, Vec<AudioDeviceInfo>)>> = Mutex::new(None);
static OUTPUT_DEVICE_CACHE: Mutex<Option<(Instant, Vec<AudioDeviceInfo>)>> = Mutex::new(None);

/// Audio device descriptor matching the `/api/audio/devices` and `/api/audio/output_devices` JSON shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    /// Zero-based index within the enumerated input or output list.
    pub index: usize,
    /// Human-readable device name reported by the OS audio host.
    pub name: String,
    /// Maximum supported channel count on this endpoint (`1` or `2+`).
    pub channels: u16,
    /// Whether this device is the system default input or output endpoint.
    pub is_default: bool,
    /// Host API identifier (`0` for the default OS host API).
    pub hostapi: u32,
}

/// Returns the human-readable name of a `cpal::Device`.
#[must_use]
pub fn resolved_device_name(dev: &cpal::Device) -> String {
    dev.description()
        .map_or_else(|_| dev.to_string(), |d| d.name().to_string())
}

/// Enumerates all system soundcard capture (input) devices, using a short TTL cache
/// unless `force_refresh` is `true`.
#[must_use]
pub fn list_audio_input_devices_cached(force_refresh: bool) -> Vec<AudioDeviceInfo> {
    if !force_refresh {
        if let Ok(guard) = INPUT_DEVICE_CACHE.lock() {
            if let Some((ts, ref cached)) = *guard {
                if ts.elapsed() < DEVICE_CACHE_TTL {
                    return cached.clone();
                }
            }
        }
    }

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .map(|d| resolved_device_name(&d));

    let mut out = Vec::new();
    let Ok(devices) = host.input_devices() else {
        return out;
    };

    for (idx, dev) in devices.enumerate() {
        let name = dev
            .description()
            .map_or_else(|_| format!("Audio Device {idx}"), |d| d.name().to_string());
        let channels = dev
            .default_input_config()
            .map_or(1, |c| c.channels())
            .max(1);
        let is_default = default_name.as_deref().is_some_and(|def| def == name);
        out.push(AudioDeviceInfo {
            index: idx,
            name,
            channels,
            is_default,
            hostapi: 0,
        });
    }

    if let Ok(mut guard) = INPUT_DEVICE_CACHE.lock() {
        *guard = Some((Instant::now(), out.clone()));
    }
    out
}

/// Enumerates all system soundcard capture (input) devices (5 s TTL cached).
#[must_use]
pub fn list_audio_input_devices() -> Vec<AudioDeviceInfo> {
    list_audio_input_devices_cached(false)
}

/// Enumerates all system soundcard playback (output) devices, using a short TTL cache
/// unless `force_refresh` is `true`.
#[must_use]
pub fn list_audio_output_devices_cached(force_refresh: bool) -> Vec<AudioDeviceInfo> {
    if !force_refresh {
        if let Ok(guard) = OUTPUT_DEVICE_CACHE.lock() {
            if let Some((ts, ref cached)) = *guard {
                if ts.elapsed() < DEVICE_CACHE_TTL {
                    return cached.clone();
                }
            }
        }
    }

    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .map(|d| resolved_device_name(&d));

    let mut out = Vec::new();
    let Ok(devices) = host.output_devices() else {
        return out;
    };

    for (idx, dev) in devices.enumerate() {
        let name = dev
            .description()
            .map_or_else(|_| format!("Output Device {idx}"), |d| d.name().to_string());
        let channels = dev
            .default_output_config()
            .map_or(2, |c| c.channels())
            .max(1);
        let is_default = default_name.as_deref().is_some_and(|def| def == name);
        out.push(AudioDeviceInfo {
            index: idx,
            name,
            channels,
            is_default,
            hostapi: 0,
        });
    }

    if let Ok(mut guard) = OUTPUT_DEVICE_CACHE.lock() {
        *guard = Some((Instant::now(), out.clone()));
    }
    out
}

/// Enumerates all system soundcard playback (output) devices (5 s TTL cached).
#[must_use]
pub fn list_audio_output_devices() -> Vec<AudioDeviceInfo> {
    list_audio_output_devices_cached(false)
}

/// Resolves a `cpal` input device by stored device `name` first, falling back to `index`
/// for legacy `config.json` files, or the system default input device when neither is specified.
#[must_use]
pub fn resolve_input_device(
    device_name: Option<&str>,
    device_index: Option<u32>,
) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(target_name) = device_name.map(str::trim).filter(|s| !s.is_empty()) {
        if target_name.eq_ignore_ascii_case("None") {
            return None;
        }
        if target_name.eq_ignore_ascii_case("Primary Sound Capture Device") {
            return host.default_input_device();
        }
        if let Ok(mut devs) = host.input_devices() {
            if let Some(exact) = devs.find(|d| resolved_device_name(d) == target_name) {
                return Some(exact);
            }
        }
        let target_lower = target_name.to_ascii_lowercase();
        if let Ok(mut devs) = host.input_devices() {
            if let Some(sub) = devs.find(|d| {
                resolved_device_name(d)
                    .to_ascii_lowercase()
                    .contains(&target_lower)
            }) {
                return Some(sub);
            }
        }
    }

    if let Some(idx) = device_index {
        if let Ok(mut devs) = host.input_devices() {
            if let Some(dev) = devs.nth(idx as usize) {
                return Some(dev);
            }
        }
    }

    host.default_input_device()
}

/// Resolves a `cpal` output device by stored device `name` first, falling back to `index`
/// for legacy `config.json` files, or the system default output device when neither is specified.
#[must_use]
pub fn resolve_output_device(
    device_name: Option<&str>,
    device_index: Option<u32>,
) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(target_name) = device_name.map(str::trim).filter(|s| !s.is_empty()) {
        if target_name.eq_ignore_ascii_case("None") {
            return None;
        }
        if target_name.eq_ignore_ascii_case("Primary Sound Playback Device") {
            return host.default_output_device();
        }
        if let Ok(mut devs) = host.output_devices() {
            if let Some(exact) = devs.find(|d| resolved_device_name(d) == target_name) {
                return Some(exact);
            }
        }
        let target_lower = target_name.to_ascii_lowercase();
        if let Ok(mut devs) = host.output_devices() {
            if let Some(sub) = devs.find(|d| {
                resolved_device_name(d)
                    .to_ascii_lowercase()
                    .contains(&target_lower)
            }) {
                return Some(sub);
            }
        }
    }

    if let Some(idx) = device_index {
        if let Ok(mut devs) = host.output_devices() {
            if let Some(dev) = devs.nth(idx as usize) {
                return Some(dev);
            }
        }
    }

    host.default_output_device()
}
