//! `ProScan` template formatter engines (Engine A & Engine B) and path sanitizers.

use chrono::{DateTime, Datelike, Local, Timelike};

use crate::record::ProScanMetadata;

/// Canonical `ProScan` Engine A template formatter (`General.GetFormatters`, `General.cs:33805`).
///
/// Drives recording filenames, subfolders, window captions, and MP3/WAV metadata tags.
/// Supports all 37 custom `%` specifiers and the three critical inversions:
/// - `%MM` is Month Name (e.g. "Sep"), `%M` is Month Number (e.g. "09")
/// - `%YY` is 4-digit Year (e.g. "2026"), `%Y` is 2-digit Year (e.g. "26")
/// - `%D` is Full Date (e.g. "09/23/26"), `%DD` is 2-digit Day (e.g. "23")
#[must_use]
#[allow(clippy::similar_names)]
pub fn format_template(
    template: &str,
    meta: &ProScanMetadata,
    now: Option<DateTime<Local>>,
) -> String {
    let dt = now.unwrap_or_else(Local::now);

    // Hybrid %FT: TalkGroup if present, else frequency
    let ft_val = if meta.tgid.trim().is_empty() {
        meta.frequency.trim()
    } else {
        meta.tgid.trim()
    };

    // Frequency with DMR slot (%F)
    let freq_with_dmr = if meta.dmr_slot.trim().is_empty() {
        meta.frequency.trim().to_string()
    } else {
        let slot = meta.dmr_slot.trim().replace('/', "S");
        if slot.starts_with('S') {
            format!("{}{}", meta.frequency.trim(), slot)
        } else {
            format!("{}S{}", meta.frequency.trim(), slot)
        }
    };

    let port_str = meta.comm_port.trim();
    let port_val = if port_str.to_ascii_uppercase().starts_with("COM")
        && port_str.len() > 3
        && port_str[3..].chars().all(|c| c.is_ascii_digit())
    {
        &port_str[3..]
    } else {
        port_str
    };

    let prog_filename = if !meta.program_filename.trim().is_empty() {
        meta.program_filename.trim()
    } else if let Some((_, file)) = meta.program_path.trim().rsplit_once(&['/', '\\'][..]) {
        file
    } else {
        meta.program_path.trim()
    };

    // Date/Time formatting helpers
    let dd_str = dt.format("%d").to_string();
    let dt_str = format_dt_custom(&dt);
    let tt_str = format_time_custom(&dt);
    let h_str = dt.format("%H").to_string();
    let mm_str = dt.format("%b").to_string();
    let yy_str = dt.format("%Y").to_string();
    let d_str = dt.format("%m/%d/%y").to_string();
    let m_str = dt.format("%m").to_string();
    let y_str = dt.format("%y").to_string();

    // 37 replacements in exact ProScan evaluation order (Common_PS_RF.cs / General.cs)
    let replacements: [(&str, &str); 37] = [
        ("%FT", ft_val),
        ("%TG", meta.tgid.trim()),
        ("%L2", meta.system_name.trim()),
        ("%L1", meta.channel_name.trim()),
        ("%ST1", meta.service_type.trim()),
        ("%STC", meta.service_color.trim()),
        ("%AC", meta.alert_color.trim()),
        ("%SC", meta.info_sc_state.trim()),
        ("%ST", meta.scanner.trim()),
        ("%DD", &dd_str),
        ("%DT", &dt_str),
        ("%TT", &tt_str),
        ("%H", &h_str),
        ("%MM", &mm_str),
        ("%YY", &yy_str),
        ("%AF1", meta.program_path.trim()),
        ("%AF2", prog_filename),
        ("%FA", meta.favorite_name.trim()),
        ("%SI", meta.site_name.trim()),
        ("%S", meta.system_name.trim()),
        ("%G", meta.department_name.trim()),
        ("%B", meta.department_name.trim()),
        ("%DE", meta.department_name.trim()),
        ("%C", meta.channel_name.trim()),
        ("%F", &freq_with_dmr),
        ("%TA", meta.ftoa.trim()),
        ("%TB", meta.ftob.trim()),
        ("%T", meta.tone.trim()),
        ("%MD", meta.modulation.trim()),
        ("%R", meta.rssi.trim()),
        ("%U1", meta.uid.trim()),
        ("%U2", meta.uid_number.trim()),
        ("%U3", meta.uid_number.trim()),
        ("%P", port_val),
        ("%D", &d_str),
        ("%M", &m_str),
        ("%Y", &y_str),
    ];

    let mut result = template.to_string();
    for (spec, val) in replacements {
        result = result.replace(spec, val);
    }
    result
}

