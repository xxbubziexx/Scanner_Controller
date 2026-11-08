//! `sc-metadata`: `ProScan` pros chunks, ID3v2.3, RIFF LIST/INFO, and 33 % formatters.

#![deny(unsafe_code)]

/// Placeholder to verify crate compilation.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(version(), "0.1.0");
    }
}
