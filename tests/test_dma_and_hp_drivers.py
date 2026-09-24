import unittest
import asyncio
from unittest.mock import AsyncMock, patch
from config import ScannerConfig, ScannerModel, ConnectionMode
from drivers.bcd996p2 import BCD996P2Driver
from drivers.bcd436hp import BCD436HPDriver

SAMPLE_DMA_CONVENTIONAL_RECV = (
    "MDL,BCD996P2\r"
    "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
    "GLG,155.7975,NFM,,0,St. Francois County,Sheriff Dispatch,Law 1 Dispatch,1,,,,,0\r"
    "VOL,12\r"
    "SQL,2\r"
    "PWR,4,15579750\r"
)

SAMPLE_DMA_TRUNK_RECV = (
    "MDL,BCD996P2\r"
    "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
    "GLG,ID:10401,NFM,,0,MOSWIN,St. Francois County,County Fire Main,1,,,,,0\r"
    "VOL,12\r"
    "SQL,2\r"
    "PWR,5,85350000\r"
)

SAMPLE_DMA_SCANNING = (
    "MDL,BCD996P2\r"
    "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
    "GLG,,,,0,Scanning...,,,0,,,,,0\r"
    "VOL,12\r"
    "SQL,2\r"
    "PWR,0,0\r"
)

SAMPLE_HP_PIPELINED_RECV = (
    "MDL,BCD436HP\r"
    "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
    "GLG,155.7975,NFM,,0,St. Francois County,Sheriff,Law 1,0,1,0,0,0,0\r"
    "VOL,10\r"
    "SQL,2\r"
    "PWR,4,155.7975\r"
    "GSI,<XML>,<ScannerInfo>"
    "<Department Name=\"Sheriff Dispatch\"/>"
    "<ConvFrequency Name=\"Law 1 Dispatch\" Freq=\"155.7975\" TGID=\"\"/>"
    "<Property P25Status=\"None\" Mute=\"Unmute\" RecSlot=\"\" U_Id=\"\" Rssi=\"4\"/>"
    "</ScannerInfo>\r"
)

SAMPLE_HP_ENCRYPTED_RECV = (
    "MDL,SDS100\r"
    "GLG,853.5000,NFM,,0,MOSWIN,State Highway Patrol,Troop C Tac,0,0,0,0,0,0\r"
    "PWR,5,853.5000\r"
    "GSI,<XML>,<ScannerInfo>"
    "<Department Name=\"Troop C Tactical\"/>"
    "<ConvFrequency Name=\"Troop C Enc Tac\" Freq=\"853.5000\" TGID=\"51022\"/>"
    "<Property P25Status=\"Enc\" Mute=\"Mute\" RecSlot=\"/1\" U_Id=\"720101\" Rssi=\"5\"/>"
    "</ScannerInfo>\r"
)

