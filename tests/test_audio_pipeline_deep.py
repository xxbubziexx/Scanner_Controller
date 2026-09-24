import time
import unittest
import numpy as np
from audio.recorder import PreRollRingBuffer, AudioRingBuffer, AudioLoopbackMonitor
from drivers.base_driver import ScannerStatus
from engine.sync_manager import SyncManager
from config import AppConfig

class TestAudioPipelineDeep(unittest.TestCase):
    """
    Rigorously tests:
    1. Exact sample-accurate pre-roll boundary (zero chunk quantization / off-by-one errors)
    2. Ring buffer underrun recovery and zero-lockout resilience under thread stalls
    3. Smooth 15ms de-click gain ramping during mutual exclusion muting/unmuting
    4. Decoupled ThreadPoolExecutor async dispatch under writer delays
    """

    def test_preroll_boundary_exact_sample_accuracy(self):
        """
        Asserts the 2.0s ring yields EXACTLY 2.0s of pre-roll at a transmission boundary,
        with 0 off-by-one errors and strict chronological order preserved across wraps.
        """
        sample_rate = 44100
        exact_capacity = int(sample_rate * 2.0)  # 88,200 samples = 176,400 bytes
        preroll = PreRollRingBuffer(capacity_samples=exact_capacity)

        # 1. Partial fill: feed 1.0s of sequential audio
        seq_1s = (np.arange(44100) % 25000).astype(np.int16)
        preroll.write(seq_1s)
        partial_bytes = preroll.get_preroll_bytes()
        self.assertEqual(len(partial_bytes), 44100 * 2)
        partial_arr = np.frombuffer(partial_bytes, dtype=np.int16)
        np.testing.assert_array_equal(partial_arr, seq_1s)

        # 2. Overfill & wrap: write another 100,000 sequential samples (total 144,100 samples)
        # Oldest samples should be overwritten; buffer must contain exactly the last 88,200 samples.
        seq_overflow = (np.arange(44100, 144100) % 25000).astype(np.int16)
        preroll.write(seq_overflow)

        full_bytes = preroll.get_preroll_bytes()
        # Assert exact byte count (not "about 2s")
        self.assertEqual(len(full_bytes), exact_capacity * 2)
        self.assertEqual(len(full_bytes), 176400)

        # Assert exact chronological order from (144100 - 88200) to 144100
        full_arr = np.frombuffer(full_bytes, dtype=np.int16)
        self.assertEqual(len(full_arr), exact_capacity)
        expected = (np.arange(144100 - exact_capacity, 144100) % 25000).astype(np.int16)
        np.testing.assert_array_equal(full_arr, expected)

    def test_underrun_recovery_no_lockout(self):
        """
        Feeds audio into AudioRingBuffer, causes an intentional consumer starvation / underrun,
        and verifies that:
        1. The buffer zero-pads the deficit instead of raising an error or corrupting state
        2. The buffer does NOT enter a permanent muting lockout
        3. Next incoming audio block plays immediately without being silenced
        """
        # Block size = 1024, capacity = 96000, prebuffer = 2048 (~46.4ms @ 44.1k)
        ring = AudioRingBuffer(capacity=96000, channels=2, min_prebuffer=2048)
        self.assertTrue(ring.is_buffering)

        # Prime with 2048 samples of ones
        prime_data = np.ones((2048, 2), dtype=np.float32) * 0.5
        ring.write(prime_data)
        self.assertFalse(ring.is_buffering)
        self.assertEqual(ring.available, 2048)

        # Read 1 block of 1024
        out_block1 = np.zeros((1024, 2), dtype=np.float32)
        ring.read(out_block1)
        self.assertTrue(np.allclose(out_block1, 0.5))
        self.assertEqual(ring.available, 1024)

        # Read 1 block of 1024 (drains buffer completely)
        out_block2 = np.zeros((1024, 2), dtype=np.float32)
        ring.read(out_block2)
        self.assertTrue(np.allclose(out_block2, 0.5))
        self.assertEqual(ring.available, 0)

        # Starvation / Underrun test: consumer reads when available is 0
        out_underrun = np.full((1024, 2), -99.0, dtype=np.float32)
        ring.read(out_underrun)
        # Should be zero-padded
        self.assertTrue(np.allclose(out_underrun, 0.0))
        # Critical assertion: should NOT have permanently re-locked into buffering
        self.assertFalse(ring.is_buffering)

        # Partial underrun test: write 512 samples, consumer asks for 1024
        partial_data = np.ones((512, 2), dtype=np.float32) * 0.75
        ring.write(partial_data)
        self.assertEqual(ring.available, 512)

        out_partial = np.zeros((1024, 2), dtype=np.float32)
        ring.read(out_partial)
        # First 512 must be 0.75, remaining 512 must be zero-padded
        self.assertTrue(np.allclose(out_partial[:512], 0.75))
        self.assertTrue(np.allclose(out_partial[512:], 0.0))
        self.assertEqual(ring.available, 0)

        # Immediate resumption: next block must play immediately without lockout
        resume_data = np.ones((1024, 2), dtype=np.float32) * 0.9
        ring.write(resume_data)
        out_resume = np.zeros((1024, 2), dtype=np.float32)
        ring.read(out_resume)
        self.assertTrue(np.allclose(out_resume, 0.9))

    def test_smooth_declicking_gain_ramp(self):
        """
        Tests that muting or unmuting a scanner applies a smooth 15ms linear gain ramp
        rather than an instantaneous step-function discontinuity that causes audible clicks.
        """
        monitor = AudioLoopbackMonitor(output_device_index=None, enabled=True, volume=1.0)
        monitor.samplerate = 44100
        monitor._running = True
        monitor._ramp_samples = int(44100 * 0.015)  # 661 samples (~15ms)

        # Prime with 2048 steady samples to complete initial pre-buffering
        monitor.push_audio("scanner_a", np.ones(2048, dtype=np.float32), in_rate=44100)

        # Drain the unmuted primed blocks
        drain = np.zeros((2048, 2), dtype=np.float32)
        monitor.ring_buffer.read(drain)
        self.assertEqual(monitor.ring_buffer.available, 0)

        # Now trigger mutual exclusion mute on Scanner A
        monitor.mute_scanner("scanner_a", mute=True)
        self.assertEqual(monitor._target_gain["scanner_a"], 0.0)

        # Push next block: gain must ramp smoothly downwards
        input_samples = np.ones(1024, dtype=np.float32)
        monitor.push_audio("scanner_a", input_samples, in_rate=44100)

        ramp_out = np.zeros((1024, 2), dtype=np.float32)
        monitor.ring_buffer.read(ramp_out)

        left_ch = ramp_out[:, 0]
        # First sample must start close to 1.0 (0.998)
        self.assertAlmostEqual(left_ch[0], 1.0, delta=0.01)
        # Gain must decrease monotonically across the ramp
        self.assertTrue(np.all(np.diff(left_ch[:monitor._ramp_samples]) <= 0.0))
        # After ramp_samples (~661 samples), gain must reach exactly 0.0 (muted)
        self.assertEqual(left_ch[monitor._ramp_samples], 0.0)
        self.assertEqual(left_ch[-1], 0.0)

        # Subsequent block must be completely silent and not pushed to ring buffer (saving CPU)
        avail_before = monitor.ring_buffer.available
        monitor.push_audio("scanner_a", input_samples, in_rate=44100)
        self.assertEqual(monitor.ring_buffer.available, avail_before)

        # Test Unmute Ramp: trigger unmute
        monitor.mute_scanner("scanner_a", mute=False)
        self.assertEqual(monitor._target_gain["scanner_a"], 1.0)

        monitor.push_audio("scanner_a", input_samples, in_rate=44100)
        unmute_out = np.zeros((1024, 2), dtype=np.float32)
        monitor.ring_buffer.read(unmute_out)
        left_unmute = unmute_out[:, 0]

        # Starts near 0 and ramps monotonically upwards
        self.assertAlmostEqual(left_unmute[0], 0.0, delta=0.01)
        self.assertTrue(np.all(np.diff(left_unmute[:monitor._ramp_samples]) >= 0.0))
        self.assertEqual(left_unmute[monitor._ramp_samples], 1.0)
        self.assertEqual(left_unmute[-1], 1.0)

    def test_async_dispatch_executor_unblocked(self):
        """
        Tests that call dispatch is offloaded to the ThreadPoolExecutor
        and does NOT block the caller even when the downstream writer encounters delays.
        """
        config = AppConfig()
        config.scanner_a.port = "NONE"
        config.scanner_b.port = "NONE"
        config.feeder.enable_feeder = True

        sync_mgr = SyncManager(config)

        call_dispatched_event = []

        def slow_dispatch(payload):
            time.sleep(0.1)  # Simulate 100ms disk / MP3 encode delay
            call_dispatched_event.append(payload.scanner_id)
            return {"status": "success"}

        sync_mgr.feeder.dispatch_call = slow_dispatch

        status = ScannerStatus("scanner_a", "Scanner A", "BCD436HP")
        status.channel_name = "Dispatch 1"
        status.tgid = "10401"
        dummy_pcm = b"\x00\x00" * 8000

        # Dispatch call: must return immediately (non-blocking)
        t0 = time.time()
        sync_mgr._on_call_completed("scanner_a", status, dummy_pcm, 1.0)
        elapsed = time.time() - t0

        # Must return in under 20ms despite 100ms worker sleep
        self.assertLess(elapsed, 0.02)

        # Wait for worker thread to finish
        time.sleep(0.15)
        self.assertEqual(len(call_dispatched_event), 1)
        self.assertEqual(call_dispatched_event[0], "scanner_a")

        sync_mgr._dispatch_executor.shutdown(wait=True)

    def test_hardware_samplerate_resampling_to_feeder_rate(self):
        """
        Verifies that when hardware captures at 48000Hz (or 44100Hz),
        _dispatch_worker downsamples the PCM to the feeder target rate (16000Hz)
        preventing audio files from playing back slowed down.
        """
        config = AppConfig()
        config.scanner_a.port = "NONE"
        config.scanner_b.port = "NONE"
        config.feeder.enable_feeder = True
        config.feeder.sample_rate = 16000

        sync_mgr = SyncManager(config)
        # Mock scanner_a stream to have 48000Hz sample_rate (as negotiated by sound card)
        stream_a = sync_mgr.audio_recorder.streams.get("scanner_a")
        if stream_a:
            stream_a.sample_rate = 48000

        dispatched_payload = []
        sync_mgr.feeder.dispatch_call = lambda p: dispatched_payload.append(p) or {"status": "success"}

        status = ScannerStatus("scanner_a", "Scanner A", "BCD436HP")
        # 1.0 second of 48000Hz 16-bit mono PCM = 48000 samples = 96000 bytes
        input_pcm = (np.sin(2 * np.pi * 440 * np.arange(48000) / 48000.0) * 16000).astype(np.int16).tobytes()
        self.assertEqual(len(input_pcm), 96000)

        sync_mgr._dispatch_worker("scanner_a", status, input_pcm, 1.0)
        self.assertEqual(len(dispatched_payload), 1)

        output_pcm = dispatched_payload[0].audio_pcm
        # 1.0 second of 16000Hz 16-bit mono PCM = 16000 samples = 32000 bytes
        self.assertEqual(len(output_pcm), 32000)
        sync_mgr._dispatch_executor.shutdown(wait=True)

if __name__ == "__main__":
    unittest.main()
