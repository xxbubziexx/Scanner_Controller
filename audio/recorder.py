import time
import copy
import logging
import threading
from collections import deque
from typing import List, Dict, Any, Optional, Callable
import numpy as np

logger = logging.getLogger("AudioRecorder")


def list_audio_input_devices() -> List[Dict[str, Any]]:
    """Enumerate system soundcards and audio capture devices"""
    devices = []
    try:
        import sounddevice as sd
        default_dev = sd.default.device[0] if sd.default.device else -1
        all_devs = sd.query_devices()
        for idx, d in enumerate(all_devs):
            if d.get("max_input_channels", 0) > 0:
                devices.append({
                    "index": idx,
                    "name": d.get("name", f"Audio Device {idx}"),
                    "channels": d.get("max_input_channels", 1),
                    "is_default": (idx == default_dev),
                    "hostapi": d.get("hostapi", 0)
                })
    except Exception as e:
        logger.warning(f"Could not enumerate sound input devices: {e}")
    return devices


def list_audio_output_devices() -> List[Dict[str, Any]]:
    """Enumerate system soundcards and playback output devices (speakers/headphones)"""
    devices = []
    try:
        import sounddevice as sd
        default_out = sd.default.device[1] if sd.default.device else -1
        all_devs = sd.query_devices()
        for idx, d in enumerate(all_devs):
            if d.get("max_output_channels", 0) > 0:
                devices.append({
                    "index": idx,
                    "name": d.get("name", f"Output Device {idx}"),
                    "channels": d.get("max_output_channels", 2),
                    "is_default": (idx == default_out),
                    "hostapi": d.get("hostapi", 0)
                })
    except Exception as e:
        logger.warning(f"Could not enumerate sound output devices: {e}")
    return devices


class PreRollRingBuffer:
    """
    Exact, sample-accurate circular buffer for pre-roll transmission capture.
    Guarantees exactly capacity_samples of chronological audio with zero chunk-rounding errors.
    """
    def __init__(self, capacity_samples: int):
        self.capacity = max(1, capacity_samples)
        self.buffer = np.zeros(self.capacity, dtype=np.int16)
        self.write_idx = 0
        self.count = 0
        self.lock = threading.Lock()

    def write(self, samples: np.ndarray):
        """Append raw int16 PCM samples to circular buffer."""
        n = len(samples)
        if n == 0:
            return
        with self.lock:
            if n >= self.capacity:
                self.buffer[:] = samples[-self.capacity:]
                self.write_idx = 0
                self.count = self.capacity
                return
            end = self.write_idx + n
            if end <= self.capacity:
                self.buffer[self.write_idx:end] = samples
            else:
                first = self.capacity - self.write_idx
                self.buffer[self.write_idx:] = samples[:first]
                self.buffer[:end % self.capacity] = samples[first:]
            self.write_idx = end % self.capacity
            self.count = min(self.capacity, self.count + n)

    def get_preroll_bytes(self) -> bytes:
        """Returns exact chronological int16 PCM bytes up to capacity."""
        with self.lock:
            if self.count == 0:
                return b""
            if self.count < self.capacity:
                return self.buffer[:self.count].tobytes()
            # Ring buffer full: oldest sample starts at write_idx
            return np.concatenate((self.buffer[self.write_idx:], self.buffer[:self.write_idx])).tobytes()