/// Streaming metadata Engine B formatter (`Common_PS_RF.cs:784`, `:801`).
///
/// Drives live metadata streams (Source Client / Icecast / Web Server).
/// Does not process date/time formatters, but supports `%CL` (listener count).
#[must_use]
pub fn format_streaming_metadata(
    template: &str,
    meta: &ProScanMetadata,
    listener_count: u32,
) -> String {
    let ft_val = if meta.tgid.trim().is_empty() {
        meta.frequency.trim()
    } else {
        meta.tgid.trim()
    };

    let freq_with_dmr = if meta.dmr_slot.trim().is_empty() {
        meta.frequency.trim().to_string()
    } else {
        let slot = meta.dmr_slot.trim().replace('/', "S");
        if slot.starts_with('S') {
            format!("{}{}", meta.frequency.trim(), slot)
        } else {
            format!("{}S{}", meta.frequency.trim(), slot)
        }
    };

    let cl_str = listener_count.to_string();

    let replacements: [(&str, &str); 19] = [
        ("%FT", ft_val),
        ("%TG", meta.tgid.trim()),
        ("%L2", meta.system_name.trim()),
        ("%L1", meta.channel_name.trim()),
        ("%FA", meta.favorite_name.trim()),
        ("%SI", meta.site_name.trim()),
        ("%S", meta.system_name.trim()),
        ("%G", meta.department_name.trim()),
        ("%B", meta.department_name.trim()),
        ("%DE", meta.department_name.trim()),
        ("%CL", &cl_str),
        ("%C", meta.channel_name.trim()),
        ("%F", &freq_with_dmr),
        ("%T", meta.tone.trim()),
        ("%MD", meta.modulation.trim()),
        ("%R", meta.rssi.trim()),
        ("%U1", meta.uid.trim()),
        ("%U2", meta.uid_number.trim()),
        ("%U3", meta.uid_number.trim()),
    ];

    let mut result = template.to_string();
    for (spec, val) in replacements {
        result = result.replace(spec, val);
    }
    result
}

/// Sanitizes a string for safe usage as a Windows/Linux filename according to `ProScan` specifications.
///
/// Implements `FilterFileNamesLegalCharactersOnly` (`Recorder.cs:1220`, `General.cs:34069`):
/// 1. Replaces `/`, `\`, `:` with `-`
/// 2. Allowlists characters: `A-Za-z0-9!@#$%^&()-_+={}[];,. `
/// 3. Trims whitespace
/// 4. Strips trailing dots
/// 5. Fallback: "No Custom Formatters Specified"
#[must_use]
pub fn sanitize_filename(filename: &str) -> String {
    let replaced: String = filename
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();

    let cleaned: String = replaced
        .chars()
        .filter(|&c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '!' | '@'
                        | '#'
                        | '$'
                        | '%'
                        | '^'
                        | '&'
                        | '('
                        | ')'
                        | '-'
                        | '_'
                        | '+'
                        | '='
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | ';'
                        | ','
                        | '.'
                        | ' '
                )
        })
        .collect();

    let mut trimmed = cleaned.trim().to_string();
    while trimmed.ends_with('.') {
        trimmed.pop();
    }

    if trimmed.is_empty() {
        "No Custom Formatters Specified".to_string()
    } else {
        trimmed
    }
}

/// Sanitizes a folder path string according to `ProScan` specifications.
///
/// Implements `FilterFolderLegalCharactersOnly` (`Recorder.cs:1204`, `General.cs:34031`):
/// 1. Replaces `/` with `-`
/// 2. Applies folder allowlist (permits `\`)
/// 3. Drive-letter guard: protects first 3 characters (e.g. `C:\`), then turns remaining colons to `-`
/// 4. Strips trailing dots
/// 5. Ensures trailing `\`
#[must_use]
pub fn sanitize_folder_path(folder_path: &str) -> String {
    let raw: String = folder_path
        .chars()
        .map(|c| if c == '/' { '-' } else { c })
        .collect();

    let cleaned: String = raw
        .chars()
        .filter(|&c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '!' | '@'
                        | '#'
                        | '$'
                        | '%'
                        | '^'
                        | '&'
                        | '('
                        | ')'
                        | '-'
                        | '_'
                        | '+'
                        | '='
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | ';'
                        | ','
                        | '.'
                        | ' '
                        | '\\'
                        | ':'
                )
        })
        .collect();

    let cleaned_trim = cleaned.trim();
    let mut path = if cleaned_trim.len() >= 3
        && (&cleaned_trim[1..3] == ":\\" || &cleaned_trim[1..3] == ":/")
    {
        let prefix = &cleaned_trim[..3];
        let rest = cleaned_trim[3..].replace(':', "-");
        format!("{prefix}{rest}")
    } else {
        cleaned_trim.replace(':', "-")
    };

    while path.ends_with('.') {
        path.pop();
    }

    if !path.ends_with('\\') && !path.ends_with('/') {
        path.push('\\');
    }
    path
}

fn format_dt_custom(dt: &DateTime<Local>) -> String {
    // Format: MM/dd/yy hh:mm:ss tt (with single digit hour without leading zero)
    let hour = dt.hour12().1;
    let am_pm = if dt.hour() >= 12 { "PM" } else { "AM" };
    format!(
        "{:02}/{:02}/{:02} {}:{:02}:{:02} {}",
        dt.month(),
        dt.day(),
        dt.year() % 100,
        hour,
        dt.minute(),
        dt.second(),
        am_pm
    )
}

fn format_time_custom(dt: &DateTime<Local>) -> String {
    // Format: hh:mm:ss tt (single digit hour without leading zero)
    let hour = dt.hour12().1;
    let am_pm = if dt.hour() >= 12 { "PM" } else { "AM" };
    format!("{}:{:02}:{:02} {}", hour, dt.minute(), dt.second(), am_pm)
}