class TestDmaAndHpDrivers(unittest.TestCase):

    def test_dma_conventional_reception_parse(self):
        cfg = ScannerConfig(id="scanner_b", name="DMA Radio", model=ScannerModel.BCD996P2, port="COM6")
        driver = BCD996P2Driver(cfg)
        driver.status.connected = True

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_DMA_CONVENTIONAL_RECV
            status = asyncio.run(driver.poll())

            self.assertEqual(status.model, "BCD996P2")
            self.assertEqual(status.system_name, "St. Francois County")
            self.assertEqual(status.dept_name, "Sheriff Dispatch")
            self.assertEqual(status.channel_name, "Law 1 Dispatch")
            self.assertEqual(status.frequency, 155.7975)
            self.assertIsNone(status.tgid)
            self.assertTrue(status.receiving)
            self.assertEqual(status.rssi, 4)
            self.assertIn("RECV: Law 1 Dispatch", status.status_line)

    def test_dma_trunk_reception_parse(self):
        cfg = ScannerConfig(id="scanner_b", name="DMA Radio", model=ScannerModel.BCD996P2, port="COM6")
        driver = BCD996P2Driver(cfg)
        driver.status.connected = True

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_DMA_TRUNK_RECV
            status = asyncio.run(driver.poll())

            self.assertEqual(status.system_name, "MOSWIN")
            self.assertEqual(status.dept_name, "St. Francois County")
            self.assertEqual(status.channel_name, "County Fire Main")
            self.assertEqual(status.tgid, "10401")
            self.assertTrue(status.receiving)
            self.assertEqual(status.rssi, 5)

    def test_dma_scanning_state_parse(self):
        cfg = ScannerConfig(id="scanner_b", name="DMA Radio", model=ScannerModel.BCD996P2, port="COM6")
        driver = BCD996P2Driver(cfg)
        driver.status.connected = True

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_DMA_SCANNING
            status = asyncio.run(driver.poll())

            self.assertEqual(status.channel_name, "Scanning...")
            self.assertFalse(status.receiving)
            self.assertEqual(status.status_line, "SCANNING")

    def test_hp_pipelined_reception_parse(self):
        cfg = ScannerConfig(id="scanner_a", name="HP Radio", model=ScannerModel.BCD436HP, port="COM5")
        driver = BCD436HPDriver(cfg)
        driver.status.connected = True

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_HP_PIPELINED_RECV
            status = asyncio.run(driver.poll())

            self.assertEqual(status.model, "BCD436HP")
            self.assertEqual(status.system_name, "St. Francois County")
            self.assertEqual(status.dept_name, "Sheriff Dispatch")
            self.assertEqual(status.channel_name, "Law 1 Dispatch")
            self.assertEqual(status.frequency, 155.7975)
            self.assertTrue(status.receiving)
            self.assertEqual(status.rssi, 4)

    def test_hp_encrypted_reception_triggers_activity(self):
        cfg = ScannerConfig(id="scanner_a", name="HP Radio", model=ScannerModel.SDS100, port="COM5")
        driver = BCD436HPDriver(cfg)
        driver.status.connected = True

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = SAMPLE_HP_ENCRYPTED_RECV
            status = asyncio.run(driver.poll())

            self.assertEqual(status.model, "SDS100")
            self.assertEqual(status.system_name, "MOSWIN")
            self.assertEqual(status.dept_name, "Troop C Tactical")
            self.assertEqual(status.channel_name, "Troop C Enc Tac")
            self.assertEqual(status.tgid, "51022")
            # In ProScan, DigitalStatus == "Enc" forces Activity = true
            self.assertTrue(status.receiving)

    def test_frequency_normalizer_across_formats(self):
        from drivers.base_driver import normalize_frequency
        # 100Hz 7-digit unit (e.g. BCD996P2 raw PWR response)
        self.assertEqual(normalize_frequency(1557975), 155.7975)
        # 10Hz 8-digit unit
        self.assertEqual(normalize_frequency(15579750), 155.7975)
        # Hz 9-digit unit
        self.assertEqual(normalize_frequency(155797500), 155.7975)
        # 800MHz 100Hz 7-digit
        self.assertEqual(normalize_frequency(8512500), 851.25)
        # 800MHz 10Hz 8-digit
        self.assertEqual(normalize_frequency(85125000), 851.25)
        # Low band 10-meter in 100Hz
        self.assertEqual(normalize_frequency(296000), 29.6)
        # 1.2GHz amateur band in 100Hz
        self.assertEqual(normalize_frequency(12500000), 1250.0)
        # Already normalized float in MHz
        self.assertEqual(normalize_frequency(155.7975), 155.7975)
        self.assertEqual(normalize_frequency(853.5000), 853.5)

    def test_hp_driver_seamlessly_parses_dma_scanner(self):
        """
        Verifies that if a user has a BCD996P2 connected to COM5 while
        configured as BCD436HP, the parser auto-detects DMA format and
        normalizes the frequency and channel names without 'Active Call'.
        """
        cfg = ScannerConfig(id="scanner_a", name="Auto-detected Radio", model=ScannerModel.BCD436HP, port="COM5")
        driver = BCD436HPDriver(cfg)
        driver.status.connected = True

        raw_response = (
            "MDL,BCD996P2\r"
            "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
            "GLG,1557975,NFM,,0,St. Francois,Sheriff,Law 1 Dispatch,1,,,,,0\r"
            "VOL,12\r"
            "SQL,2\r"
            "PWR,272,1557975\r"
        )

        with patch.object(driver, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = raw_response
            status = asyncio.run(driver.poll())

            self.assertEqual(status.model, "BCD996P2")
            self.assertEqual(status.system_name, "St. Francois")
            self.assertEqual(status.dept_name, "Sheriff")
            self.assertEqual(status.channel_name, "Law 1 Dispatch")
            self.assertEqual(status.frequency, 155.7975)
            self.assertTrue(status.receiving)
            self.assertNotEqual(status.channel_name, "Active Call")

    def test_bcd436hp_send_skip_does_not_send_key_s(self):
        """
        Critical regression test: BCD436HP does not have a physical Scan key.
        Sending KEY,S,P on a 436HP activates System Quick Key navigation (opening
        the Quick Key menu). send_skip() must use rotary knob or channel hold cycle
        and must NEVER send KEY,S,P.
        """
        cfg = ScannerConfig(id="scanner_b", name="BCD436HP", model=ScannerModel.BCD436HP, port="COM6")
        driver = BCD436HPDriver(cfg)
        driver.status.connected = True

        sent_commands = []

        async def fake_send_cmd(cmd):
            sent_commands.append(cmd)
            if cmd == "KEY,>,P":
                return "NG"  # Simulate knob failure
            elif cmd == "KEY,>, P":
                return "NG"  # Simulate knob failure with space
            return "OK"

        with patch.object(driver, "_send_command", side_effect=fake_send_cmd):
            result = asyncio.run(driver.send_skip())
            self.assertTrue(result)
            self.assertNotIn("KEY,S,P", sent_commands)
            self.assertIn("KEY,>,P", sent_commands)
            self.assertIn("KEY,Z,P", sent_commands)

if __name__ == "__main__":
    unittest.main()