class AudioRingBuffer:
    """
    Thread-safe, click-free dual-channel ring buffer for smooth real-time audio loopback monitoring.
    Maintains independent channel write/read queues so that Scanner A and Scanner B can stream
    audio concurrently without interleaving, alternating silence chunks, or clock-drift starvation.
    """
    def __init__(self, capacity: int = 96000, channels: int = 2, min_prebuffer: int = 4096):
        self.capacity = max(1, capacity)
        self.channels = channels
        self.buffer = np.zeros((self.capacity, channels), dtype=np.float32)
        self.write_idx = [0] * channels
        self.read_idx = [0] * channels
        self.available_ch = [0] * channels
        self.lock = threading.Lock()
        self.min_prebuffer = min_prebuffer  # ~92.8ms @ 44.1kHz (4 blocks of 1024)
        self.is_buffering_ch = [True] * channels

    @property
    def available(self) -> int:
        with self.lock:
            return max(self.available_ch) if self.available_ch else 0

    @property
    def is_buffering(self) -> bool:
        with self.lock:
            return all(self.is_buffering_ch)

    @is_buffering.setter
    def is_buffering(self, val: bool):
        with self.lock:
            for ch in range(self.channels):
                self.is_buffering_ch[ch] = val

    def write(self, data: np.ndarray):
        """Append multi-channel or interleaved data across channels."""
        n = len(data)
        if n == 0:
            return
        with self.lock:
            if data.ndim == 1:
                for ch in range(self.channels):
                    self._write_ch(ch, data)
            else:
                for ch in range(min(self.channels, data.shape[1])):
                    self._write_ch(ch, data[:, ch])

    def write_channel(self, ch: int, data: np.ndarray):
        """Append data directly to a specific channel (e.g. 0=scanner_a, 1=scanner_b)."""
        n = len(data)
        if n == 0 or ch >= self.channels:
            return
        with self.lock:
            self._write_ch(ch, data)

    def _write_ch(self, ch: int, data: np.ndarray):
        n = len(data)
        avail = self.available_ch[ch]
        w_idx = self.write_idx[ch]
        if n > self.capacity - avail:
            excess = n - (self.capacity - avail)
            self.read_idx[ch] = (self.read_idx[ch] + excess) % self.capacity
            avail -= excess

        end = w_idx + n
        if end <= self.capacity:
            self.buffer[w_idx:end, ch] = data
        else:
            first = self.capacity - w_idx
            self.buffer[w_idx:, ch] = data[:first]
            self.buffer[:end % self.capacity, ch] = data[first:]
        self.write_idx[ch] = end % self.capacity
        self.available_ch[ch] = avail + n
        if self.is_buffering_ch[ch] and self.available_ch[ch] >= self.min_prebuffer:
            self.is_buffering_ch[ch] = False

    def read(self, outdata: np.ndarray, pan_mode: str = "STEREO_SPLIT"):
        """Read time-aligned audio for all channels with smooth mixing."""
        frames = len(outdata)
        outdata.fill(0)
        num_ch = min(self.channels, outdata.shape[1])
        temp = np.zeros((frames, num_ch), dtype=np.float32)

        with self.lock:
            for ch in range(num_ch):
                avail = self.available_ch[ch]
                if self.is_buffering_ch[ch] or avail == 0:
                    continue
                to_read = min(frames, avail)
                r_idx = self.read_idx[ch]
                end = r_idx + to_read
                if end <= self.capacity:
                    temp[:to_read, ch] = self.buffer[r_idx:end, ch]
                else:
                    first = self.capacity - r_idx
                    temp[:first, ch] = self.buffer[r_idx:, ch]
                    temp[first:to_read, ch] = self.buffer[:end % self.capacity, ch]
                self.read_idx[ch] = end % self.capacity
                self.available_ch[ch] -= to_read

        if pan_mode == "CENTER" and num_ch >= 2:
            mixed = np.clip((temp[:, 0] + temp[:, 1]) * 0.707, -1.0, 1.0)
            outdata[:, 0] = mixed
            outdata[:, 1] = mixed
        else:
            outdata[:, :num_ch] = temp


