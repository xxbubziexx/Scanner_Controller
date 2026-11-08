//! Binary parser and builder for the proprietary `ProScan` `pros` RIFF chunk.

use crate::record::ProScanMetadata;

/// Builds the binary `pros` scanner record chunk according to `ProScan` specifications (`FileRecording.cs:459`).
///
/// Framing:
/// - `pros` (4 ASCII bytes)
/// - 4-byte `u32` little-endian payload length
/// - Payload (22 key:value strings in exact order, separated by NUL `0x00`)
/// - Length counts payload only (excludes the 8 header bytes), but INCLUDES the `0x00` pad byte if payload is odd.
#[must_use]
pub fn build_pros_chunk(meta: &ProScanMetadata) -> Vec<u8> {
    let mut ending_date = meta.ending_date.trim().to_string();
    if ending_date.is_empty() {
        ending_date = "00000000000000".to_string();
    }
    while ending_date.len() < 14 {
        ending_date.push('0');
    }
    if ending_date.len() > 14 {
        ending_date.truncate(14);
    }

    let dmr_slot = meta.dmr_slot.trim().replace('/', "S");
    let scanner_mode = meta
        .scanner_mode
        .trim()
        .split(',')
        .next()
        .unwrap_or("")
        .to_string();

    let tone_padded = pad_right(&meta.tone, 24);
    let uid_padded = pad_right(&meta.uid, 64);
    let uid_num_padded = pad_right(&meta.uid_number, 16);

    let items: [(&str, &str); 22] = [
        ("Scanner:", meta.scanner.trim()),
        ("EndingDate:", &ending_date),
        ("FavoriteName:", meta.favorite_name.trim()),
        ("SystemName:", meta.system_name.trim()),
        ("SiteName:", meta.site_name.trim()),
        ("DepartmentName:", meta.department_name.trim()),
        ("ChannelName:", meta.channel_name.trim()),
        ("Frequency:", meta.frequency.trim()),
        ("Modulation:", meta.modulation.trim()),
        ("Tone:", &tone_padded),
        ("TGID:", meta.tgid.trim()),
        ("UID:", &uid_padded),
        ("UID#:", &uid_num_padded),
        ("FTOA:", meta.ftoa.trim()),
        ("FTOB:", meta.ftob.trim()),
        ("RSSI:", meta.rssi.trim()),
        ("AlertColor:", meta.alert_color.trim()),
        ("ServiceType:", meta.service_type.trim()),
        ("DisplayedSystemType:", meta.displayed_system_type.trim()),
        ("DigitalStatus:", meta.digital_status.trim()),
        ("DMRSlot:", &dmr_slot),
        ("ScannerMode:", &scanner_mode),
    ];

    let mut payload = Vec::with_capacity(512);
    for (key, val) in items {
        payload.extend_from_slice(key.as_bytes());
        payload.extend_from_slice(val.as_bytes());
        payload.push(0x00);
    }

    // Odd payload alignment: append 0x00 pad byte and include it in declared length (FileRecording.cs:496-501)
    if payload.len() % 2 != 0 {
        payload.push(0x00);
    }

    let payload_len = u32::try_from(payload.len()).unwrap_or(0);
    let mut chunk = Vec::with_capacity(8 + payload.len());
    chunk.extend_from_slice(b"pros");
    chunk.extend_from_slice(&payload_len.to_le_bytes());
    chunk.extend_from_slice(&payload);
    chunk
}

/// Parses a binary blob containing a `pros` chunk.
///
/// Returns `Some((ProScanMetadata, bytes_consumed))` or `None` if `pros` chunk is not found or corrupted.
#[must_use]
pub fn parse_pros_chunk(data: &[u8]) -> Option<(ProScanMetadata, usize)> {
    let pros_pos = find_subslice(data, b"pros")?;
    if data.len() < pros_pos + 8 {
        return None;
    }

    let len_bytes: [u8; 4] = data[pros_pos + 4..pros_pos + 8].try_into().ok()?;
    let payload_len = u32::from_le_bytes(len_bytes) as usize;
    let payload_start = pros_pos + 8;
    let payload_end = payload_start + payload_len;

    if data.len() < payload_end {
        return None;
    }

    let payload = &data[payload_start..payload_end];
    let mut meta = ProScanMetadata::default();

    for entry_bytes in payload.split(|&b| b == 0x00) {
        if entry_bytes.is_empty() {
            continue;
        }
        let entry_str = String::from_utf8_lossy(entry_bytes);
        if let Some((key, val)) = entry_str.split_once(':') {
            match key.trim() {
                "Scanner" => meta.scanner = val.trim().to_string(),
                "EndingDate" => meta.ending_date = val.trim().to_string(),
                "FavoriteName" => meta.favorite_name = val.trim().to_string(),
                "SystemName" => meta.system_name = val.trim().to_string(),
                "SiteName" => meta.site_name = val.trim().to_string(),
                "DepartmentName" => meta.department_name = val.trim().to_string(),
                "ChannelName" => meta.channel_name = val.trim().to_string(),
                "Frequency" => meta.frequency = val.trim().to_string(),
                "Modulation" => meta.modulation = val.trim().to_string(),
                "Tone" => meta.tone = val.trim_end().to_string(),
                "TGID" => meta.tgid = val.trim().to_string(),
                "UID" => meta.uid = val.trim_end().to_string(),
                "UID#" => meta.uid_number = val.trim_end().to_string(),
                "FTOA" => meta.ftoa = val.trim().to_string(),
                "FTOB" => meta.ftob = val.trim().to_string(),
                "RSSI" => meta.rssi = val.trim().to_string(),
                "AlertColor" => meta.alert_color = val.trim().to_string(),
                "ServiceType" => meta.service_type = val.trim().to_string(),
                "DisplayedSystemType" => meta.displayed_system_type = val.trim().to_string(),
                "DigitalStatus" => meta.digital_status = val.trim().to_string(),
                "DMRSlot" => meta.dmr_slot = val.trim().to_string(),
                "ScannerMode" => meta.scanner_mode = val.trim().to_string(),
                _ => {}
            }
        }
    }

    Some((meta, payload_end - pros_pos))
}

fn pad_right(s: &str, width: usize) -> String {
    let mut res = s.to_string();
    while res.len() < width {
        res.push(' ');
    }
    res
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
