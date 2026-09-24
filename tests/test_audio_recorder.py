import unittest
import time
from audio.recorder import ScannerAudioRecorder, list_audio_input_devices
from drivers.base_driver import ScannerStatus

class TestAudioRecording(unittest.TestCase):
    def test_list_audio_input_devices(self):
        devs = list_audio_input_devices()
        self.assertIsInstance(devs, list)
        for d in devs:
            self.assertIn("index", d)
            self.assertIn("name", d)
            self.assertIn("channels", d)

    def test_per_scanner_audio_lifecycle_and_dispatch(self):
        dispatched = []

        def callback(scanner_id, status, pcm_bytes, duration):
            dispatched.append({
                "scanner_id": scanner_id,
                "status": status,
                "pcm": pcm_bytes,
                "duration": duration
            })

        recorder = ScannerAudioRecorder(
            sample_rate=16000,
            hang_time_seconds=0.1,
            min_duration_seconds=0.0,
            dispatch_callback=callback
        )
        recorder.configure_scanner("scanner_a", device_index=None, channel_mode="MONO", enabled=True)
        recorder.configure_scanner("scanner_b", device_index=None, channel_mode="RIGHT", enabled=True)

        status_a = ScannerStatus("scanner_a", "Scanner A", "BCD436HP")
        status_a.channel_name = "Dispatch 1"
        status_a.frequency = 461.525

        status_b = ScannerStatus("scanner_b", "Scanner B", "BCD996P2")
        status_b.channel_name = "Tac 2"
        status_b.frequency = 154.280

        # Simulate squelch open on Scanner A
        recorder.on_squelch_open("scanner_a", status_a)
        stream_a = recorder.streams["scanner_a"]
        self.assertIsNotNone(stream_a.active_session)

        # Simulate feeding audio frames
        dummy_pcm = b"\x10\x00" * 4000
        stream_a.active_session.append(dummy_pcm)
        time.sleep(0.05)

        # Simulate squelch close on Scanner A
        recorder.on_squelch_close("scanner_a", status_a)
        self.assertIsNone(stream_a.active_session)
        self.assertIsNotNone(stream_a.pending_release)

        # Wait past hang time
        time.sleep(0.12)
        recorder.check_hang_time_expirations()

        # Verify call from Scanner A was dispatched
        self.assertEqual(len(dispatched), 1)
        self.assertEqual(dispatched[0]["scanner_id"], "scanner_a")
        self.assertEqual(dispatched[0]["status"].channel_name, "Dispatch 1")
        self.assertGreater(len(dispatched[0]["pcm"]), 0)

        # Now test Scanner B with channel mode RIGHT
        recorder.on_squelch_open("scanner_b", status_b)
        stream_b = recorder.streams["scanner_b"]
        self.assertEqual(stream_b.channel_mode, "RIGHT")
        stream_b.active_session.append(dummy_pcm)
        time.sleep(0.05)
        recorder.on_squelch_close("scanner_b", status_b)
        time.sleep(0.12)
        recorder.check_hang_time_expirations()

        self.assertEqual(len(dispatched), 2)
        self.assertEqual(dispatched[1]["scanner_id"], "scanner_b")
        self.assertEqual(dispatched[1]["status"].channel_name, "Tac 2")

    def test_abort_session_prevents_dispatch(self):
        """Verify that aborting a session immediately discards audio and prevents ScanScribe dispatch."""
        dispatched = []
        recorder = ScannerAudioRecorder(
            sample_rate=16000,
            hang_time_seconds=0.1,
            min_duration_seconds=0.0,
            dispatch_callback=lambda *args: dispatched.append(args)
        )
        recorder.configure_scanner("scanner_b", device_index=None, enabled=True)
        status_b = ScannerStatus("scanner_b", "Scanner B", "BCD436HP")
        status_b.channel_name = "Excluded Channel"

        recorder.on_squelch_open("scanner_b", status_b)
        self.assertIsNotNone(recorder.streams["scanner_b"].active_session)

        # Feed audio
        recorder.streams["scanner_b"].active_session.append(b"\x20\x00" * 2000)

        # Abort session due to mutual exclusion
        recorder.abort_session("scanner_b")
        self.assertIsNone(recorder.streams["scanner_b"].active_session)
        self.assertIsNone(recorder.streams["scanner_b"].pending_release)

        # Even after hang time expiration, no dispatch should occur
        time.sleep(0.12)
        recorder.check_hang_time_expirations()
        self.assertEqual(len(dispatched), 0)

if __name__ == "__main__":
    unittest.main()
