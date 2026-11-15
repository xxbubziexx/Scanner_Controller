//! Audio encoders (RIFF WAV + LAME MP3) and MPEG Layer III sync frame validator.
//!
//! Ported from `feeder/scanscribe_feeder.py:42-109, 188-233` and `Plan/phase_3_audio_pipeline.md` §5.

use std::io::Cursor;

use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use sc_core::ALLOWED_SAMPLE_RATES;
use sc_metadata::{write_mp3_id3v23, write_wav_riff_info, ProScanMetadata};

use crate::error::AudioError;
use crate::resample::bytes_to_i16_samples;

/// Verifies whether `data` contains genuine MPEG Layer III (MP3) audio frames,
/// preventing raw 16-bit PCM audio (which frequently starts with negative samples
/// such as `0xFFFF`) from being misidentified as MP3.
///
/// Ported from `feeder/scanscribe_feeder.py:42-109`.
#[must_use]
pub fn is_valid_mp3_payload(data: &[u8]) -> bool {
    const BITRATE_V1_L3: [usize; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const BITRATE_V2_L3: [usize; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    if data.len() < 64 || data.starts_with(b"RIFF") {
        return false;
    }

    let mut start_idx = 0usize;
    if data.starts_with(b"ID3") && data.len() >= 10 {
        let tag_len = ((usize::from(data[6]) & 0x7F) << 21)
            | ((usize::from(data[7]) & 0x7F) << 14)
            | ((usize::from(data[8]) & 0x7F) << 7)
            | (usize::from(data[9]) & 0x7F);
        start_idx = 10 + tag_len;
        if (data[5] & 0x10) != 0 {
            // ID3v2.4 footer present
            start_idx += 10;
        }
    }

    let n = data.len();
    if start_idx + 4 >= n {
        return false;
    }

    let limit = (n - 4).min(start_idx + 1024);
    let mut i = start_idx;
    while i < limit {
        let b0 = data[i];
        let b1 = data[i + 1];
        if b0 == 0xFF && (b1 & 0xE0) == 0xE0 {
            let version_idx = (b1 >> 3) & 0x03;
            let layer_idx = (b1 >> 1) & 0x03;
            // Layer III is layer_idx == 1; version_idx 1 is reserved
            if layer_idx == 1 && (version_idx == 0 || version_idx == 2 || version_idx == 3) {
                let b2 = data[i + 2];
                let br_idx = usize::from((b2 >> 4) & 0x0F);
                let sr_idx = usize::from((b2 >> 2) & 0x03);
                let padding = usize::from((b2 >> 1) & 0x01);
                if (1..15).contains(&br_idx) && sr_idx < 3 {
                    let sr = match version_idx {
                        3 => [44100usize, 48000, 32000][sr_idx],
                        2 => [22050usize, 24000, 16000][sr_idx],
                        _ => [11025usize, 12000, 8000][sr_idx],
                    };
                    let br = if version_idx == 3 {
                        BITRATE_V1_L3[br_idx] * 1000
                    } else {
                        BITRATE_V2_L3[br_idx] * 1000
                    };
                    let frame_len = if version_idx == 3 {
                        (144 * br) / sr + padding
                    } else {
                        (72 * br) / sr + padding
                    };
                    let next_i = i + frame_len;
                    if next_i + 2 <= n {
                        let nb0 = data[next_i];
                        let nb1 = data[next_i + 1];
                        if nb0 == 0xFF && (nb1 & 0xE0) == 0xE0 {
                            let n_ver = (nb1 >> 3) & 0x03;
                            let n_layer = (nb1 >> 1) & 0x03;
                            if n_ver == version_idx && n_layer == layer_idx {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }

    false
}

/// Validates 16-bit mono PCM using `hound` and writes a complete `ProScan`-tagged RIFF WAV
/// file (`RIFF` + `fmt ` + `data` + `LIST`/`INFO` + `pros` chunk) via [`write_wav_riff_info`].
///
/// # Errors
///
/// Returns [`AudioError`] if `sample_rate` is not in [`ALLOWED_SAMPLE_RATES`], `pcm_bytes`
/// has an odd byte length, or `hound` / `sc-metadata` fails.
pub fn encode_wav_with_metadata(
    pcm_bytes: &[u8],
    sample_rate: u32,
    meta: &ProScanMetadata,
    title_template: &str,
    artist_template: &str,
) -> Result<Vec<u8>, AudioError> {
    if !ALLOWED_SAMPLE_RATES.contains(&sample_rate) {
        return Err(AudioError::UnsupportedSampleRate(sample_rate));
    }
    let samples = bytes_to_i16_samples(pcm_bytes)?;

    // Validate PCM framing with hound::WavWriter into an in-memory cursor
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut hound_buf = Cursor::new(Vec::with_capacity(pcm_bytes.len() + 64));
    {
        let mut writer = hound::WavWriter::new(&mut hound_buf, spec)
            .map_err(|e| AudioError::Wav(e.to_string()))?;
        for &s in &samples {
            writer
                .write_sample(s)
                .map_err(|e| AudioError::Wav(e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| AudioError::Wav(e.to_string()))?;
    }

    // Write the canonical ProScan RIFF WAV with LIST/INFO and pros chunks
    let wav_bytes = write_wav_riff_info(
        pcm_bytes,
        meta,
        title_template,
        artist_template,
        sample_rate,
        1,
        16,
        None,
    );
    Ok(wav_bytes)
}

/// Encodes raw 16-bit mono PCM samples into MPEG Layer III (MP3) frames using LAME (`mp3lame-encoder`),
/// matching the exact bitrate and quality curve from `feeder/scanscribe_feeder.py:213-223`:
///
/// * `8000` or `11025` Hz -> `32 kbps`
/// * `16000` or `22050` Hz -> `64 kbps`
/// * `44100` Hz (or other) -> `128 kbps`
/// * Mono (`1` channel), Quality `2` (`Quality::SecondBest`).
///
/// # Errors
///
/// Returns [`AudioError::Mp3Encode`] if LAME initialization, encoding, or flushing fails.
pub fn encode_mp3_raw_lame(pcm_samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AudioError> {
    let mut builder = Builder::new()
        .ok_or_else(|| AudioError::Mp3Encode("failed to allocate LAME builder".into()))?;
    builder
        .set_num_channels(1)
        .map_err(|e| AudioError::Mp3Encode(format!("set_num_channels(1) failed: {e:?}")))?;
    builder.set_sample_rate(sample_rate).map_err(|e| {
        AudioError::Mp3Encode(format!("set_sample_rate({sample_rate}) failed: {e:?}"))
    })?;

    let brate = match sample_rate {
        8000 | 11025 => Bitrate::Kbps32,
        16000 | 22050 => Bitrate::Kbps64,
        _ => Bitrate::Kbps128,
    };
    builder
        .set_brate(brate)
        .map_err(|e| AudioError::Mp3Encode(format!("set_brate failed: {e:?}")))?;
    builder
        .set_quality(Quality::SecondBest)
        .map_err(|e| AudioError::Mp3Encode(format!("set_quality(2) failed: {e:?}")))?;

    let mut encoder = builder
        .build()
        .map_err(|e| AudioError::Mp3Encode(format!("LAME build failed: {e:?}")))?;

    let max_size = mp3lame_encoder::max_required_buffer_size(pcm_samples.len()) + 7200;
    let mut mp3_out = Vec::with_capacity(max_size);

    encoder
        .encode_to_vec(MonoPcm(pcm_samples), &mut mp3_out)
        .map_err(|e| AudioError::Mp3Encode(format!("LAME encode_to_vec failed: {e:?}")))?;

    mp3_out.reserve(7200);
    encoder
        .flush_to_vec::<FlushNoGap>(&mut mp3_out)
        .map_err(|e| AudioError::Mp3Encode(format!("LAME flush_to_vec failed: {e:?}")))?;

    Ok(mp3_out)
}

/// Generates `ProScan`-tagged audio bytes (MP3 or WAV) and returns `(audio_bytes, file_extension)`.
///
/// Mirrors `ScanScribeFeeder::_generate_audio_bytes` (`feeder/scanscribe_feeder.py:188-233`):
/// * Validates `sample_rate` in [`ALLOWED_SAMPLE_RATES`].
/// * If `payload_pcm` is already a valid MP3 stream and `requested_format` is `"mp3"`,
///   prepends the `ProScan` ID3v2.3 + `pros` tag without re-encoding.
/// * If `requested_format` is `"mp3"` and `payload_pcm` is raw 16-bit PCM, encodes with LAME
///   and prepends the `ProScan` ID3v2.3 + `pros` tag (falling back to RIFF WAV if LAME fails
///   so a call is never lost per Prime Directive 1).
/// * Otherwise encodes as a `ProScan`-tagged RIFF WAV (`LIST`/`INFO` + `pros`).
///
/// # Errors
///
/// Returns [`AudioError`] if `sample_rate` is invalid or metadata writing fails.
pub fn encode_audio_for_dispatch(
    payload_pcm: Option<&[u8]>,
    requested_format: &str,
    sample_rate: u32,
    meta: &ProScanMetadata,
    title_template: &str,
    artist_template: &str,
) -> Result<(Vec<u8>, &'static str), AudioError> {
    if !ALLOWED_SAMPLE_RATES.contains(&sample_rate) {
        return Err(AudioError::UnsupportedSampleRate(sample_rate));
    }

    let fallback_silence;
    let pcm: &[u8] = match payload_pcm {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => {
            // 3 seconds of 16-bit mono silence (scanscribe_feeder.py:197)
            fallback_silence = vec![0_u8; (sample_rate as usize) * 2 * 3];
            &fallback_silence
        }
    };

    if requested_format.trim().eq_ignore_ascii_case("mp3") {
        if is_valid_mp3_payload(pcm) {
            let tagged = write_mp3_id3v23(pcm, meta, title_template, artist_template, "%G", None);
            return Ok((tagged, "mp3"));
        }

        if let Ok(samples) = bytes_to_i16_samples(pcm) {
            match encode_mp3_raw_lame(&samples, sample_rate) {
                Ok(mp3_raw) => {
                    let tagged = write_mp3_id3v23(
                        &mp3_raw,
                        meta,
                        title_template,
                        artist_template,
                        "%G",
                        None,
                    );
                    return Ok((tagged, "mp3"));
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "MP3 encoder unavailable for raw PCM payload; falling back to RIFF WAV for ScanScribe compatibility"
                    );
                }
            }
        }
    }

    let wav_bytes =
        encode_wav_with_metadata(pcm, sample_rate, meta, title_template, artist_template)?;
    Ok((wav_bytes, "wav"))
}
