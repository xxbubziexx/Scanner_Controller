//! Error types for scanner transport and protocol drivers.

use thiserror::Error;

/// Errors that can occur during scanner communication or protocol parsing.
#[derive(Debug, Error)]
pub enum DriverError {
    #[error("scanner is disabled in configuration")]
    Disabled,

    #[error("no serial port or IP host configured")]
    NoPortConfigured,

    #[error("connection retry backoff active")]
    RetryBackoffActive,

    #[error("not connected to scanner")]
    NotConnected,

    #[error("serial port error on '{port}': {source}")]
    Serial {
        port: String,
        #[source]
        source: serialport::Error,
    },

    #[error("I/O error on '{endpoint}': {source}")]
    Io {
        endpoint: String,
        #[source]
        source: std::io::Error,
    },

    #[error("XML parse error in GSI response: {0}")]
    XmlParse(String),
}
