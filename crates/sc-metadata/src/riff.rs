//! RIFF WAVE container and `LIST`/`INFO` chunk writer according to `ProScan` specifications.

use chrono::{DateTime, Local};

use crate::clean::clean_ascii_1;
use crate::formatters::format_template;
use crate::id3::{build_id3v23_text_frame, encode_syncsafe};
use crate::pros::build_pros_chunk;
use crate::record::ProScanMetadata;

/// Builds a RIFF INFO subchunk with NUL terminator and even byte alignment (`FileRecording.cs:509`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn build_riff_info_subchunk(chunk_id: &str, text: &str) -> Vec<u8> {
    let clean = clean_ascii_1(text, 253);
    let mut raw = clean.into_bytes();
    raw.push(0x00);
    if raw.len() % 2 != 0 {
        raw.push(0x00); // Pad byte for even 2-byte alignment
    }
    let length = raw.len() as u32;

    let mut chunk = Vec::with_capacity(8 + raw.len());
    chunk.extend_from_slice(chunk_id.as_bytes());
    chunk.extend_from_slice(&length.to_le_bytes());
    chunk.extend_from_slice(&raw);
    chunk
}

/// Wraps raw PCM audio bytes into a canonical `ProScan` RIFF WAVE container (`FileRecording.cs:403`).
///
/// Container structure:
/// - `RIFF` header
/// - `fmt ` chunk (`WAVEFORMATEX`)
/// - `data` chunk (PCM bytes with even padding)
/// - `LIST` / `INFO` chunk (`IPRD`, `ICRD`, `INAM`, `IART`, and optional `ICMT`, `IGNR`, `ITRK`, `IKEY`, `ISBJ`)
/// - `id3 ` chunk (for mutagen and modern media player tag compatibility)
/// - `pros` chunk (scanner transmission metadata)
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub fn write_wav_riff_info(
    pcm_bytes: &[u8],
    meta: &ProScanMetadata,
    title_template: &str,
    artist_template: &str,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    now: Option<DateTime<Local>>,
) -> Vec<u8> {
    let dt = now.unwrap_or_else(Local::now);
    let icrd_str = format!("{}-00000000000000", dt.format("%Y%m%d%H%M%S"));

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

    let tgid_str = meta.tgid.trim();

    // 1. fmt chunk (WAVEFORMATEX)
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let mut fmt_chunk = Vec::with_capacity(24);
    fmt_chunk.extend_from_slice(b"fmt \x10\x00\x00\x00"); // 16 bytes payload
    fmt_chunk.extend_from_slice(&1u16.to_le_bytes()); // Format 1 = PCM
    fmt_chunk.extend_from_slice(&channels.to_le_bytes());
    fmt_chunk.extend_from_slice(&sample_rate.to_le_bytes());
    fmt_chunk.extend_from_slice(&byte_rate.to_le_bytes());
    fmt_chunk.extend_from_slice(&block_align.to_le_bytes());
    fmt_chunk.extend_from_slice(&bits_per_sample.to_le_bytes());

    // 2. data chunk
    let pcm_len = pcm_bytes.len() as u32;
    let mut data_chunk = Vec::with_capacity(8 + pcm_bytes.len() + 1);
    data_chunk.extend_from_slice(b"data");
    data_chunk.extend_from_slice(&pcm_len.to_le_bytes());
    data_chunk.extend_from_slice(pcm_bytes);
    if pcm_bytes.len() % 2 != 0 {
        data_chunk.push(0x00);
    }

    // 3. RIFF LIST INFO chunk
    let mut info_subchunks = Vec::with_capacity(256);
    info_subchunks.extend_from_slice(&build_riff_info_subchunk("IPRD", "ProScan"));
    info_subchunks.extend_from_slice(&build_riff_info_subchunk("ICRD", &icrd_str));
    info_subchunks.extend_from_slice(&build_riff_info_subchunk("INAM", &title_val));
    info_subchunks.extend_from_slice(&build_riff_info_subchunk("IART", &artist_val));

    if !tgid_str.is_empty() {
        info_subchunks.extend_from_slice(&build_riff_info_subchunk(
            "ICMT",
            &format!("TGID: {tgid_str}"),
        ));
        info_subchunks.extend_from_slice(&build_riff_info_subchunk("IGNR", tgid_str));
        info_subchunks.extend_from_slice(&build_riff_info_subchunk("ITRK", tgid_str));
        info_subchunks.extend_from_slice(&build_riff_info_subchunk(
            "IKEY",
            &format!("TGID={tgid_str}"),
        ));
        info_subchunks.extend_from_slice(&build_riff_info_subchunk(
            "ISBJ",
            &format!("Talkgroup {tgid_str} - {title_val}"),
        ));
    }

    let mut info_payload = Vec::with_capacity(4 + info_subchunks.len() + 1);
    info_payload.extend_from_slice(b"INFO");
    info_payload.extend_from_slice(&info_subchunks);
    if info_payload.len() % 2 != 0 {
        info_payload.push(0x00);
    }
    let list_len = info_payload.len() as u32;

    let mut list_chunk = Vec::with_capacity(8 + info_payload.len());
    list_chunk.extend_from_slice(b"LIST");
    list_chunk.extend_from_slice(&list_len.to_le_bytes());
    list_chunk.extend_from_slice(&info_payload);

    // 4. id3 chunk in WAV for Mutagen / ID3 readers
    let mut id3_frames = Vec::with_capacity(256);
    id3_frames.extend_from_slice(&build_id3v23_text_frame("TIT2", &title_val));
    id3_frames.extend_from_slice(&build_id3v23_text_frame("TPE1", &artist_val));
    if !meta.department_name.trim().is_empty() {
        id3_frames.extend_from_slice(&build_id3v23_text_frame(
            "TPE2",
            meta.department_name.trim(),
        ));
    }
    if !tgid_str.is_empty() {
        id3_frames.extend_from_slice(&build_id3v23_text_frame("TRCK", tgid_str));
        id3_frames.extend_from_slice(&build_id3v23_text_frame(
            "TCON",
            &format!("TGID {tgid_str}"),
        ));
        id3_frames.extend_from_slice(&build_id3v23_text_frame(
            "COMM",
            &format!("TGID: {tgid_str}"),
        ));
    }

    let id3_tag_size = id3_frames.len() as u32;
    let syncsafe_size = encode_syncsafe(id3_tag_size);

    let mut id3_full = Vec::with_capacity(10 + id3_frames.len() + 1);
    id3_full.extend_from_slice(b"ID3\x03\x00\x00");
    id3_full.extend_from_slice(&syncsafe_size);
    id3_full.extend_from_slice(&id3_frames);
    if id3_full.len() % 2 != 0 {
        id3_full.push(0x00);
    }
    let id3_chunk_len = id3_full.len() as u32;

    let mut id3_chunk = Vec::with_capacity(8 + id3_full.len());
    id3_chunk.extend_from_slice(b"id3 ");
    id3_chunk.extend_from_slice(&id3_chunk_len.to_le_bytes());
    id3_chunk.extend_from_slice(&id3_full);

    // 5. ProScan binary scanner record chunk (pros)
    let pros_chunk = build_pros_chunk(meta);

    // Assemble canonical RIFF WAVE file: fmt -> data -> LIST -> id3 -> pros
    let mut riff_payload = Vec::with_capacity(
        4 + fmt_chunk.len()
            + data_chunk.len()
            + list_chunk.len()
            + id3_chunk.len()
            + pros_chunk.len(),
    );
    riff_payload.extend_from_slice(b"WAVE");
    riff_payload.extend_from_slice(&fmt_chunk);
    riff_payload.extend_from_slice(&data_chunk);
    riff_payload.extend_from_slice(&list_chunk);
    riff_payload.extend_from_slice(&id3_chunk);
    riff_payload.extend_from_slice(&pros_chunk);

    let riff_len = riff_payload.len() as u32;
    let mut riff_header = Vec::with_capacity(8 + riff_payload.len());
    riff_header.extend_from_slice(b"RIFF");
    riff_header.extend_from_slice(&riff_len.to_le_bytes());
    riff_header.extend_from_slice(&riff_payload);
    riff_header
}