class AudioLoopbackMonitor:
    """
    Real-time audio loopback monitor that plays incoming scanner audio
    to a user-selected playback output device (speakers or headphones).
    Supports volume control, stereo panning (Scanner A=Left, Scanner B=Right),
    and smooth 15ms de-click gain ramping during mutual exclusion muting/ducking.
    """
    def __init__(
        self,
        output_device_index: Optional[int] = None,
        enabled: bool = True,
        volume: float = 0.8,
        pan_mode: str = "STEREO_SPLIT"
    ):
        self.output_device_index = output_device_index
        self.enabled = enabled
        self.volume = max(0.0, min(1.0, volume))
        self.pan_mode = (pan_mode or "STEREO_SPLIT").upper()
        self.samplerate = 44100
        self._stream = None
        self._running = False
        self.ring_buffer = AudioRingBuffer(capacity=96000, channels=2, min_prebuffer=2048)
        self._current_gain: Dict[str, float] = {"scanner_a": 1.0, "scanner_b": 1.0}
        self._target_gain: Dict[str, float] = {"scanner_a": 1.0, "scanner_b": 1.0}
        self._ramp_samples: int = int(44100 * 0.015)  # 15ms de-click ramp (~661 samples @ 44.1k)

    def start(self):
        if self._running or not self.enabled:
            return
        try:
            import sounddevice as sd
            try:
                # Query the endpoint mix format configured in Windows Sound settings
                if self.output_device_index is not None:
                    dev_info = sd.query_devices(self.output_device_index)
                else:
                    dev_info = sd.query_devices(kind="output")
                if dev_info and "default_samplerate" in dev_info:
                    self.samplerate = int(dev_info["default_samplerate"])
            except Exception:
                self.samplerate = 44100

            self._ramp_samples = max(1, int(self.samplerate * 0.015))

            def playback_callback(outdata, frames, time_info, status):
                self.ring_buffer.read(outdata, pan_mode=self.pan_mode)

            self._stream = sd.OutputStream(
                device=self.output_device_index,
                channels=2,
                samplerate=self.samplerate,
                callback=playback_callback,
                blocksize=1024
            )
            self._stream.start()
            self._running = True
            logger.info(f"AudioLoopbackMonitor active on device {self.output_device_index} @ {self.samplerate}Hz (pan: {self.pan_mode}, vol: {int(self.volume*100)}%)")
        except Exception as e:
            logger.warning(f"Could not start AudioLoopbackMonitor ({e}). Loopback monitor dormant.")
            self._running = False
            self._stream = None

    def stop(self):
        self._running = False
        if self._stream:
            try:
                self._stream.stop()
                self._stream.close()
            except Exception:
                pass
            self._stream = None

    def update_config(self, output_device_index: Optional[int], enabled: bool, volume: float, pan_mode: str):
        restart_needed = (output_device_index != self.output_device_index) or (enabled != self.enabled)
        self.output_device_index = output_device_index
        self.enabled = enabled
        self.volume = max(0.0, min(1.0, volume))
        self.pan_mode = (pan_mode or "STEREO_SPLIT").upper()
        if restart_needed:
            self.stop()
            if self.enabled:
                self.start()

    def mute_scanner(self, scanner_id: str, mute: bool):
        """Sets target gain to 0.0 (muted) or 1.0 (unmuted). Gain smoothly ramps over 15ms."""
        self._target_gain[scanner_id] = 0.0 if mute else 1.0

    def push_audio(self, scanner_id: str, mono: np.ndarray, in_rate: int):
        if not self._running or not self.enabled:
            return

        start_g = self._current_gain.get(scanner_id, 1.0)
        target_g = self._target_gain.get(scanner_id, 1.0)

        # If fully muted and not ramping, drop audio without processing
        if start_g == 0.0 and target_g == 0.0:
            return

        N = len(mono)
        if N == 0:
            return

        # Direct sample pass-through when sample rates match (zero overhead)
        if in_rate == self.samplerate:
            scaled = mono * self.volume
        else:
            # Fast, smooth linear interpolation if rates differ
            target_len = int(round(N * (self.samplerate / float(in_rate))))
            if target_len <= 0:
                return
            x_old = np.linspace(0.0, 1.0, N, endpoint=False, dtype=np.float32)
            x_new = np.linspace(0.0, 1.0, target_len, endpoint=False, dtype=np.float32)
            scaled = np.interp(x_new, x_old, mono).astype(np.float32) * self.volume

        # Smooth de-click gain ramping (15ms crossfade on mute/unmute/duck)
        if start_g != target_g:
            ramp_len = max(1, self._ramp_samples)
            step = (target_g - start_g) / float(ramp_len)
            sample_steps = np.arange(1, len(scaled) + 1, dtype=np.float32) * step
            ramp_g = np.clip(start_g + sample_steps, min(start_g, target_g), max(start_g, target_g))
            scaled = scaled * ramp_g
            self._current_gain[scanner_id] = float(ramp_g[-1])
        elif start_g < 1.0:
            scaled = scaled * start_g

        ch_idx = 0 if scanner_id == "scanner_a" else 1
        self.ring_buffer.write_channel(ch_idx, scaled)


