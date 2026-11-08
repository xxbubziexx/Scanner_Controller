//! Model-to-driver dispatch factory for all 34 Uniden scanner models.
//!
//! Ported from `drivers/driver_factory.py` and `DMA-VS-HP-ARCHITECTURE.md` §1.4.

use sc_core::{ConnectionMode, ScannerConfig, ScannerModel, ScannerStatus};
use tracing::info;

use crate::classic::ClassicDriver;
use crate::dma::DmaDriver;
use crate::error::DriverError;
use crate::hp::HpDriver;
use crate::transport::{SerialTransport, TcpTransport, Transport, UdpTransport};

/// Protocol driver family classification across the 34 Uniden models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverFamily {
    /// `HomePatrol` / x36HP / SDS family (`BCD436HP`, `BCD536HP`, `SDS100`, `SDS200`, etc.)
    HomePatrolSds,
    /// XT / P2 / DMA family (`BCD996P2`, `BCD325P2`, `BCD996XT`, `BCD396XT`, `BCT15X`, etc.)
    XtDma,
    /// Classic Bearcat / DN family (`BC125AT`, `BCD160DN`, `BCD260DN`, `BC780XLT`, etc.)
    ClassicDn,
}

/// Returns the [`DriverFamily`] appropriate for any of the 34 Uniden [`ScannerModel`] variants.
#[must_use]
pub const fn driver_family_for_model(model: ScannerModel) -> DriverFamily {
    match model {
        ScannerModel::BCD436HP
        | ScannerModel::BCD536HP
        | ScannerModel::SDS100
        | ScannerModel::SDS200
        | ScannerModel::SDS150
        | ScannerModel::SDS100E
        | ScannerModel::SDS200E
        | ScannerModel::USDS100
        | ScannerModel::UBCD3600XLT
        | ScannerModel::UBCD436PT
        | ScannerModel::UBCD536PT => DriverFamily::HomePatrolSds,

        ScannerModel::BCD996P2
        | ScannerModel::BCD325P2
        | ScannerModel::BCD996XT
        | ScannerModel::BCD396XT
        | ScannerModel::BCT15X
        | ScannerModel::BCT15
        | ScannerModel::BC346XT
        | ScannerModel::BC346XTC
        | ScannerModel::BR330T
        | ScannerModel::BCD396T
        | ScannerModel::BCD996T => DriverFamily::XtDma,

        ScannerModel::BC125AT
        | ScannerModel::BCD160DN
        | ScannerModel::BCD260DN
        | ScannerModel::UBCD160DN
        | ScannerModel::UBCD260DN
        | ScannerModel::UBC125XLT
        | ScannerModel::UBC126AT
        | ScannerModel::BC250D
        | ScannerModel::BC296D
        | ScannerModel::BC780XLT
        | ScannerModel::BC785D
        | ScannerModel::BC796D => DriverFamily::ClassicDn,
    }
}

/// Polymorphic enum wrapper dispatching across all three scanner driver families.
pub enum AnyScannerDriver<T: Transport> {
    HomePatrol(HpDriver<T>),
    Dma(DmaDriver<T>),
    Classic(ClassicDriver<T>),
}

impl<T: Transport> AnyScannerDriver<T> {
    /// Returns the [`DriverFamily`] of this driver instance.
    #[must_use]
    pub const fn family(&self) -> DriverFamily {
        match self {
            Self::HomePatrol(_) => DriverFamily::HomePatrolSds,
            Self::Dma(_) => DriverFamily::XtDma,
            Self::Classic(_) => DriverFamily::ClassicDn,
        }
    }

    #[must_use]
    pub fn status(&self) -> &ScannerStatus {
        match self {
            Self::HomePatrol(d) => d.status(),
            Self::Dma(d) => d.status(),
            Self::Classic(d) => d.status(),
        }
    }

    pub fn status_mut(&mut self) -> &mut ScannerStatus {
        match self {
            Self::HomePatrol(d) => d.status_mut(),
            Self::Dma(d) => d.status_mut(),
            Self::Classic(d) => d.status_mut(),
        }
    }

    #[must_use]
    pub fn config(&self) -> &ScannerConfig {
        match self {
            Self::HomePatrol(d) => d.config(),
            Self::Dma(d) => d.config(),
            Self::Classic(d) => d.config(),
        }
    }

    #[must_use]
    pub fn transport(&self) -> &T {
        match self {
            Self::HomePatrol(d) => d.transport(),
            Self::Dma(d) => d.transport(),
            Self::Classic(d) => d.transport(),
        }
    }

