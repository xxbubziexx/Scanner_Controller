//! Hardware and replay transport abstractions for Uniden scanner protocols.
//!
//! Per the Phase 2 Serial I/O Decision Spike, `Transport` is a synchronous trait intended
//! to run on a dedicated blocking thread per scanner (or inside `tokio::task::spawn_blocking`),
//! avoiding `Box<dyn Future>` per-poll heap allocations and Windows overlapped COM port quirks.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::Duration;

use serialport::{ClearBuffer, SerialPort};

use crate::error::DriverError;

/// Normalize a bare digit port string (e.g. `"3"`) to `"COM3"` on Windows,
/// matching `ProScan` (`FrmMain.cs:30007`: `PortName = "COM" + Main.CommPort`).
#[must_use]
pub fn normalize_port_name(raw_port: &str) -> String {
    let trimmed = raw_port.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        format!("COM{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Synchronous line-oriented transport for Uniden ASCII command protocols.
pub trait Transport: Send {
    /// Opens the underlying transport connection.
    ///
    /// # Errors
    /// Returns [`DriverError`] if the port or socket cannot be opened.
    fn connect(&mut self) -> Result<(), DriverError>;

    /// Closes the underlying connection.
    fn disconnect(&mut self);

    /// Returns `true` if the transport is currently open.
    fn is_connected(&self) -> bool;

    /// Sends an ASCII command (without trailing `\r`, which the transport appends)
    /// and reads the scanner's response.
    ///
    /// # Errors
    /// Returns [`DriverError`] if writing or reading fails.
    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError>;
}

/// Blocking serial COM/TTY port transport using `serialport`.
pub struct SerialTransport {
    port_name: String,
    baud_rate: u32,
    timeout: Duration,
    port: Option<Box<dyn SerialPort>>,
}

impl SerialTransport {
    /// Creates a new `SerialTransport` (not yet connected).
    #[must_use]
    pub fn new(port_name: impl Into<String>, baud_rate: u32) -> Self {
        let raw = port_name.into();
        Self {
            port_name: normalize_port_name(&raw),
            baud_rate,
            timeout: Duration::from_millis(300),
            port: None,
        }
    }

    /// Sets the read/write timeout duration.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Transport for SerialTransport {
    fn connect(&mut self) -> Result<(), DriverError> {
        let mut p = serialport::new(&self.port_name, self.baud_rate)
            .timeout(self.timeout)
            .open()
            .map_err(|source| DriverError::Serial {
                port: self.port_name.clone(),
                source,
            })?;

        // Assert hardware flow lines required by Uniden USB-serial bridges (drivers/bcd436hp.py:58-59)
        let _ = p.write_request_to_send(true); // Ignore if virtual port does not support RTS
        let _ = p.write_data_terminal_ready(true); // Ignore if virtual port does not support DTR
        let _ = p.clear(ClearBuffer::Input);

        self.port = Some(p);
        Ok(())
    }

    fn disconnect(&mut self) {
        self.port = None;
    }

    fn is_connected(&self) -> bool {
        self.port.is_some()
    }

    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError> {
        let port = self.port.as_mut().ok_or(DriverError::NotConnected)?;

        let _ = port.clear(ClearBuffer::Input);

        let mut payload = Vec::with_capacity(cmd.len() + 1);
        payload.extend_from_slice(cmd.as_bytes());
        payload.push(b'\r');

        if let Err(e) = port.write_all(&payload) {
            self.port = None;
            return Err(DriverError::Io {
                endpoint: self.port_name.clone(),
                source: e,
            });
        }

        // Wait 40ms for pipelined multi-command response to begin arriving (drivers/bcd996p2.py:109)
        thread::sleep(Duration::from_millis(40));

        let mut collected = Vec::with_capacity(512);
        loop {
            match port.bytes_to_read() {
                Ok(waiting) if waiting > 0 => {
                    let mut buf = vec![0u8; waiting as usize];
                    match port.read(&mut buf) {
                        Ok(n) if n > 0 => {
                            collected.extend_from_slice(&buf[..n]);
                            thread::sleep(Duration::from_millis(20));
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }

        // Fallback: if nothing was in `bytes_to_read()`, read until `\r` or timeout
        if collected.is_empty() {
            let mut byte = [0u8; 1];
            while let Ok(1) = port.read(&mut byte) {
                collected.push(byte[0]);
                if byte[0] == b'\r' {
                    break;
                }
            }
        }

        let s = String::from_utf8_lossy(&collected).trim().to_string();
        Ok(s)
    }
}

/// TCP socket transport for IP serial bridges (`ConnectionMode::Ip`).
pub struct TcpTransport {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
}

impl TcpTransport {
    #[must_use]
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            stream: None,
        }
    }
}

impl Transport for TcpTransport {
    fn connect(&mut self) -> Result<(), DriverError> {
        let endpoint = format!("{}:{}", self.host, self.port);
        let addr: SocketAddr = endpoint
            .to_socket_addrs()
            .map_err(|source| DriverError::Io {
                endpoint: endpoint.clone(),
                source,
            })?
            .next()
            .ok_or_else(|| DriverError::Io {
                endpoint: endpoint.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "no socket address resolved",
                ),
            })?;

        let stream =
            TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|source| {
                DriverError::Io {
                    endpoint: endpoint.clone(),
                    source,
                }
            })?;
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
        self.stream = Some(stream);
        Ok(())
    }

    fn disconnect(&mut self) {
        self.stream = None;
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError> {
        let endpoint = format!("{}:{}", self.host, self.port);
        let stream = self.stream.as_mut().ok_or(DriverError::NotConnected)?;

        let mut payload = Vec::with_capacity(cmd.len() + 1);
        payload.extend_from_slice(cmd.as_bytes());
        payload.push(b'\r');

        if let Err(source) = stream.write_all(&payload).and_then(|()| stream.flush()) {
            self.stream = None;
            return Err(DriverError::Io { endpoint, source });
        }

        let cloned = stream.try_clone().map_err(|source| DriverError::Io {
            endpoint: endpoint.clone(),
            source,
        })?;
        let mut reader = BufReader::new(cloned);
        let mut buf = Vec::with_capacity(512);
        if let Err(source) = reader.read_until(b'\r', &mut buf) {
            self.stream = None;
            return Err(DriverError::Io { endpoint, source });
        }

        Ok(String::from_utf8_lossy(&buf).trim().to_string())
    }
}

/// UDP datagram transport for Uniden LAN scanners (SDS200, BCD536HP, default UDP port 50536).
pub struct UdpTransport {
    host: String,
    port: u16,
    socket: Option<UdpSocket>,
}

impl UdpTransport {
    #[must_use]
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            socket: None,
        }
    }
}

impl Transport for UdpTransport {
    fn connect(&mut self) -> Result<(), DriverError> {
        let endpoint = format!("{}:{}", self.host, self.port);
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(|source| DriverError::Io {
            endpoint: endpoint.clone(),
            source,
        })?;
        sock.connect(&endpoint).map_err(|source| DriverError::Io {
            endpoint: endpoint.clone(),
            source,
        })?;
        let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
        let _ = sock.set_write_timeout(Some(Duration::from_millis(500)));
        self.socket = Some(sock);
        Ok(())
    }

    fn disconnect(&mut self) {
        self.socket = None;
    }

    fn is_connected(&self) -> bool {
        self.socket.is_some()
    }

    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError> {
        let endpoint = format!("{}:{}", self.host, self.port);
        let sock = self.socket.as_mut().ok_or(DriverError::NotConnected)?;

        let mut payload = Vec::with_capacity(cmd.len() + 1);
        payload.extend_from_slice(cmd.as_bytes());
        payload.push(b'\r');

        sock.send(&payload).map_err(|source| DriverError::Io {
            endpoint: endpoint.clone(),
            source,
        })?;

        let mut buf = [0u8; 4096];
        let n = sock
            .recv(&mut buf)
            .map_err(|source| DriverError::Io { endpoint, source })?;

        Ok(String::from_utf8_lossy(&buf[..n]).trim().to_string())
    }
}

/// Deterministic in-memory replay transport for testing protocol codecs and command sequences
/// against Phase 0 golden fixtures without touching hardware.
#[derive(Debug, Default, Clone)]
pub struct ReplayTransport {
    connected: bool,
    fail_connect: bool,
    responses: VecDeque<String>,
    sent_commands: Vec<String>,
}

impl ReplayTransport {
    /// Creates a new connected `ReplayTransport` pre-loaded with queued response transcripts.
    #[must_use]
    pub fn from_responses<I, S>(responses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            connected: true,
            fail_connect: false,
            responses: responses.into_iter().map(Into::into).collect(),
            sent_commands: Vec::new(),
        }
    }

    /// Creates a disconnected `ReplayTransport` that will fail `connect()` if `fail_connect` is true.
    #[must_use]
    pub fn new_disconnected(fail_connect: bool) -> Self {
        Self {
            connected: false,
            fail_connect,
            responses: VecDeque::new(),
            sent_commands: Vec::new(),
        }
    }

    /// Queues an additional response string to be returned on a subsequent `send_command` call.
    pub fn push_response(&mut self, response: impl Into<String>) {
        self.responses.push_back(response.into());
    }

    /// Returns the slice of all commands sent through this transport in order.
    #[must_use]
    pub fn sent_commands(&self) -> &[String] {
        &self.sent_commands
    }
}

impl Transport for ReplayTransport {
    fn connect(&mut self) -> Result<(), DriverError> {
        if self.fail_connect {
            return Err(DriverError::Io {
                endpoint: "replay".to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "simulated connect failure",
                ),
            });
        }
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) {
        self.connected = false;
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError> {
        if !self.connected {
            return Err(DriverError::NotConnected);
        }
        self.sent_commands.push(cmd.to_string());
        Ok(self.responses.pop_front().unwrap_or_default())
    }
}