class ActiveCallSession:
    """Tracks an ongoing transmission's audio buffer and radio metadata"""
    def __init__(self, scanner_id: str, status_snapshot: Any, pre_buffer: bytes):
        self.scanner_id: str = scanner_id
        self.start_time: float = time.time()
        self.last_active_time: float = self.start_time
        # Clone snapshot so driver serial polling doesn't mutate it in-place
        self.status = status_snapshot.clone() if hasattr(status_snapshot, "clone") else copy.deepcopy(status_snapshot)
        self.audio_chunks: List[bytes] = [pre_buffer] if pre_buffer else []
        self.closed: bool = False

    def append(self, pcm_bytes: bytes):
        self.audio_chunks.append(pcm_bytes)
        self.last_active_time = time.time()

    def get_full_pcm(self) -> bytes:
        return b"".join(self.audio_chunks)

    def duration(self) -> float:
        return self.last_active_time - self.start_time


def is_generic_channel(channel_name: Optional[str]) -> bool:
    ch = str(channel_name or "").strip().lower()
    return ch in {
        "", "scanning", "scanning...", "search", "custom search",
        "service search", "none", "blank", "active call", "channel",
        "active channel", "unknown channel"
    }


def is_generic_system(system_name: Optional[str]) -> bool:
    sys_name = str(system_name or "").strip().lower()
    return sys_name in {"", "scanning", "scanning...", "search", "custom search", "service search", "none", "blank"}


def should_update_session_status(old_status: Any, new_status: Any) -> bool:
    """Returns True if new_status contains richer or non-generic metadata than old_status"""
    if not new_status:
        return False
    if not old_status:
        return True

    old_ch_generic = is_generic_channel(getattr(old_status, "channel_name", ""))
    new_ch_generic = is_generic_channel(getattr(new_status, "channel_name", ""))

    old_sys_generic = is_generic_system(getattr(old_status, "system_name", ""))
    new_sys_generic = is_generic_system(getattr(new_status, "system_name", ""))

    # 1. Upgrade from generic channel name to real channel name
    if old_ch_generic and not new_ch_generic:
        return True

    # 2. Upgrade from generic system name to real system name
    if old_sys_generic and not new_sys_generic:
        return True

    # 3. Upgrade with new TGID if old had none
    if not getattr(old_status, "tgid", None) and getattr(new_status, "tgid", None):
        return True

    # 4. Upgrade with new Unit ID if old had none
    if not getattr(old_status, "unit_id", None) and getattr(new_status, "unit_id", None):
        return True

    # 5. Upgrade with new Frequency if old had none
    if not getattr(old_status, "frequency", None) and getattr(new_status, "frequency", None):
        return True

    # 6. Upgrade Department if old had none and new has one
    if not getattr(old_status, "dept_name", "") and getattr(new_status, "dept_name", ""):
        return True

    return False


def is_valid_metadata_status(status: Any) -> bool:
    """Check if scanner status snapshot contains valid channel/system or frequency metadata (not generic Scanning/Search)"""
    if not status:
        return False
    return not is_generic_channel(getattr(status, "channel_name", "")) or not is_generic_system(getattr(status, "system_name", "")) or bool(getattr(status, "tgid", None) or getattr(status, "frequency", None))


