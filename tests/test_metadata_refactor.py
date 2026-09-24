import unittest
import struct
import io
import wave
import time
from metadata.proscan_metadata import ProScanMetadata, write_wav_riff_info, parse_pros_chunk, format_template
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload
from config import ScanScribeFeederConfig, FeederMode

class TestMetadataRefactor(unittest.TestCase):

    def test_canonical_wav_chunk_structure(self):
        pcm = b"\x10\x00" * 8000 # 0.5s of 16kHz audio
        meta = ProScanMetadata(
            scanner="BCD436HP",
            system_name="St. Francois County",
            department_name="Sheriff Dispatch",
            channel_name="Law 1 Dispatch",
            frequency="155.7975",
            tgid="10401"
        )
        wav_bytes = write_wav_riff_info(pcm, meta, sample_rate=16000)

        # 1. RIFF WAVE header check
        self.assertEqual(wav_bytes[:4], b"RIFF")
        self.assertEqual(wav_bytes[8:12], b"WAVE")

        # 2. fmt chunk MUST be first at offset 12 for universal player compatibility
        self.assertEqual(wav_bytes[12:16], b"fmt ")
        wFormatTag, nChannels, nSamplesPerSec = struct.unpack("<HHI", wav_bytes[20:28])
        self.assertEqual(wFormatTag, 1) # PCM
        self.assertEqual(nChannels, 1)
        self.assertEqual(nSamplesPerSec, 16000)

        # 3. data chunk MUST be second at offset 36
        self.assertEqual(wav_bytes[36:40], b"data")
        data_len = struct.unpack("<I", wav_bytes[40:44])[0]
        self.assertEqual(data_len, len(pcm))

        # 4. Standard wave module must open and read all frames without errors
        with wave.open(io.BytesIO(wav_bytes), "rb") as w:
            self.assertEqual(w.getframerate(), 16000)
            self.assertEqual(w.getnchannels(), 1)
            frames = w.readframes(w.getnframes())
            self.assertEqual(len(frames), len(pcm))

        # 5. LIST INFO, id3, and pros chunks must all be present
        self.assertIn(b"LIST", wav_bytes)
        self.assertIn(b"INFO", wav_bytes)
        self.assertIn(b"id3 ", wav_bytes)
        self.assertIn(b"pros", wav_bytes)

        # 6. ProScan parser must read pros chunk
        parsed, read_len = parse_pros_chunk(wav_bytes)
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed.channel_name, "Law 1 Dispatch")
        self.assertEqual(parsed.system_name, "St. Francois County")
        self.assertEqual(parsed.tgid, "10401")

    def test_conventional_channel_strictly_no_fallbacks(self):
        # Conventional channel call (no TGID)
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="St. Francois County",
            department_name="Sheriff Dispatch",
            channel_name="Law 1: Dispatch 1",
            frequency=155.7975,
            tgid=None,
            timestamp=time.time(),
            duration_seconds=5.0
        )
        feeder = ScanScribeFeeder(ScanScribeFeederConfig())
        meta = feeder._build_proscan_metadata(payload)

        # Exact channel and system names preserved - no synthetic text
        self.assertEqual(meta.channel_name, "Law 1: Dispatch 1")
        self.assertEqual(meta.system_name, "St. Francois County")

        # TGID must NOT fallback to frequency
        self.assertEqual(meta.tgid, "")

        # %TG template must expand to empty string - NO FALLBACK to frequency
        expanded_tg = format_template("%TG", meta)
        self.assertEqual(expanded_tg, "")

        # %F expands strictly to frequency
        expanded_f = format_template("%F", meta)
        self.assertEqual(expanded_f, "155.7975")

        # %FT expands to frequency when TG is empty per ProScan spec
        expanded_ft = format_template("%FT", meta)
        self.assertEqual(expanded_ft, "155.7975")

        # WAV encoding must NOT emit fake TGID tags
        wav_bytes = write_wav_riff_info(b"\x00\x00" * 100, meta)
        self.assertNotIn(b"TGID: 155.7975", wav_bytes)
        self.assertNotIn(b"TGID 155.7975", wav_bytes)

    def test_trunked_channel_with_actual_tgid(self):
        payload = CallTransmissionPayload(
            scanner_id="scanner_a",
            scanner_model="BCD436HP",
            system_name="MOSWIN",
            department_name="Highway Patrol",
            channel_name="Troop C Dispatch",
            frequency=773.80625,
            tgid="20104",
            timestamp=time.time(),
            duration_seconds=3.2
        )
        feeder = ScanScribeFeeder(ScanScribeFeederConfig())
        meta = feeder._build_proscan_metadata(payload)

        # Actual talkgroup preserved
        self.assertEqual(meta.tgid, "20104")
        self.assertEqual(format_template("%TG", meta), "20104")
        self.assertEqual(format_template("%C (%TG)", meta), "Troop C Dispatch (20104)")

        # WAV encoding must include actual TGID tags
        wav_bytes = write_wav_riff_info(b"\x00\x00" * 100, meta)
        self.assertIn(b"TGID: 20104", wav_bytes)

if __name__ == "__main__":
    unittest.main()
