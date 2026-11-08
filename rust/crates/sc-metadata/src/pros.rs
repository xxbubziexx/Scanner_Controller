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
#[allow(clippy::similar_names)]
pub fn build_pros_chunk(meta: &ProScanMetadata) -> Vec<u8> {
    let mut ending_date = sanitize_field(&meta.ending_date);
    if ending_date.is_empty() {
        ending_date = "00000000000000".to_string();
    }
    while ending_date.len() < 14 {
        ending_date.push('0');
    }
    if ending_date.len() > 14 {
        ending_date.truncate(14);
    }

    let dmr_slot = sanitize_field(&meta.dmr_slot)
        .replace('/', "S")
        .trim()
        .to_string();
    let scanner_mode = sanitize_field(&meta.scanner_mode)
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    let tone_clean = sanitize_field(&meta.tone);
    let uid_clean = sanitize_field(&meta.uid);
    let uid_num_clean = sanitize_field(&meta.uid_number);

    let tone_padded = pad_right(&tone_clean, 24);
    let uid_padded = pad_right(&uid_clean, 64);
    let uid_num_padded = pad_right(&uid_num_clean, 16);

    let scanner = sanitize_field(&meta.scanner);
    let fav = sanitize_field(&meta.favorite_name);
    let sys = sanitize_field(&meta.system_name);
    let site = sanitize_field(&meta.site_name);
    let dept = sanitize_field(&meta.department_name);
    let chan = sanitize_field(&meta.channel_name);
    let freq = sanitize_field(&meta.frequency);
    let modu = sanitize_field(&meta.modulation);
    let tgid = sanitize_field(&meta.tgid);
    let ftoa = sanitize_field(&meta.ftoa);
    let ftob = sanitize_field(&meta.ftob);
    let rssi = sanitize_field(&meta.rssi);
    let alert = sanitize_field(&meta.alert_color);
    let stype = sanitize_field(&meta.service_type);
    let dtype = sanitize_field(&meta.displayed_system_type);
    let dstat = sanitize_field(&meta.digital_status);

    let items: [(&str, &str); 22] = [
        ("Scanner:", &scanner),
        ("EndingDate:", &ending_date),
        ("FavoriteName:", &fav),
        ("SystemName:", &sys),
        ("SiteName:", &site),
        ("DepartmentName:", &dept),
        ("ChannelName:", &chan),
        ("Frequency:", &freq),
        ("Modulation:", &modu),
        ("Tone:", &tone_padded),
        ("TGID:", &tgid),
        ("UID:", &uid_padded),
        ("UID#:", &uid_num_padded),
        ("FTOA:", &ftoa),
        ("FTOB:", &ftob),
        ("RSSI:", &rssi),
        ("AlertColor:", &alert),
        ("ServiceType:", &stype),
        ("DisplayedSystemType:", &dtype),
        ("DigitalStatus:", &dstat),
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

fn sanitize_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(10_000))]

        #[test]
        fn test_pros_roundtrip_never_panics_and_is_idempotent(
            scanner in "[a-zA-Z0-9 _-]{0,30}",
            ending_date in "[0-9]{0,20}",
            favorite_name in "\\PC{0,40}",
            system_name in "\\PC{0,40}",
            site_name in "\\PC{0,40}",
            department_name in "\\PC{0,40}",
            channel_name in "\\PC{0,40}",
            frequency in "[0-9.]{0,12}",
            modulation in "[A-Z]{0,5}",
            tone in "[a-zA-Z0-9 ]{0,24}",
            tgid in "[0-9]{0,10}",
            uid in "[a-zA-Z0-9 ]{0,64}",
            uid_number in "[0-9 ]{0,16}",
            ftoa in "[0-9.]{0,12}",
            ftob in "[0-9.]{0,12}",
            rssi in "[0-9]{0,3}",
            alert_color in "[a-zA-Z]{0,10}",
            service_type in "\\PC{0,30}",
            displayed_system_type in "\\PC{0,30}",
            digital_status in "\\PC{0,30}",
            dmr_slot in "[0-9/S]{0,5}",
            scanner_mode in "[a-zA-Z, ]{0,20}",
        ) {
            let meta = ProScanMetadata {
                scanner,
                ending_date,
                favorite_name,
                system_name,
                site_name,
                department_name,
                channel_name,
                frequency,
                modulation,
                tone,
                tgid,
                uid,
                uid_number,
                ftoa,
                ftob,
                rssi,
                alert_color,
                service_type,
                displayed_system_type,
                digital_status,
                dmr_slot,
                scanner_mode,
                ..Default::default()
            };

            let chunk1 = build_pros_chunk(&meta);
            prop_assert!(chunk1.starts_with(b"pros"));
            prop_assert!(chunk1.len() >= 8);

            let parsed_opt = parse_pros_chunk(&chunk1);
            prop_assert!(parsed_opt.is_some());
            let (parsed1, consumed) = parsed_opt.unwrap();
            prop_assert_eq!(consumed, chunk1.len());

            let chunk2 = build_pros_chunk(&parsed1);
            prop_assert_eq!(&chunk1, &chunk2);

            let (parsed2, consumed2) = parse_pros_chunk(&chunk2).unwrap();
            prop_assert_eq!(consumed2, chunk2.len());
            prop_assert_eq!(parsed1, parsed2);
        }
    }
}
