import struct
import unittest
from config import AppConfig, ScanScribeFeederConfig, ALLOWED_SAMPLE_RATES
from metadata.proscan_metadata import ProScanMetadata, write_wav_riff_info
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload
from engine.sync_manager import resample_pcm_int16


class TestSamplesPerSecondSupport(unittest.TestCase):
    def test_allowed_sample_rates_list(self):
        """Verifies strictly only the rates from the screenshot: 8000, 11025, 16000, 22050, 44100. NONE ELSE."""
        expected_rates = [8000, 11025, 16000, 22050, 44100]
        self.assertEqual(ALLOWED_SAMPLE_RATES, expected_rates)

    def test_16000_samples_per_second_is_default_and_supported(self):
        """Specifically verifies 16000 samples per second is supported and is default."""
        feeder_cfg = ScanScribeFeederConfig()
        self.assertEqual(feeder_cfg.sample_rate, 16000)

        app_cfg = AppConfig()
        self.assertEqual(app_cfg.feeder.sample_rate, 16000)

    def test_all_five_rates_accepted_in_config(self):
        """Verifies each of the 5 rates is accepted by Pydantic config validation."""
        for rate in [8000, 11025, 16000, 22050, 44100]:
            cfg = ScanScribeFeederConfig(sample_rate=rate)
            self.assertEqual(cfg.sample_rate, rate)

    def test_invalid_rates_rejected_in_config(self):
        """Verifies any rate NOT in [8000, 11025, 16000, 22050, 44100] raises ValueError."""
        invalid_rates = [0, -1, 4000, 12000, 24000, 32000, 48000, 96000, 192000]
        for rate in invalid_rates:
            with self.assertRaises(ValueError, msg=f"Rate {rate} should have been rejected"):
                ScanScribeFeederConfig(sample_rate=rate)

    def test_wav_riff_header_sample_rates(self):
        """Verifies write_wav_riff_info writes accurate nSamplesPerSec for all 5 allowed rates."""
        meta = ProScanMetadata(system_name="TestSys", channel_name="TestChan", tgid="101")
        for rate in [8000, 11025, 16000, 22050, 44100]:
            pcm = b"\x00\x00" * rate  # 1 second
            wav_bytes = write_wav_riff_info(pcm, meta, sample_rate=rate)
            self.assertTrue(wav_bytes.startswith(b"RIFF"))

            # Locate fmt chunk
            fmt_idx = wav_bytes.find(b"fmt ")
            self.assertNotEqual(fmt_idx, -1)
            # WAVEFORMATEX: fmt (4) + len (4) + format (2) + channels (2) + samples_per_sec (4)
            fmt_data = wav_bytes[fmt_idx + 8: fmt_idx + 24]
            wFormatTag, nChannels, nSamplesPerSec, nAvgBytesPerSec, nBlockAlign, wBitsPerSample = struct.unpack(
                "<HHIIHH", fmt_data
            )
            self.assertEqual(wFormatTag, 1)  # PCM
            self.assertEqual(nChannels, 1)
            self.assertEqual(nSamplesPerSec, rate)
            self.assertEqual(nAvgBytesPerSec, rate * 2)
            self.assertEqual(nBlockAlign, 2)
            self.assertEqual(wBitsPerSample, 16)

    def test_wav_riff_rejects_unsupported_rates(self):
        """Verifies write_wav_riff_info rejects invalid rates."""
        meta = ProScanMetadata(system_name="TestSys", channel_name="TestChan")
        for rate in [12000, 32000, 48000, 96000]:
            with self.assertRaises(ValueError):
                write_wav_riff_info(b"\x00\x00" * 100, meta, sample_rate=rate)

    def test_feeder_audio_generation_for_all_supported_rates(self):
        """Verifies feeder _generate_audio_bytes generates tagged audio for all 5 rates."""
        for rate in [8000, 11025, 16000, 22050, 44100]:
            cfg = ScanScribeFeederConfig(audio_format="WAV", sample_rate=rate)
            feeder = ScanScribeFeeder(cfg)
            payload = CallTransmissionPayload(
                scanner_id="scanner_a",
                scanner_model="BCD436HP",
                system_name="Police System",
                channel_name="Dispatch",
                tgid="1001",
                frequency=851.0125,
                audio_pcm=b"\x00\x00" * rate
            )
            audio_bytes, ext = feeder._generate_audio_bytes(payload, ProScanMetadata(tgid="1001"))
            self.assertEqual(ext, "wav")
            fmt_idx = audio_bytes.find(b"fmt ")
            nSamplesPerSec = struct.unpack("<I", audio_bytes[fmt_idx + 12: fmt_idx + 16])[0]
            self.assertEqual(nSamplesPerSec, rate)

    def test_resampling_to_16000_and_all_allowed_rates(self):
        """Verifies resampling from standard sound card 48000Hz/44100Hz to 16000 and all allowed rates."""
        for in_rate in [44100, 48000]:
            pcm_in = b"\x10\x00" * in_rate  # 1.0 second of audio
            for target_rate in [8000, 11025, 16000, 22050, 44100]:
                out = resample_pcm_int16(pcm_in, in_rate, target_rate)
                samples_out = len(out) // 2
                # Allowed slight rounding delta (+/- 1 sample)
                self.assertAlmostEqual(samples_out, target_rate, delta=2)


if __name__ == "__main__":
    unittest.main()
