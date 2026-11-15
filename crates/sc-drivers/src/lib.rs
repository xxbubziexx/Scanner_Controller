//! `sc-drivers`: Serial & network transports and Uniden DMA, `HomePatrol`/SDS, and Classic protocol drivers.

#![deny(unsafe_code)]

pub mod classic;
pub mod dma;
pub mod error;
pub mod factory;
pub mod hp;
pub mod parse_util;
pub mod transport;

pub use classic::{parse_classic_response, ClassicDriver};
pub use dma::{parse_dma_response, DmaDriver, DMA_POLL_COMMAND};
pub use error::DriverError;
pub use factory::{
    create_driver_for_model, create_hardware_driver, driver_family_for_model, AnyScannerDriver,
    DriverFamily, HardwareTransport,
};
pub use hp::{
    clean_display_text, parse_hp_response, parse_sts_fallback, HpDriver, HP_POLL_COMMAND,
};
pub use parse_util::extract_section;
pub use transport::{
    normalize_port_name, ReplayTransport, SerialTransport, TcpTransport, Transport, UdpTransport,
};

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use sc_core::{AppConfig, ConnectionMode, ScannerConfig, ScannerModel, ScannerStatus};

    fn get_fixtures_dir() -> std::path::PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        std::path::Path::new(manifest_dir)
            .join("..")
            .join("..")
            .join("fixtures")
            .join("drivers")
    }

    fn make_config(id: &str, model: ScannerModel, port: &str) -> ScannerConfig {
        let mut cfg = AppConfig::default().scanner_a;
        cfg.id = id.to_string();
        cfg.name = format!("Test {}", model.as_str());
        cfg.model = model;
        cfg.connection_mode = ConnectionMode::Serial;
        cfg.port = port.to_string();
        cfg
    }

    #[test]
    fn test_all_5_driver_fixtures_parity() {
        let drv_dir = get_fixtures_dir();
        let cases = [
            ("dma_conventional", "scanner_b", ScannerModel::BCD996P2),
            ("dma_trunk", "scanner_b", ScannerModel::BCD996P2),
            ("dma_scanning", "scanner_b", ScannerModel::BCD996P2),
            ("hp_pipelined", "scanner_a", ScannerModel::BCD436HP),
            ("hp_encrypted", "scanner_a", ScannerModel::SDS100),
        ];

        for (name, scanner_id, model) in cases {
            let txt_path = drv_dir.join(format!("{name}.txt"));
            let json_path = drv_dir.join(format!("{name}_expected.json"));

            let raw_transcript =
                std::fs::read_to_string(&txt_path).expect("failed to read driver transcript");
            let expected_json =
                std::fs::read_to_string(&json_path).expect("failed to read expected json");
            let expected: ScannerStatus =
                serde_json::from_str(&expected_json).expect("failed to parse expected status");

            let cfg = make_config(scanner_id, model, "COM5");
            let replay = ReplayTransport::from_responses([raw_transcript]);
            let mut driver = create_driver_for_model(cfg, replay);

            let actual = driver.poll().clone();

            assert_eq!(actual.scanner_id, expected.scanner_id, "case {name}");
            assert_eq!(actual.model, expected.model, "case {name}");
            assert_eq!(actual.connected, expected.connected, "case {name}");
            assert_eq!(actual.receiving, expected.receiving, "case {name}");
            assert_eq!(actual.system_name, expected.system_name, "case {name}");
            assert_eq!(actual.dept_name, expected.dept_name, "case {name}");
            assert_eq!(actual.channel_name, expected.channel_name, "case {name}");
            assert_eq!(actual.frequency, expected.frequency, "case {name}");
            assert_eq!(actual.tgid, expected.tgid, "case {name}");
            assert_eq!(actual.modulation, expected.modulation, "case {name}");
            assert_eq!(actual.rssi, expected.rssi, "case {name}");
            assert_eq!(actual.status_line, expected.status_line, "case {name}");
        }
    }

    #[test]
    fn test_all_34_models_driver_factory_mapping() {
        for model in ScannerModel::ALL {
            let cfg = make_config("test_scanner", model, "COM1");
            let replay = ReplayTransport::from_responses(Vec::<String>::new());
            let driver = create_driver_for_model(cfg, replay);
            assert_eq!(driver.config().model, model);
        }

        assert_eq!(
            driver_family_for_model(ScannerModel::SDS200),
            DriverFamily::HomePatrolSds
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::SDS100),
            DriverFamily::HomePatrolSds
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::BCD436HP),
            DriverFamily::HomePatrolSds
        );

        assert_eq!(
            driver_family_for_model(ScannerModel::BCD996P2),
            DriverFamily::XtDma
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::BCD325P2),
            DriverFamily::XtDma
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::BCD996XT),
            DriverFamily::XtDma
        );

        assert_eq!(
            driver_family_for_model(ScannerModel::BC125AT),
            DriverFamily::ClassicDn
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::BCD160DN),
            DriverFamily::ClassicDn
        );
        assert_eq!(
            driver_family_for_model(ScannerModel::BC780XLT),
            DriverFamily::ClassicDn
        );
    }

    #[test]
    fn test_hp_sts_screen_fallback_parsing() {
        let sample_sts_hold = concat!(
            "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,",
            "________________________,Medical Helicopters /   ,,EMS Agencies (Area Wide),,Statewide               ,",
            "________________________,Air Methods - ARCH      ,,Helicopter              ,,                        ,",
            "________________________,ARCH Dispatch - St Louis,,                        ,,  461.5250MHz\x0f\x10\x0e\x0bC192.8 ,",
            ",EMS Dispatch        ,________________________,                      ,,Tag:--.--.---         ,,0,1,0,0,,,0,OFF,0"
        );

        let cfg = make_config("scanner_a", ScannerModel::BCD436HP, "COM5");
        let replay = ReplayTransport::from_responses([sample_sts_hold]);
        let mut driver = HpDriver::new(cfg, replay);
        let status = driver.poll().clone();

        assert_eq!(status.channel_name, "ARCH Dispatch - St Louis");
        assert_eq!(status.dept_name, "Air Methods - ARCH");
        assert_eq!(
            status.system_name,
            "Medical Helicopters / EMS Agencies (Area Wide)"
        );
        assert_eq!(status.frequency, Some(461.525));
        assert!(status.status_line.contains("HOLD"));
        assert_eq!(status.rssi, 1);

        let sample_sts_scanning = concat!(
            "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,",
            "________________________,Missouri Statewide     ,,Emergency Services     ,,Statewide               ,",
            "________________________,Department 1           ,,                        ,,                        ,",
            "________________________,Scanning...             ,,                        ,,                        ,",
            ",                    ,________________________,                      ,,Tag:--.--.---         ,,0,0,0,0,,,0,OFF,0"
        );

        let cfg2 = make_config("scanner_a", ScannerModel::BCD436HP, "COM5");
        let replay2 = ReplayTransport::from_responses([sample_sts_scanning]);
        let mut driver2 = HpDriver::new(cfg2, replay2);
        let status2 = driver2.poll().clone();

        assert_eq!(status2.channel_name, "Scanning...");
        assert_eq!(status2.status_line, "SCANNING");
        assert!(!status2.receiving);
        assert_eq!(status2.frequency, None);
    }

    #[test]
    fn test_hp_driver_seamlessly_parses_dma_scanner() {
        let raw_response = concat!(
            "MDL,BCD996P2\r",
            "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r",
            "GLG,1557975,NFM,,0,St. Francois,Sheriff,Law 1 Dispatch,1,,,,,0\r",
            "VOL,12\r",
            "SQL,2\r",
            "PWR,272,1557975\r"
        );

        let cfg = make_config("scanner_a", ScannerModel::BCD436HP, "COM5");
        let replay = ReplayTransport::from_responses([raw_response]);
        let mut driver = HpDriver::new(cfg, replay);
        let status = driver.poll().clone();

        assert_eq!(status.model, "BCD996P2");
        assert_eq!(status.system_name, "St. Francois");
        assert_eq!(status.dept_name, "Sheriff");
        assert_eq!(status.channel_name, "Law 1 Dispatch");
        assert_eq!(status.frequency, Some(155.7975));
        assert!(status.receiving);
        assert_eq!(status.rssi, 272);
    }

    #[test]
    fn test_bcd436hp_send_skip_never_sends_key_s() {
        let cfg = make_config("scanner_b", ScannerModel::BCD436HP, "COM6");
        // Simulate knob step failing ("NG", "NG"), then Channel Hold cycle succeeding ("OK", "OK")
        let replay = ReplayTransport::from_responses(["NG", "NG", "OK", "OK"]);
        let mut driver = HpDriver::new(cfg, replay);

        assert!(driver.send_skip());
        let sent = driver.transport().sent_commands();
        assert!(!sent.contains(&"KEY,S,P".to_string()));
        assert_eq!(sent, &["KEY,>,P", "KEY,>, P", "KEY,Z,P", "KEY,Z,P"]);
    }
}
