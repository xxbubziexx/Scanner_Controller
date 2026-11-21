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

    pub fn config_mut(&mut self) -> &mut ScannerConfig {
        match self {
            Self::HomePatrol(d) => d.config_mut(),
            Self::Dma(d) => d.config_mut(),
            Self::Classic(d) => d.config_mut(),
        }
    }

    /// Consumes this driver, returning `(config, status, transport)` while keeping the transport open.
    #[must_use]
    pub fn into_parts(self) -> (ScannerConfig, ScannerStatus, T) {
        match self {
            Self::HomePatrol(d) => d.into_parts(),
            Self::Dma(d) => d.into_parts(),
            Self::Classic(d) => d.into_parts(),
        }
    }

    /// Constructs the driver family matching `model` from existing `(config, status, transport)`.
    #[must_use]
    pub fn from_parts_for_model(
        mut config: ScannerConfig,
        mut status: ScannerStatus,
        transport: T,
        model: ScannerModel,
    ) -> Self {
        config.model = model;
        status.model = model.as_str().to_string();
        match driver_family_for_model(model) {
            DriverFamily::HomePatrolSds => {
                Self::HomePatrol(HpDriver::from_parts(config, status, transport))
            }
            DriverFamily::XtDma => Self::Dma(DmaDriver::from_parts(config, status, transport)),
            DriverFamily::ClassicDn => {
                Self::Classic(ClassicDriver::from_parts(config, status, transport))
            }
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

/// Polls `slot` and automatically updates the scanner model and driver family in-place
/// if the connected hardware reports a different [`ScannerModel`] via `MDL` / `SI`.
///
/// Returns `(updated_status, Some(newly_detected_model))` when the model changed.
pub fn poll_slot_with_autodetect<T: Transport>(
    slot: &mut Option<AnyScannerDriver<T>>,
) -> Option<(ScannerStatus, Option<ScannerModel>)> {
    let (status, detected_opt) = {
        let drv = slot.as_mut()?;
        let st = drv.poll().clone();
        let detected = ScannerModel::from_mdl_str(&st.model).filter(|&m| m != drv.config().model);
        (st, detected)
    };

    let Some(detected) = detected_opt else {
        return Some((status, None));
    };

    let mut updated_status = status;
    updated_status.model = detected.as_str().to_string();
    let current_family = slot.as_ref()?.family();
    let target_family = driver_family_for_model(detected);

    if current_family == target_family {
        if let Some(drv) = slot.as_mut() {
            drv.config_mut().model = detected;
            drv.status_mut().model = detected.as_str().to_string();
            info!(
                scanner_id = %drv.config().id,
                detected_model = %detected.as_str(),
                "Auto-detected scanner model from COM port"
            );
        }
    } else if let Some(old_drv) = slot.take() {
        let (cfg, _old_st, transport) = old_drv.into_parts();
        let scanner_id = cfg.id.clone();
        let upgraded = AnyScannerDriver::from_parts_for_model(
            cfg,
            updated_status.clone(),
            transport,
            detected,
        );
        info!(
            scanner_id = %scanner_id,
            detected_model = %detected.as_str(),
            ?target_family,
            "Auto-detected scanner model from COM port; upgraded driver family in-place"
        );
        *slot = Some(upgraded);
    }

    Some((updated_status, Some(detected)))
}
