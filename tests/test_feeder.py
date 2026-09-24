import unittest
import os
import sys
import json
import tempfile
import time

# Add project root to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from config import ScanScribeFeederConfig, FeederMode
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload

class TestScanScribeFeeder(unittest.TestCase):

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()
        self.config = ScanScribeFeederConfig(
            enable_feeder=True,
            feeder_mode=FeederMode.DIRECTORY_DROP,
            inbox_directory=self.temp_dir,
            audio_format="MP3",
            enable_sidecar_json=True
        )
        self.feeder = ScanScribeFeeder(self.config)

    def test_directory_drop_feeder(self):
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="Metro Trunking",
            department_name="Fire Dispatch",
            channel_name="Fire Main",
            tgid="20101",
            frequency=852.1000,
            rssi=5,
            duration_seconds=6.8,
            timestamp=time.time()
        )

        res = self.feeder.dispatch_call(payload)
        self.assertEqual(res["status"], "success")
        self.assertEqual(res["mode"], "DIRECTORY_DROP")

        audio_file = res["audio_file"]
        sidecar_file = res["sidecar_file"]

        self.assertTrue(os.path.exists(audio_file))
        self.assertTrue(os.path.exists(sidecar_file))

        # Verify sidecar JSON content
        with open(sidecar_file, "r", encoding="utf-8") as f:
            sidecar_data = json.load(f)

        self.assertEqual(sidecar_data["scanner_id"], "scanner_a")
        self.assertEqual(sidecar_data["scanner_model"], "BCD436HP")
        self.assertEqual(sidecar_data["system_name"], "Metro Trunking")
        self.assertEqual(sidecar_data["tgid"], "20101")
        self.assertEqual(sidecar_data["frequency"], 852.1000)
        self.assertEqual(sidecar_data["duration_seconds"], 6.8)

        # Verify ProScan metadata in sidecar
        proscan_meta = sidecar_data["proscan_metadata"]
        self.assertEqual(proscan_meta["Scanner"], "BCD436HP")
        self.assertEqual(proscan_meta["TGID"], "20101")
        self.assertEqual(proscan_meta["ChannelName"], "Fire Main")

    def test_wav_directory_drop(self):
        self.config.audio_format = "WAV"
        payload = CallTransmissionPayload(
            scanner_id="scanner_b",
            scanner_model="SDS200",
            system_name="County P25",
            channel_name="Sheriff Tac 1",
            tgid="40012",
            frequency=460.1250,
            duration_seconds=3.5
        )

        res = self.feeder.dispatch_call(payload)
        self.assertEqual(res["status"], "success")
        self.assertTrue(res["audio_file"].endswith(".wav"))
        self.assertTrue(os.path.exists(res["audio_file"]))

    def test_drive_root_resolution(self):
        # Testing bare drive root handling
        self.config.inbox_directory = "A:\\"
        resolved = self.feeder._resolve_inbox_directory()
        self.assertFalse(resolved in ["A:\\", "A:/", "A:"])
        self.assertTrue("inbox" in resolved or "recordings" in resolved)

    def test_filename_sanitization(self):
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="State Police / Emergency",
            department_name="Dispatch: Main",
            channel_name="Law 1: Dispatch 1",
            tgid="TG:10401/A",
            frequency=155.7975,
            duration_seconds=5.0
        )
        res = self.feeder.dispatch_call(payload)
        self.assertEqual(res["status"], "success")
        audio_file = res["audio_file"]
        # Filename should not have raw colon or slashes in basename
        base = os.path.basename(audio_file)
        self.assertNotIn(":", base)
        self.assertNotIn("/", base)
        self.assertNotIn("\\", base)
        self.assertTrue(os.path.exists(audio_file))

    def test_unscaled_frequency_and_active_call_fallback(self):
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD996P2",
            system_name="St. Francois",
            department_name="",
            channel_name="Active Call",
            frequency=1557975.0,
            duration_seconds=10.0
        )
        meta = self.feeder._build_proscan_metadata(payload)
        self.assertEqual(meta.frequency, "155.7975")
        self.assertEqual(meta.channel_name, "155.7975 MHz")
        self.assertEqual(meta.system_name, "St. Francois")

        res = self.feeder.dispatch_call(payload)
        self.assertEqual(res["status"], "success")
        self.assertNotIn("Active Call", res["audio_file"])
        self.assertIn("155.7975 MHz", res["audio_file"])

    def test_tit2_template_custom_format(self):
        self.config.audio_format = "WAV"
        self.config.tit2_template = "%TG - %C"
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="Metro Trunking",
            department_name="Fire",
            channel_name="FIRE-TAC1",
            tgid="1201",
            duration_seconds=3.0
        )
        meta = self.feeder._build_proscan_metadata(payload)
        audio_bytes, ext = self.feeder._generate_audio_bytes(payload, meta)
        self.assertEqual(ext, "wav")
        # In WAV with RIFF INFO chunk, INAM tag gets the formatted title
        self.assertIn(b"INAM", audio_bytes)
        self.assertIn(b"1201 - FIRE-TAC1", audio_bytes)

    def test_per_scanner_filename_and_tit2_templates(self):
        from config import ScannerConfig, ScannerModel
        cfg_a = ScannerConfig(
            id="scanner_a",
            name="Scanner A",
            model=ScannerModel.BCD436HP,
            filename_template="%ST_%C_%TG",
            tit2_template="%C [%TG]"
        )
        cfg_b = ScannerConfig(
            id="scanner_b",
            name="Scanner B",
            model=ScannerModel.BCD996P2,
            filename_template="%S_%DE_%C",
            tit2_template="%S - %C"
        )
        self.feeder.update_scanner_configs({"scanner_a": cfg_a, "scanner_b": cfg_b})
        self.config.audio_format = "WAV"

        # Dispatch call from Scanner A
        payload_a = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="Metro P25",
            department_name="Fire",
            channel_name="Fire Main",
            tgid="101",
            duration_seconds=2.0
        )
        res_a = self.feeder.dispatch_call(payload_a)
        self.assertEqual(res_a["status"], "success")
        base_a = os.path.basename(res_a["audio_file"])
        self.assertTrue(base_a.startswith("BCD436HP_Fire Main_101"))

        with open(res_a["audio_file"], "rb") as f:
            bytes_a = f.read()
        self.assertIn(b"Fire Main [101]", bytes_a)

        # Dispatch call from Scanner B
        payload_b = CallTransmissionPayload(
            scanner_id="scanner_b",
            scanner_model="BCD996P2",
            system_name="County Trunk",
            department_name="Police",
            channel_name="Dispatch North",
            tgid="202",
            duration_seconds=2.0
        )
        res_b = self.feeder.dispatch_call(payload_b)
        self.assertEqual(res_b["status"], "success")
        base_b = os.path.basename(res_b["audio_file"])
        self.assertTrue(base_b.startswith("County Trunk_Police_Dispatch North"))

        with open(res_b["audio_file"], "rb") as f:
            bytes_b = f.read()
        self.assertIn(b"County Trunk - Dispatch North", bytes_b)

    def test_payload_explicit_template_override(self):
        self.config.audio_format = "WAV"
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="Custom Sys",
            channel_name="Custom Chan",
            tgid="999",
            duration_seconds=1.0,
            filename_template="OVERRIDE_%C_%TG",
            tit2_template="OVERRIDE_TITLE_%C"
        )
        res = self.feeder.dispatch_call(payload)
        base = os.path.basename(res["audio_file"])
        self.assertTrue(base.startswith("OVERRIDE_Custom Chan_999"))

        with open(res["audio_file"], "rb") as f:
            audio_bytes = f.read()
        self.assertIn(b"OVERRIDE_TITLE_Custom Chan", audio_bytes)

if __name__ == "__main__":
    unittest.main()

