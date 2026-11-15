//! Pure string extraction helpers for Uniden multi-command pipelined responses.

use std::time::{SystemTime, UNIX_EPOCH};

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
