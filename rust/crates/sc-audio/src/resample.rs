//! Audio sample-rate conversion (`resample_pcm_int16` linear & `rubato` band-limited resamplers).
//!
//! Port of `engine/sync_manager.py:38-56` and `audio/recorder.py:307-318`.

use rubato::{FftFixedInOut, Resampler};

use crate::error::AudioError;

/// Converts raw little-endian 16-bit PCM bytes into a `Vec<i16>`.
///
/// # Errors
///
/// Returns [`AudioError::InvalidPcmByteLength`] if `pcm_bytes.len()` is odd.
pub fn bytes_to_i16_samples(pcm_bytes: &[u8]) -> Result<Vec<i16>, AudioError> {
    if pcm_bytes.len() % 2 != 0 {
        return Err(AudioError::InvalidPcmByteLength(pcm_bytes.len()));
    }
    let mut out = Vec::with_capacity(pcm_bytes.len() / 2);
    for pair in pcm_bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    Ok(out)
}

/// Converts a slice of `i16` samples into little-endian 16-bit PCM bytes.
#[must_use]
pub fn i16_samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Resamples 16-bit mono PCM bytes from `in_rate` to `out_rate` matching Python's
/// `engine/sync_manager.py:resample_pcm_int16` (`np.linspace(0, len - 1, num_out)`).
///
/// # Errors
///
/// Returns [`AudioError::InvalidPcmByteLength`] if `pcm_bytes` has an odd byte length,
/// or [`AudioError::UnsupportedSampleRate`] if `in_rate == 0` or `out_rate == 0`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn resample_pcm_int16(
    pcm_bytes: &[u8],
    in_rate: u32,
    out_rate: u32,
) -> Result<Vec<u8>, AudioError> {
    if in_rate == 0 {
        return Err(AudioError::UnsupportedSampleRate(in_rate));
    }
    if out_rate == 0 {
        return Err(AudioError::UnsupportedSampleRate(out_rate));
    }
    if in_rate == out_rate || pcm_bytes.is_empty() {
        if pcm_bytes.len() % 2 != 0 {
            return Err(AudioError::InvalidPcmByteLength(pcm_bytes.len()));
        }
        return Ok(pcm_bytes.to_vec());
    }

    let arr = bytes_to_i16_samples(pcm_bytes)?;
    let n_in = arr.len();
    if n_in == 0 {
        return Ok(Vec::new());
    }

    let ratio = f64::from(in_rate) / f64::from(out_rate);
    let num_out = ((n_in as f64) / ratio) as usize;
    if num_out == 0 {
        return Ok(pcm_bytes.to_vec());
    }

    let mut out_bytes = Vec::with_capacity(num_out * 2);
    if num_out == 1 || n_in == 1 {
        out_bytes.extend_from_slice(&arr[0].to_le_bytes());
        return Ok(out_bytes);
    }

    let max_in_idx = (n_in - 1) as f32;
    let denom = (num_out - 1) as f32;
    for i in 0..num_out {
        let t = (i as f32) * max_in_idx / denom;
        let idx0 = (t.floor() as usize).min(n_in - 1);
        let idx1 = (idx0 + 1).min(n_in - 1);
        let frac = f64::from(t - (idx0 as f32));
        let y0 = f64::from(arr[idx0]);
        let y1 = f64::from(arr[idx1]);
        let val = y0 + frac * (y1 - y0);
        let clamped = val.clamp(-32768.0, 32767.0) as i16;
        out_bytes.extend_from_slice(&clamped.to_le_bytes());
    }

    Ok(out_bytes)
}

/// Resamples 16-bit mono PCM bytes from `in_rate` to `out_rate` using `rubato`'s
/// band-limited FFT resampler (`FftFixedInOut`).
///
/// # Errors
///
/// Returns [`AudioError`] if `pcm_bytes` has an odd byte length or if `rubato` fails to construct.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn resample_pcm_rubato(
    pcm_bytes: &[u8],
    in_rate: u32,
    out_rate: u32,
) -> Result<Vec<u8>, AudioError> {
    if in_rate == 0 {
        return Err(AudioError::UnsupportedSampleRate(in_rate));
    }
    if out_rate == 0 {
        return Err(AudioError::UnsupportedSampleRate(out_rate));
    }
    if in_rate == out_rate || pcm_bytes.is_empty() {
        if pcm_bytes.len() % 2 != 0 {
            return Err(AudioError::InvalidPcmByteLength(pcm_bytes.len()));
        }
        return Ok(pcm_bytes.to_vec());
    }

    let samples_i16 = bytes_to_i16_samples(pcm_bytes)?;
    let n_in = samples_i16.len();
    let expected_out = ((n_in as u64 * u64::from(out_rate)) / u64::from(in_rate)) as usize;
    if expected_out == 0 {
        return Ok(Vec::new());
    }

    let mut resampler = FftFixedInOut::<f32>::new(in_rate as usize, out_rate as usize, 1024, 1)
        .map_err(|e| AudioError::Resample(e.to_string()))?;

    let input_frames_needed = resampler.input_frames_next();
    let mut f32_in: Vec<f32> = samples_i16
        .iter()
        .map(|&s| f32::from(s) / 32768.0)
        .collect();
    let delay = resampler.output_delay();

    // Pad input so all original samples plus filter delay flush through
    let total_needed = n_in + input_frames_needed * 2;
    if f32_in.len() < total_needed {
        f32_in.resize(total_needed, 0.0);
    }

    let mut out_f32 = Vec::with_capacity(expected_out + resampler.output_frames_max() * 2);
    let mut pos = 0;
    while pos + input_frames_needed <= f32_in.len() {
        let chunk = [&f32_in[pos..pos + input_frames_needed]];
        let waves_out = resampler
            .process(&chunk, None)
            .map_err(|e| AudioError::Resample(e.to_string()))?;
        if let Some(ch0) = waves_out.first() {
            out_f32.extend_from_slice(ch0);
        }
        pos += input_frames_needed;
    }

    let start = delay.min(out_f32.len());
    let end = (start + expected_out).min(out_f32.len());
    let trimmed = &out_f32[start..end];

    let mut out_bytes = Vec::with_capacity(trimmed.len() * 2);
    for &sample in trimmed {
        let scaled = (sample * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        out_bytes.extend_from_slice(&scaled.to_le_bytes());
    }

    Ok(out_bytes)
}

/// Fast linear resampler for `f32` mono loopback buffers (`audio/recorder.py:311-317`).
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn resample_f32_linear(mono: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if mono.is_empty() || in_rate == 0 || out_rate == 0 {
        return Vec::new();
    }
    if in_rate == out_rate {
        return mono.to_vec();
    }
    let n = mono.len();
    let target_len = ((n as f64) * (f64::from(out_rate) / f64::from(in_rate))).round() as usize;
    if target_len == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![mono[0]; target_len];
    }

    let mut out = Vec::with_capacity(target_len);
    let n_f32 = n as f32;
    let target_f32 = target_len as f32;
    for j in 0..target_len {
        // Matches np.linspace(0, 1, N, endpoint=False) vs np.linspace(0, 1, target_len, endpoint=False)
        let pos = (j as f32) * (n_f32 / target_f32);
        let idx0 = (pos.floor() as usize).min(n - 1);
        let idx1 = (idx0 + 1).min(n - 1);
        let frac = pos - (idx0 as f32);
        let sample = mono[idx0] + frac * (mono[idx1] - mono[idx0]);
        out.push(sample);
    }
    out
}