    pub fn transport_mut(&mut self) -> &mut T {
        match self {
            Self::HomePatrol(d) => d.transport_mut(),
            Self::Dma(d) => d.transport_mut(),
            Self::Classic(d) => d.transport_mut(),
        }
    }

    pub fn connect(&mut self) -> bool {
        match self {
            Self::HomePatrol(d) => d.connect(),
            Self::Dma(d) => d.connect(),
            Self::Classic(d) => d.connect(),
        }
    }

    pub fn disconnect(&mut self) {
        match self {
            Self::HomePatrol(d) => d.disconnect(),
            Self::Dma(d) => d.disconnect(),
            Self::Classic(d) => d.disconnect(),
        }
    }

    pub fn poll(&mut self) -> &ScannerStatus {
        match self {
            Self::HomePatrol(d) => d.poll(),
            Self::Dma(d) => d.poll(),
            Self::Classic(d) => d.poll(),
        }
    }

    pub fn send_skip(&mut self) -> bool {
        match self {
            Self::HomePatrol(d) => d.send_skip(),
            Self::Dma(d) => d.send_skip(),
            Self::Classic(d) => d.send_skip(),
        }
    }

    pub fn temporary_lockout(&mut self, tgid_or_freq: &str) -> bool {
        match self {
            Self::HomePatrol(d) => d.temporary_lockout(tgid_or_freq),
            Self::Dma(d) => d.temporary_lockout(tgid_or_freq),
            Self::Classic(d) => d.temporary_lockout(tgid_or_freq),
        }
    }

    pub fn release_lockout(&mut self, tgid_or_freq: &str) -> bool {
        match self {
            Self::HomePatrol(d) => d.release_lockout(tgid_or_freq),
            Self::Dma(d) => d.release_lockout(tgid_or_freq),
            Self::Classic(d) => d.release_lockout(tgid_or_freq),
        }
    }
}

/// Factory function instantiating the appropriate driver for any Uniden model with a custom [`Transport`].
pub fn create_driver_for_model<T: Transport>(
    config: ScannerConfig,
    transport: T,
) -> AnyScannerDriver<T> {
    let family = driver_family_for_model(config.model);
    info!(
        scanner_id = %config.id,
        name = %config.name,
        model = %config.model.as_str(),
        ?family,
        "Instantiating scanner driver"
    );
    match family {
        DriverFamily::HomePatrolSds => {
            AnyScannerDriver::HomePatrol(HpDriver::new(config, transport))
        }
        DriverFamily::XtDma => AnyScannerDriver::Dma(DmaDriver::new(config, transport)),
        DriverFamily::ClassicDn => AnyScannerDriver::Classic(ClassicDriver::new(config, transport)),
    }
}

/// Concrete hardware transport enum combining Serial, TCP, and UDP transports.
pub enum HardwareTransport {
    Serial(SerialTransport),
    Tcp(TcpTransport),
    Udp(UdpTransport),
}

impl Transport for HardwareTransport {
    fn connect(&mut self) -> Result<(), DriverError> {
        match self {
            Self::Serial(t) => t.connect(),
            Self::Tcp(t) => t.connect(),
            Self::Udp(t) => t.connect(),
        }
    }

    fn disconnect(&mut self) {
        match self {
            Self::Serial(t) => t.disconnect(),
            Self::Tcp(t) => t.disconnect(),
            Self::Udp(t) => t.disconnect(),
        }
    }

    fn is_connected(&self) -> bool {
        match self {
            Self::Serial(t) => t.is_connected(),
            Self::Tcp(t) => t.is_connected(),
            Self::Udp(t) => t.is_connected(),
        }
    }

    fn send_command(&mut self, cmd: &str) -> Result<String, DriverError> {
        match self {
            Self::Serial(t) => t.send_command(cmd),
            Self::Tcp(t) => t.send_command(cmd),
            Self::Udp(t) => t.send_command(cmd),
        }
    }
}

/// Instantiates a live hardware driver (`SerialTransport` or `TcpTransport`) from a [`ScannerConfig`].
#[must_use]
pub fn create_hardware_driver(config: ScannerConfig) -> AnyScannerDriver<HardwareTransport> {
    let transport = match config.connection_mode {
        ConnectionMode::Serial => {
            HardwareTransport::Serial(SerialTransport::new(&config.port, config.baud_rate))
        }
        ConnectionMode::Ip => {
            HardwareTransport::Tcp(TcpTransport::new(&config.ip_host, config.ip_port))
        }
    };
    create_driver_for_model(config, transport)
}
