//! Audio device enumeration and name-first device resolution via `cpal`.
//!
//! Ported from `audio/recorder.py:12-52` and `Plan/phase_3_audio_pipeline.md` §2.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

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

/// Enumerates all system soundcard capture (input) devices.
#[must_use]
pub fn list_audio_input_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::new();
    let Ok(devices) = host.input_devices() else {
        return out;
    };

    for (idx, dev) in devices.enumerate() {
        let name = dev.name().unwrap_or_else(|_| format!("Audio Device {idx}"));
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
    out
}

/// Enumerates all system soundcard playback (output) devices.
#[must_use]
pub fn list_audio_output_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    let mut out = Vec::new();
    let Ok(devices) = host.output_devices() else {
        return out;
    };

    for (idx, dev) in devices.enumerate() {
        let name = dev
            .name()
            .unwrap_or_else(|_| format!("Output Device {idx}"));
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
    out
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
            if let Some(exact) = devs.find(|d| d.name().ok().as_deref() == Some(target_name)) {
                return Some(exact);
            }
        }
        let target_lower = target_name.to_ascii_lowercase();
        if let Ok(mut devs) = host.input_devices() {
            if let Some(sub) = devs.find(|d| {
                d.name()
                    .is_ok_and(|n| n.to_ascii_lowercase().contains(&target_lower))
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
            if let Some(exact) = devs.find(|d| d.name().ok().as_deref() == Some(target_name)) {
                return Some(exact);
            }
        }
        let target_lower = target_name.to_ascii_lowercase();
        if let Ok(mut devs) = host.output_devices() {
            if let Some(sub) = devs.find(|d| {
                d.name()
                    .is_ok_and(|n| n.to_ascii_lowercase().contains(&target_lower))
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
