//! String sanitization and ASCII filtering according to `ProScan` specifications.

/// Filters text to printable ASCII (Asc > 31 and Asc < 127), trims leading/trailing
/// whitespace, and truncates to `max_len` characters.
///
/// Implements `ProScan` `General.CleanASCII_1` (`General.cs:35949`).
#[must_use]
pub fn clean_ascii_1(text: &str, max_len: usize) -> String {
    let filtered: String = text
        .chars()
        .filter(|&c| c as u32 > 31 && (c as u32) < 127)
        .collect();
    let trimmed = filtered.trim();
    if trimmed.len() > max_len {
        trimmed[..max_len].to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_ascii_filtering() {
        assert_eq!(clean_ascii_1("Hello World", 253), "Hello World");
        assert_eq!(clean_ascii_1("  Spaced  ", 253), "Spaced");
        assert_eq!(
            clean_ascii_1("Non-ASCII: Café & naïve", 253),
            "Non-ASCII: Caf & nave"
        );
        assert_eq!(clean_ascii_1("A".repeat(300).as_str(), 253).len(), 253);
    }
}
