import unittest
import asyncio
from unittest.mock import AsyncMock, patch
from config import ScannerConfig, ScannerModel, ConnectionMode
from drivers.bcd436hp import BCD436HPDriver

SAMPLE_STS_HOLD = (
    "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,"
    "________________________,Medical Helicopters /   ,,EMS Agencies (Area Wide),,Statewide               ,"
    "________________________,Air Methods - ARCH      ,,Helicopter              ,,                        ,"
    "________________________,ARCH Dispatch - St Louis,,                        ,,  461.5250MHz\x0f\x10\x0e\x0bC192.8 ,"
    ",EMS Dispatch        ,________________________,                      ,,Tag:--.--.---         ,,0,1,0,0,,,0,OFF,0"
)

SAMPLE_STS_SCANNING = (
    "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,"
    "________________________,Missouri Statewide     ,,Emergency Services     ,,Statewide               ,"
    "________________________,Department 1           ,,                        ,,                        ,"
    "________________________,Scanning...             ,,                        ,,                        ,"
    ",                    ,________________________,                      ,,Tag:--.--.---         ,,0,0,0,0,,,0,OFF,0"
)

class TestBCD436HPParsing(unittest.TestCase):
    def setUp(self):
        self.config = ScannerConfig(
            id="scanner_a",
            name="Primary Scanner",
            model=ScannerModel.BCD436HP,
            connection_mode=ConnectionMode.SERIAL,
            port="COM5",
            baud_rate=115200
        )
        self.driver = BCD436HPDriver(self.config)
        self.driver.status.connected = True

    def test_hold_reception_parse(self):
        with patch.object(self.driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_STS_HOLD
            status = asyncio.run(self.driver.poll())

            self.assertEqual(status.channel_name, "ARCH Dispatch - St Louis")
            self.assertEqual(status.dept_name, "Air Methods - ARCH")
            self.assertEqual(status.system_name, "Medical Helicopters / EMS Agencies (Area Wide)")
            self.assertEqual(status.frequency, 461.525)
            self.assertIn("HOLD", status.status_line)
            self.assertEqual(status.rssi, 1)

    def test_scanning_state_parse(self):
        with patch.object(self.driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_STS_SCANNING
            status = asyncio.run(self.driver.poll())

            self.assertEqual(status.channel_name, "Scanning...")
            self.assertEqual(status.status_line, "SCANNING")
            self.assertFalse(status.receiving)
            self.assertIsNone(status.frequency)

if __name__ == "__main__":
    unittest.main()
