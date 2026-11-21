//! Pure string extraction helpers for Uniden multi-command pipelined responses.

use std::time::{SystemTime, UNIX_EPOCH};

use sc_core::{DisplayLine, ScannerStatus};

/// Resets all active call/frequency metadata fields and sets `status_line` for an inactive scanner
/// (e.g. `"DISABLED"` or `"NO PORT (SELECT IN SETTINGS)"`).
pub fn set_inactive_status(status: &mut ScannerStatus, status_line: &str) {
    status.connected = false;
    status.receiving = false;
    status.avoided = false;
    status.rssi = 0;
    status.tgid = None;
    status.frequency = None;
    status.unit_id = None;
    status.tone = None;
    status.system_name.clear();
    status.dept_name.clear();
    status.channel_name.clear();
    status.display_lines.clear();
    status.status_line = status_line.to_string();
}

/// Extracts the raw payload of a specific command tag from a CR/LF-delimited pipelined response
/// without trimming leading/trailing spaces (preserving fixed-width LCD column alignment).
#[must_use]
pub fn extract_section_raw<'a>(data: &'a str, key: &str) -> Option<&'a str> {
    for line in data.split(['\r', '\n']) {
        if let Some(rest) = line.strip_prefix(key) {
            return Some(rest);
        }
    }
    None
}

/// Converts non-printable Uniden glyph bytes (`< 0x20`, `0x7F`, non-ASCII) into single spaces
/// so character column alignment with `STS` mode masks stays intact.
#[must_use]
pub fn clean_lcd_row_preserving_columns(text: &str) -> String {
    text.chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { ' ' })
        .collect()
}

fn is_sts_mode_token(tok: &str) -> bool {
    tok.is_empty()
        || tok
            .bytes()
            .all(|b| matches!(b, b' ' | b'*' | b'_' | b'#' | b'^'))
}

/// Parses a Uniden `STS` response (either with or without leading `"STS,"`) into structured
/// [`DisplayLine`] entries preserving font size (`DISP_FORM`) and highlight/divider attributes.
#[must_use]
pub fn parse_sts_display_lines(sts_payload: &str) -> Vec<DisplayLine> {
    let payload = sts_payload.strip_prefix("STS,").unwrap_or(sts_payload);
    let tokens: Vec<&str> = payload.split(',').collect();
    if tokens.len() < 2 {
        return Vec::new();
    }

    let disp_form = tokens[0].trim();
    if disp_form.len() < 2 || !disp_form.bytes().all(|b| b == b'0' || b == b'1') {
        return Vec::new();
    }

    let mut lines = Vec::with_capacity(disp_form.len());
    let mut idx = 1usize;

    for row_idx in 0..disp_form.len() {
        if idx >= tokens.len() {
            break;
        }
        let mut raw_text = tokens[idx].to_string();
        idx += 1;

        // Rejoin unescaped literal commas inside scanner alpha tags (stopping if the next token is a mode mask).
        while idx < tokens.len() && !is_sts_mode_token(tokens[idx]) && raw_text.len() <= 64 {
            raw_text.push(',');
            raw_text.push_str(tokens[idx]);
            idx += 1;
        }

        let raw_mode = if idx < tokens.len() {
            let m = tokens[idx];
            idx += 1;
            m
        } else {
            ""
        };

        let text = clean_lcd_row_preserving_columns(&raw_text);
        let large = disp_form.as_bytes().get(row_idx) == Some(&b'1');
        let underscore_count = raw_mode.bytes().filter(|&b| b == b'_').count();
        let underline_divider =
            underscore_count >= 8 && raw_mode.bytes().all(|b| b == b'_' || b == b' ');
        let mode =
            if underline_divider || !raw_mode.bytes().any(|b| matches!(b, b'*' | b'_' | b'#')) {
                String::new()
            } else {
                raw_mode.to_string()
            };

        lines.push(DisplayLine {
            text,
            mode,
            large,
            underline_divider,
        });
    }

    let any_visible = lines
        .iter()
        .any(|l| !l.text.trim().is_empty() || !l.mode.is_empty() || l.underline_divider);
    if any_visible {
        lines
    } else {
        Vec::new()
    }
}

/// Extracts any `STS,` section from a single or pipelined response and updates
/// `status.display_lines` when at least one non-blank LCD line is present.
pub fn update_sts_display_lines(res: &str, status: &mut ScannerStatus) {
    if let Some(sts_raw) = extract_section_raw(res, "STS,") {
        let lines = parse_sts_display_lines(sts_raw);
        if !lines.is_empty() {
            status.display_lines = lines;
        }
    }
}

/// Extracts the payload of a specific command tag from a CR/LF-delimited pipelined response.
///
/// Matches `(?:^|[\r\n]){key}(.*?)(?:[\r\n]|$)` (e.g. `extract_section(resp, "GLG,")` finds
/// `\rGLG,<payload>\r` or `^GLG,<payload>\r`).
#[must_use]
pub fn extract_section(data: &str, key: &str) -> Option<String> {
    for line in data.split(['\r', '\n']) {
        if let Some(rest) = line.strip_prefix(key) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Returns the current UNIX timestamp in fractional seconds.
#[must_use]
pub fn now_unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section_pipelined() {
        let sample =
            "MDL,BCD996P2\rSTS,000000\rGLG,155.7975,NFM,,0,Sys,Dept,Chan,1\rPWR,4,15579750\r";
        assert_eq!(extract_section(sample, "MDL,").as_deref(), Some("BCD996P2"));
        assert_eq!(
            extract_section(sample, "GLG,").as_deref(),
            Some("155.7975,NFM,,0,Sys,Dept,Chan,1")
        );
        assert_eq!(
            extract_section(sample, "PWR,").as_deref(),
            Some("4,15579750")
        );
        assert_eq!(extract_section(sample, "GSI,"), None);
    }
}