class SingleScannerAudioStream:
    """
    Dedicated audio capture stream for a single scanner.
    Supports independent hardware audio device selection, channel demux (MONO, LEFT, RIGHT),
    pre-roll ring buffer, squelch sync, live audio loopback monitoring, and hang-time handling.
    """
    def __init__(
        self,
        scanner_id: str,
        device_index: Optional[int] = None,
        channel_mode: str = "MONO",
        sample_rate: int = 48000,
        hang_time_seconds: float = 1.5,
        min_duration_seconds: float = 0.4,
        enabled: bool = True,
        dispatch_callback: Optional[Callable] = None,
        loopback_monitor: Optional[AudioLoopbackMonitor] = None
    ):
        self.scanner_id: str = scanner_id
        self.device_index: Optional[int] = device_index
        self.channel_mode: str = (channel_mode or "MONO").upper()
        self.sample_rate: int = sample_rate
        self.hang_time_seconds: float = hang_time_seconds
        self.min_duration_seconds: float = min_duration_seconds
        self.enabled: bool = enabled
        self.recording_enabled: bool = True
        self.dispatch_callback: Optional[Callable] = dispatch_callback
        self.loopback_monitor: Optional[AudioLoopbackMonitor] = loopback_monitor

        self._stream = None
        self._running: bool = False
        self._lock = threading.Lock()

        # Exact 2.0s sample-accurate pre-roll circular buffer (zero chunk-rounding errors)
        self._pre_buffer = PreRollRingBuffer(capacity_samples=int(sample_rate * 2.0))

        self.active_session: Optional[ActiveCallSession] = None
        self.pending_release: Optional[ActiveCallSession] = None
        self.live_level_pct: int = 0

    def start(self):
        if self._running or not self.enabled:
            return

        try:
            import sounddevice as sd
            self._running = True

            # Query hardware endpoint mix format configured in Windows Sound settings
            if self.device_index is not None:
                try:
                    dev_info = sd.query_devices(self.device_index)
                    if dev_info and "default_samplerate" in dev_info:
                        self.sample_rate = int(dev_info["default_samplerate"])
                except Exception:
                    pass

            # Match capture sample rate to loopback monitor output if active
            if self.loopback_monitor and self.loopback_monitor.samplerate:
                self.sample_rate = self.loopback_monitor.samplerate

            # Re-init pre-buffer with final matched sample rate
            self._pre_buffer = PreRollRingBuffer(capacity_samples=int(self.sample_rate * 2.0))

            # Open 2 channels if supported by device to avoid Windows hardware auto-downmixing
            num_channels = 2
            if self.device_index is not None:
                try:
                    dev_info = sd.query_devices(self.device_index)
                    num_channels = min(2, dev_info.get("max_input_channels", 1))
                except Exception:
                    num_channels = 1

            def audio_callback(indata, frames, time_info, status):
                if status:
                    logger.debug(f"[{self.scanner_id}] Audio status: {status}")

                # Demux channel based on configuration without channel cross-talk
                if indata.shape[1] > 1:
                    if self.channel_mode == "LEFT":
                        mono = indata[:, 0]
                    elif self.channel_mode == "RIGHT":
                        mono = indata[:, 1]
                    else:
                        mono = 0.5 * (indata[:, 0] + indata[:, 1])
                else:
                    mono = indata[:, 0] if indata.ndim > 1 else indata.flatten()

                # Scale float to int16 PCM for recording
                scaled = np.clip(mono, -1.0, 1.0) * 32767.0
                int16_samples = scaled.astype(np.int16)
                pcm_chunk = int16_samples.tobytes()

                # Calculate RMS VU level
                rms = np.sqrt(np.mean(mono ** 2))
                self.live_level_pct = min(100, int(rms * 250))

                # Real-time audio loopback to output speakers/headphones (always active if monitor enabled)
                if self.loopback_monitor:
                    self.loopback_monitor.push_audio(self.scanner_id, mono, self.sample_rate)

                # Only buffer and record calls if recording is enabled
                if self.recording_enabled:
                    with self._lock:
                        self._pre_buffer.write(int16_samples)
                        if self.active_session:
                            self.active_session.append(pcm_chunk)

            self._stream = sd.InputStream(
                device=self.device_index,
                channels=num_channels,
                samplerate=self.sample_rate,
                dtype="float32",
                blocksize=1024,
                callback=audio_callback
            )
            self._stream.start()
            logger.info(f"[{self.scanner_id}] Audio capture started on device {self.device_index} (ch: {self.channel_mode}) @ {self.sample_rate}Hz")
        except Exception as e:
            logger.warning(f"[{self.scanner_id}] Could not start audio stream ({e}). Audio dormant.")
            self._running = False
            self._stream = None

    def stop(self):
        self._running = False
        if self._stream:
            try:
                self._stream.stop()
                self._stream.close()
            except Exception:
                pass
            self._stream = None
        self.live_level_pct = 0

    def on_squelch_open(self, status_snapshot: Any):
        if not self.recording_enabled:
            return

        with self._lock:
            if self.pending_release:
                session = self.pending_release
                self.pending_release = None
                if should_update_session_status(session.status, status_snapshot):
                    session.status = status_snapshot.clone() if hasattr(status_snapshot, "clone") else copy.deepcopy(status_snapshot)
                self.active_session = session
                return

            if not self.active_session:
                pre_roll = self._pre_buffer.get_preroll_bytes()
                self.active_session = ActiveCallSession(self.scanner_id, status_snapshot, pre_roll)
                raw_ch = getattr(status_snapshot, "channel_name", "")
                raw_freq = getattr(status_snapshot, "frequency", None)
                raw_tgid = getattr(status_snapshot, "tgid", None)
                if raw_ch and not is_generic_channel(raw_ch):
                    ch_desc = raw_ch
                elif raw_freq:
                    ch_desc = f"{raw_freq:.4f} MHz"
                elif raw_tgid:
                    ch_desc = f"TGID {raw_tgid}"
                else:
                    ch_desc = raw_ch or "Active Call"
                logger.info(f"[{self.scanner_id}] Squelch Open -> Started recording: {ch_desc}")
            else:
                # Update status if new status contains richer/non-generic metadata
                if should_update_session_status(self.active_session.status, status_snapshot):
                    self.active_session.status = status_snapshot.clone() if hasattr(status_snapshot, "clone") else copy.deepcopy(status_snapshot)

    def on_squelch_close(self, status_snapshot: Any):
        with self._lock:
            if self.active_session:
                self.active_session.last_active_time = time.time()
                self.pending_release = self.active_session
                self.active_session = None

    def abort_session(self):
        """Immediately cancel and discard any ongoing or pending recording session due to mutual exclusion."""
        with self._lock:
            if self.active_session or self.pending_release:
                ch = ""
                sess = self.active_session or self.pending_release
                if sess and hasattr(sess.status, "channel_name"):
                    ch = sess.status.channel_name
                logger.info(f"[{self.scanner_id}] Squelch / Recording aborted due to mutual exclusion ({ch}).")
            self.active_session = None
            self.pending_release = None

    def check_hang_time(self):
        to_dispatch = None
        with self._lock:
            if self.pending_release:
                if time.time() - self.pending_release.last_active_time >= self.hang_time_seconds:
                    to_dispatch = self.pending_release
                    self.pending_release = None

        if to_dispatch:
            self._finalize_and_dispatch(to_dispatch)

    def _finalize_and_dispatch(self, session: ActiveCallSession):
        pcm = session.get_full_pcm()
        dur = session.duration()

        if dur < self.min_duration_seconds or len(pcm) < 1000:
            logger.debug(f"[{self.scanner_id}] Discarded brief transmission ({dur:.2f}s)")
            return

        logger.info(f"[{self.scanner_id}] Transmission complete ({dur:.1f}s, {len(pcm)} bytes @ {self.sample_rate}Hz). Dispatching to ScanScribe.")
        if self.dispatch_callback:
            try:
                import inspect
                sig = inspect.signature(self.dispatch_callback)
                if len(sig.parameters) >= 5:
                    self.dispatch_callback(session.scanner_id, session.status, pcm, dur, self.sample_rate)
                else:
                    self.dispatch_callback(session.scanner_id, session.status, pcm, dur)
            except Exception:
                try:
                    self.dispatch_callback(session.scanner_id, session.status, pcm, dur)
                except Exception as e:
                    logger.error(f"[{self.scanner_id}] Dispatch callback error: {e}")


