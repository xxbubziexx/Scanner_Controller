//! ID3v2.3 metadata tag generator for MP3 audio files according to `ProScan` specifications.

use chrono::{DateTime, Local};

use crate::clean::clean_ascii_1;
use crate::formatters::format_template;
use crate::pros::build_pros_chunk;
use crate::record::ProScanMetadata;

/// Encodes a 32-bit unsigned integer to a 4-byte `ID3v2` syncsafe integer (big-endian, 7 bits per byte).
#[must_use]
pub const fn encode_syncsafe(val: u32) -> [u8; 4] {
    let b1 = ((val >> 21) & 0x7F) as u8;
    let b2 = ((val >> 14) & 0x7F) as u8;
    let b3 = ((val >> 7) & 0x7F) as u8;
    let b4 = (val & 0x7F) as u8;
    [b1, b2, b3, b4]
}

/// Builds a standard ID3v2.3 text frame with ISO-8859-1 (Latin1) encoding and NUL terminator.
///
/// Byte layout:
/// - 4 bytes: `frame_id` (ASCII)
/// - 4 bytes: size (`u32` big-endian = text length + 2)
/// - 2 bytes: flags (`0x00 0x00`)
/// - 1 byte:  encoding (`0x00`)
/// - N bytes: text (`CleanASCII_1` filtered, <= 253 chars)
/// - 1 byte:  `0x00` terminator
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn build_id3v23_text_frame(frame_id: &str, text: &str) -> Vec<u8> {
    let clean = clean_ascii_1(text, 253);
    let text_bytes = clean.as_bytes();
    let frame_size = (text_bytes.len() + 2) as u32;

    let mut frame = Vec::with_capacity(10 + text_bytes.len() + 1);
    frame.extend_from_slice(frame_id.as_bytes());
    frame.extend_from_slice(&frame_size.to_be_bytes());
    frame.extend_from_slice(&[0x00, 0x00, 0x00]); // 2 flags bytes + 1 encoding byte (0x00 = Latin1)
    frame.extend_from_slice(text_bytes);
    frame.push(0x00);
    frame
}

/// Wraps MP3 audio bytes with a ProScan-compliant ID3v2.3 header at file start (`FileRecording.cs:333`).
///
/// Frames:
/// - `TSEE`: "`ProScan`"
/// - `TRDA`: Date/time formatted `yyyyMMddHHmmss-00000000000000`
/// - `TIT2`: Title template expansion
/// - `TPE1`: Artist template expansion
/// - `TPE2`: Band template expansion
/// - Optional `TRCK`, `TCON`, `COMM` if TGID is non-empty
/// - Embedded `pros` chunk
///
/// Gated: Only writes the tag if at least one of TIT2/TPE1/TPE2 expands to non-empty.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn write_mp3_id3v23(
    audio_bytes: &[u8],
    meta: &ProScanMetadata,
    title_template: &str,
    artist_template: &str,
    band_template: &str,
    now: Option<DateTime<Local>>,
) -> Vec<u8> {
    let dt = now.unwrap_or_else(Local::now);
    let trda_str = format!("{}-00000000000000", dt.format("%Y%m%d%H%M%S"));

    let title_val = if title_template.is_empty() {
        String::new()
    } else {
        format_template(title_template, meta, Some(dt))
    };

    let artist_val = if artist_template.is_empty() {
        String::new()
    } else {
        format_template(artist_template, meta, Some(dt))
    };

    let band_val = if band_template.is_empty() {
        String::new()
    } else {
        format_template(band_template, meta, Some(dt))
    };

    // Gate: ProScan FileRecording.cs:337 writes tag if any of TIT2/TPE1/TPE2 is non-empty
    if title_val.trim().is_empty() && artist_val.trim().is_empty() && band_val.trim().is_empty() {
        return audio_bytes.to_vec();
    }

    let mut frames = Vec::with_capacity(512);
    frames.extend_from_slice(&build_id3v23_text_frame("TSEE", "ProScan"));
    frames.extend_from_slice(&build_id3v23_text_frame("TRDA", &trda_str));
    frames.extend_from_slice(&build_id3v23_text_frame("TIT2", &title_val));
    frames.extend_from_slice(&build_id3v23_text_frame("TPE1", &artist_val));
    frames.extend_from_slice(&build_id3v23_text_frame("TPE2", &band_val));

    let tgid = meta.tgid.trim();
    if !tgid.is_empty() {
        frames.extend_from_slice(&build_id3v23_text_frame("TRCK", tgid));
        frames.extend_from_slice(&build_id3v23_text_frame("TCON", &format!("TGID {tgid}")));
        frames.extend_from_slice(&build_id3v23_text_frame("COMM", &format!("TGID: {tgid}")));
    }

    // Embed the raw pros chunk
    frames.extend_from_slice(&build_pros_chunk(meta));

    let tag_size = frames.len() as u32;
    let syncsafe_size = encode_syncsafe(tag_size);

    // Strip existing ID3v2 header from audio_bytes to avoid duplicate/corrupted tags
    let mut clean_audio = audio_bytes;
    if clean_audio.starts_with(b"ID3") && clean_audio.len() >= 10 {
        let b0 = u32::from(clean_audio[6]);
        let b1 = u32::from(clean_audio[7]);
        let b2 = u32::from(clean_audio[8]);
        let b3 = u32::from(clean_audio[9]);
        let existing_len = (b0 << 21) | (b1 << 14) | (b2 << 7) | b3;
        let mut strip_offset = 10 + existing_len as usize;
        if (clean_audio[5] & 0x10) != 0 {
            strip_offset += 10; // Footer present
        }
        if clean_audio.len() >= strip_offset {
            clean_audio = &clean_audio[strip_offset..];
        }
    }

    let mut result = Vec::with_capacity(10 + frames.len() + clean_audio.len());
    result.extend_from_slice(b"ID3\x03\x00\x00"); // ID3v2.3.0, flags 0
    result.extend_from_slice(&syncsafe_size);
    result.extend_from_slice(&frames);
    result.extend_from_slice(clean_audio);
    result
}
