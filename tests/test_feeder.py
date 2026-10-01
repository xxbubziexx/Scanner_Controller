import unittest
import os
import sys
import json
import tempfile
import time

# Add project root to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from config import ScanScribeFeederConfig, FeederMode
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload, is_valid_mp3_payload
from metadata.proscan_metadata import ProScanMetadata, write_mp3_id3v23

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

    def test_is_valid_mp3_payload_rejects_pcm_and_accepts_mp3(self):
        """Verifies raw PCM starting with 0xFFFF (sample -1) is NOT falsely flagged as MP3."""
        # 16-bit signed PCM starting with -1 (0xFFFF)
        negative_pcm = b"\xff\xff\x00\x00" * 2000
        self.assertFalse(is_valid_mp3_payload(negative_pcm))

        # Silence PCM
        silence_pcm = b"\x00\x00" * 2000
        self.assertFalse(is_valid_mp3_payload(silence_pcm))

        # Real encoded MP3
        import lameenc
        encoder = lameenc.Encoder()
        encoder.set_bit_rate(64)
        encoder.set_in_sample_rate(16000)
        encoder.set_channels(1)
        real_mp3 = encoder.encode(negative_pcm) + encoder.flush()
        self.assertTrue(is_valid_mp3_payload(real_mp3))

    def test_mp3_generation_with_negative_pcm_samples(self):
        """Verifies feeder properly encodes negative PCM samples to valid MP3 that Mutagen can parse."""
        self.config.audio_format = "MP3"
        self.config.sample_rate = 16000
        negative_pcm = b"\xff\xff\x00\x00\x01\x00\x04\x00" * 4000  # 16000 samples = 1 sec
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD996P2",
            system_name="St. Francois",
            department_name="Central Dispatch",
            channel_name="EMS 1",
            tgid="101",
            frequency=155.1975,
            duration_seconds=1.0,
            audio_pcm=negative_pcm
        )
        res = self.feeder.dispatch_call(payload)
        self.assertEqual(res["status"], "success")
        self.assertTrue(res["audio_file"].endswith(".mp3"))

        # Verify with Mutagen that there is NO "can't sync to MPEG frame" exception!
        import mutagen.mp3
        mp3_obj = mutagen.mp3.MP3(res["audio_file"])
        self.assertIsNotNone(mp3_obj.info)
        self.assertGreater(mp3_obj.info.length, 0.5)
        self.assertEqual(mp3_obj.info.sample_rate, 16000)
        self.assertIn("TIT2", mp3_obj.tags)

    def test_write_mp3_id3v23_strips_existing_tag(self):
        """Verifies write_mp3_id3v23 does not double-wrap or corrupt already-tagged MP3 files."""
        meta1 = ProScanMetadata(channel_name="Original Tag")
        import lameenc
        encoder = lameenc.Encoder()
        encoder.set_bit_rate(64)
        encoder.set_in_sample_rate(16000)
        encoder.set_channels(1)
        mp3_raw = encoder.encode(b"\x00\x00" * 8000) + encoder.flush()

        tagged_once = write_mp3_id3v23(mp3_raw, meta1, title_template="%C")
        meta2 = ProScanMetadata(channel_name="Replacement Tag")
        tagged_twice = write_mp3_id3v23(tagged_once, meta2, title_template="%C")

        # Verify only a single ID3 header exists at byte 0
        self.assertTrue(tagged_twice.startswith(b"ID3"))
        second_id3_idx = tagged_twice[3:].find(b"ID3")
        self.assertEqual(second_id3_idx, -1)

        import io
        import mutagen.mp3
        m = mutagen.mp3.MP3(io.BytesIO(tagged_twice))
        self.assertEqual(str(m.tags["TIT2"]), "Replacement Tag")

if __name__ == "__main__":
    unittest.main()