class ScannerAudioRecorder:
    """
    Multi-Scanner Audio Recorder & Real-Time Playback Monitor Manager.
    Manages dedicated per-scanner audio input streams and a shared low-latency
    playback loopback monitor for hearing live scanner audio on computer speakers/headphones.
    """
    def __init__(
        self,
        sample_rate: int = 44100,
        hang_time_seconds: float = 1.5,
        min_duration_seconds: float = 0.4,
        dispatch_callback: Optional[Callable] = None,
        loopback_output_device: Optional[int] = None,
        loopback_enabled: bool = True,
        loopback_volume: float = 0.8,
        loopback_pan_mode: str = "STEREO_SPLIT"
    ):
        self.sample_rate = sample_rate
        self.hang_time_seconds = hang_time_seconds
        self.min_duration_seconds = min_duration_seconds
        self.dispatch_callback = dispatch_callback
        self.streams: Dict[str, SingleScannerAudioStream] = {}
        self.loopback_monitor = AudioLoopbackMonitor(
            output_device_index=loopback_output_device,
            enabled=loopback_enabled,
            volume=loopback_volume,
            pan_mode=loopback_pan_mode
        )
        self.recording_enabled: bool = True
        self._running = False

    def configure_loopback(
        self,
        output_device_index: Optional[int],
        enabled: bool = True,
        volume: float = 0.8,
        pan_mode: str = "STEREO_SPLIT"
    ):
        self.loopback_monitor.update_config(
            output_device_index=output_device_index,
            enabled=enabled,
            volume=volume,
            pan_mode=pan_mode
        )

    def mute_scanner(self, scanner_id: str, mute: bool):
        self.loopback_monitor.mute_scanner(scanner_id, mute)

    def set_recording_enabled(self, enabled: bool):
        self.recording_enabled = enabled
        for s in self.streams.values():
            s.recording_enabled = enabled

    def configure_scanner(
        self,
        scanner_id: str,
        device_index: Optional[int],
        channel_mode: str = "MONO",
        enabled: bool = True
    ):
        if scanner_id in self.streams:
            self.streams[scanner_id].stop()

        stream = SingleScannerAudioStream(
            scanner_id=scanner_id,
            device_index=device_index,
            channel_mode=channel_mode,
            sample_rate=self.sample_rate,
            hang_time_seconds=self.hang_time_seconds,
            min_duration_seconds=self.min_duration_seconds,
            enabled=enabled,
            dispatch_callback=self.dispatch_callback,
            loopback_monitor=self.loopback_monitor
        )
        stream.recording_enabled = self.recording_enabled
        self.streams[scanner_id] = stream
        if self._running:
            stream.start()

    def start(self):
        self._running = True
        self.loopback_monitor.start()
        for s in self.streams.values():
            if self.loopback_monitor and self.loopback_monitor.samplerate:
                s.sample_rate = self.loopback_monitor.samplerate
            s.start()

    def stop(self):
        self._running = False
        for s in self.streams.values():
            s.stop()
        self.loopback_monitor.stop()

    def abort_session(self, scanner_id: str):
        if scanner_id in self.streams:
            self.streams[scanner_id].abort_session()

    def mute_scanner(self, scanner_id: str, mute: bool):
        if self.loopback_monitor:
            self.loopback_monitor.mute_scanner(scanner_id, mute)

    def on_squelch_open(self, scanner_id: str, status_snapshot: Any):
        if scanner_id in self.streams:
            self.streams[scanner_id].on_squelch_open(status_snapshot)

    def on_squelch_close(self, scanner_id: str, status_snapshot: Any):
        if scanner_id in self.streams:
            self.streams[scanner_id].on_squelch_close(status_snapshot)

    def check_hang_time_expirations(self):
        for s in self.streams.values():
            s.check_hang_time()

    def get_levels(self) -> Dict[str, int]:
        return {sc_id: (s.live_level_pct if (s.enabled and s._running) else 0) for sc_id, s in self.streams.items()}

    @property
    def live_level_pct(self) -> int:
        levels = self.get_levels()
        return max(levels.values()) if levels else 0
