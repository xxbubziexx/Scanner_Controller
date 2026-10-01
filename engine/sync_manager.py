import time
import asyncio
import logging
from concurrent.futures import ThreadPoolExecutor
from typing import Dict, Any, List, Optional, Callable
from config import AppConfig, PriorityMode, ExclusionMode, ALLOWED_SAMPLE_RATES
from drivers.base_driver import BaseScannerDriver, ScannerStatus
from drivers.driver_factory import create_driver_for_model
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload
from audio.recorder import ScannerAudioRecorder

logger = logging.getLogger("SyncManager")

class ExclusionEvent:
    def __init__(self, primary_id: str, target_id: str, tgid: Optional[str], freq: Optional[float], channel: str, reason: str):
        self.timestamp: float = time.time()
        self.primary_id: str = primary_id
        self.target_id: str = target_id
        self.tgid: Optional[str] = tgid
        self.freq: Optional[float] = freq
        self.channel: str = channel
        self.reason: str = reason

    def to_dict(self) -> Dict[str, Any]:
        t_str = time.strftime("%H:%M:%S", time.localtime(self.timestamp))
        return {
            "timestamp": self.timestamp,
            "time_str": t_str,
            "primary_id": self.primary_id,
            "target_id": self.target_id,
            "tgid": self.tgid,
            "freq": self.freq,
            "channel": self.channel,
            "reason": self.reason,
            "message": f"[{t_str}] EXCLUDED: {self.target_id.upper()} skipped TGID {self.tgid or self.freq} ({self.channel}) because {self.primary_id.upper()} is active."
        }

def resample_pcm_int16(pcm_bytes: bytes, in_rate: int, out_rate: int) -> bytes:
    """Resamples 16-bit mono PCM bytes from in_rate to out_rate for AI transcription dispatch"""
    if in_rate == out_rate or not pcm_bytes:
        return pcm_bytes
    try:
        import numpy as np
        arr = np.frombuffer(pcm_bytes, dtype=np.int16).astype(np.float32)
        ratio = in_rate / float(out_rate)
        num_out = int(len(arr) / ratio)
        if num_out == 0:
            return pcm_bytes
        x_in = np.arange(len(arr), dtype=np.float32)
        t = np.linspace(0, len(arr) - 1, num_out, dtype=np.float32)
        resampled = np.interp(t, x_in, arr)
        scaled = np.clip(resampled, -32768.0, 32767.0).astype(np.int16)
        return scaled.tobytes()
    except Exception:
        return pcm_bytes


class SyncManager:
    def __init__(self, config: AppConfig):
        self.config: AppConfig = config
        self.scanner_a: Optional[BaseScannerDriver] = None
        self.scanner_b: Optional[BaseScannerDriver] = None
        self._loop_task: Optional[asyncio.Task] = None
        self._running: bool = False
        self.event_log: List[ExclusionEvent] = []
        self.ws_broadcast_callback: Optional[Callable] = None

        # Lockout tracking: key = tgid or str(freq), val = {"holder": "scanner_a", "release_at": float}
        self.active_exclusions: Dict[str, Dict[str, Any]] = {}

        # ScanScribe Feeder & Hardware Audio Recorder
        self.feeder = ScanScribeFeeder(
            self.config.feeder,
            {"scanner_a": self.config.scanner_a, "scanner_b": self.config.scanner_b}
        )
        self._dispatch_executor = ThreadPoolExecutor(max_workers=2, thread_name_prefix="AudioDispatch")
        self.audio_recorder = ScannerAudioRecorder(
            sample_rate=self.config.feeder.sample_rate,
            hang_time_seconds=self.config.sync.hang_time_seconds,
            dispatch_callback=self._on_call_completed
        )
        self._configure_audio_streams()

        self._prev_receiving_a: bool = False
        self._prev_receiving_b: bool = False
        self._last_skip_time: Dict[str, float] = {"scanner_a": 0.0, "scanner_b": 0.0}

        self.initialize_drivers()

    def _configure_audio_streams(self):
        """Set up independent audio streams and channel demux for Scanner A and Scanner B"""
        cfg_a = self.config.scanner_a
        dev_a = cfg_a.audio_device_index
        ch_a = cfg_a.audio_channel.value if hasattr(cfg_a.audio_channel, "value") else str(cfg_a.audio_channel)
        enabled_a = bool(cfg_a.enabled and cfg_a.audio_enabled)
        self.audio_recorder.configure_scanner(
            scanner_id="scanner_a",
            device_index=dev_a,
            channel_mode=ch_a,
            enabled=enabled_a
        )
        self.audio_recorder.mute_scanner("scanner_a", not enabled_a)

        cfg_b = self.config.scanner_b
        dev_b = cfg_b.audio_device_index
        ch_b = cfg_b.audio_channel.value if hasattr(cfg_b.audio_channel, "value") else str(cfg_b.audio_channel)
        enabled_b = bool(cfg_b.enabled and cfg_b.audio_enabled)
        self.audio_recorder.configure_scanner(
            scanner_id="scanner_b",
            device_index=dev_b,
            channel_mode=ch_b,
            enabled=enabled_b
        )
        self.audio_recorder.mute_scanner("scanner_b", not enabled_b)

        # Configure Real-Time Audio Loopback Playback Monitor
        if hasattr(self.config, "audio_loopback"):
            lb = self.config.audio_loopback
            self.audio_recorder.configure_loopback(
                output_device_index=lb.output_device_index,
                enabled=lb.enabled,
                volume=lb.volume,
                pan_mode=lb.pan_mode
            )

        # Sync Call Recording State with Feeder Config
        if hasattr(self.config, "feeder"):
            self.audio_recorder.set_recording_enabled(self.config.feeder.enable_feeder)

    def set_recording_enabled(self, enabled: bool):
        """Globally toggle call audio recording on or off without disrupting live speaker monitoring."""
        self.config.feeder.enable_feeder = enabled
        self.feeder.config.enable_feeder = enabled
        self.audio_recorder.set_recording_enabled(enabled)
        logger.info(f"Master call audio recording toggled: {'ON' if enabled else 'OFF'}")

    def _on_call_completed(self, scanner_id: str, status: ScannerStatus, pcm_bytes: bytes, duration: float, sample_rate: Optional[int] = None):
        """Called by audio recorder when a physical radio transmission completes"""
        # Offload resampling, MP3/WAV encoding, and disk/webhook dispatch to background worker pool
        # to ensure the real-time asyncio scanner poll loop is never blocked
        self._dispatch_executor.submit(self._dispatch_worker, scanner_id, status, pcm_bytes, duration, sample_rate)

    def _dispatch_worker(self, scanner_id: str, status: ScannerStatus, pcm_bytes: bytes, duration: float, sample_rate: Optional[int] = None):
        """Background thread worker for call encoding and ScanScribe dispatch"""
        try:
            if sample_rate:
                in_rate = sample_rate
            else:
                stream = self.audio_recorder.streams.get(scanner_id) if hasattr(self.audio_recorder, "streams") else None
                in_rate = stream.sample_rate if (stream and getattr(stream, "sample_rate", None)) else getattr(self.audio_recorder, "sample_rate", 44100)
            target_rate = self.config.feeder.sample_rate or 16000
            if target_rate not in ALLOWED_SAMPLE_RATES:
                logger.warning(f"feeder.sample_rate {target_rate} not in supported rates {ALLOWED_SAMPLE_RATES}. Falling back to 16000.")
                target_rate = 16000
            dispatch_pcm = resample_pcm_int16(pcm_bytes, in_rate, target_rate)

            sc_cfg = self.config.scanner_a if scanner_id == "scanner_a" else self.config.scanner_b
            payload = CallTransmissionPayload(
                scanner_id=scanner_id,
                scanner_model=status.model,
                system_name=status.system_name,
                department_name=status.dept_name,
                channel_name=status.channel_name,
                tgid=status.tgid,
                frequency=status.frequency,
                modulation=status.modulation,
                rssi=status.rssi,
                timestamp=time.time(),
                duration_seconds=duration,
                audio_pcm=dispatch_pcm,
                filename_template=getattr(sc_cfg, "filename_template", None),
                tit2_template=getattr(sc_cfg, "tit2_template", None)
            )
            res = self.feeder.dispatch_call(payload)
            logger.info(f"ScanScribe Audio Dispatch [{scanner_id}]: {res.get('status')} -> {res.get('audio_file', '')}")
        except Exception as e:
            logger.error(f"Error during async audio dispatch [{scanner_id}]: {e}", exc_info=True)

    def initialize_drivers(self):
        """Instantiate scanner drivers for any of the 34 supported Uniden models"""
        logger.info(f"Initializing Drivers: Scanner A ({self.config.scanner_a.model.value}) & Scanner B ({self.config.scanner_b.model.value})")
        self.scanner_a = create_driver_for_model(self.config.scanner_a)
        self.scanner_b = create_driver_for_model(self.config.scanner_b)

    async def start(self):
        self._running = True
        self.audio_recorder.start()
        await self.scanner_a.connect()
        await self.scanner_b.connect()
        self._loop_task = asyncio.create_task(self._poll_loop())
        logger.info("SyncManager loop and Audio Recorder started")

    async def stop(self):
        self._running = False
        self.audio_recorder.stop()
        if self._loop_task:
            self._loop_task.cancel()
            try:
                await self._loop_task
            except asyncio.CancelledError:
                pass
        if self.scanner_a:
            await self.scanner_a.disconnect()
        if self.scanner_b:
            await self.scanner_b.disconnect()
        self._dispatch_executor.shutdown(wait=False)
        logger.info("SyncManager stopped")

    def update_config(self, new_config: AppConfig):
        self.config = new_config
        self.feeder.update_config(new_config.feeder)
        self.feeder.update_scanner_configs({"scanner_a": new_config.scanner_a, "scanner_b": new_config.scanner_b})
        self.audio_recorder.hang_time_seconds = new_config.sync.hang_time_seconds
        self._configure_audio_streams()
        self.initialize_drivers()

    def log_event(self, event: ExclusionEvent):
        self.event_log.append(event)
        if len(self.event_log) > 100:
            self.event_log.pop(0)
        logger.warning(event.to_dict()["message"])

    def _should_exclude(self, status1: ScannerStatus, status2: ScannerStatus) -> bool:
        """Check if status2 collides with status1 based on ExclusionMode"""
        mode = self.config.sync.exclusion_mode
        
        # TGID check
        if mode in [ExclusionMode.TGID_AND_FREQ, ExclusionMode.TGID_ONLY]:
            if status1.tgid and status2.tgid and status1.tgid == status2.tgid:
                return True

        # Frequency check
        if mode in [ExclusionMode.TGID_AND_FREQ, ExclusionMode.FREQ_ONLY]:
            if status1.frequency and status2.frequency and abs(status1.frequency - status2.frequency) < 0.001:
                return True

        return False

    async def _poll_loop(self):
        while self._running:
            try:
                # 1. Poll both scanners concurrently
                status_a, status_b = await asyncio.gather(
                    self.scanner_a.poll(),
                    self.scanner_b.poll(),
                    return_exceptions=True
                )

                if isinstance(status_a, Exception) or isinstance(status_b, Exception):
                    await asyncio.sleep(0.2)
                    continue

                now = time.time()
                currently_excluded: set = set()

                # 1. Cleanup expired active exclusions
                expired_keys = [k for k, v in self.active_exclusions.items() if now > v["release_at"]]
                for k in expired_keys:
                    logger.info(f"Exclusion expired for {k}")
                    del self.active_exclusions[k]

                # 2. Process Mutual Exclusion Logic if Sync Enabled
                if self.config.sync.enable_sync:
                    # Case A: Both scanners landed on active calls
                    if status_a.receiving and status_b.receiving:
                        if self._should_exclude(status_a, status_b):
                            target_skip = None
                            primary_holder = None
                            victim_status = None
                            
                            p_mode = self.config.sync.priority_mode
                            if p_mode == PriorityMode.PRIMARY_SECONDARY:
                                if self.config.sync.primary_scanner_id == "scanner_a":
                                    target_skip = self.scanner_b
                                    primary_holder = "scanner_a"
                                    victim_status = status_b
                                else:
                                    target_skip = self.scanner_a
                                    primary_holder = "scanner_b"
                                    victim_status = status_a
                            else:
                                target_skip = self.scanner_b
                                primary_holder = "scanner_a"
                                victim_status = status_b

                            if target_skip:
                                target_id = target_skip.config.id
                                currently_excluded.add(target_id)
                                self.audio_recorder.abort_session(target_id)

                                if now - self._last_skip_time.get(target_id, 0.0) >= 1.0:
                                    self._last_skip_time[target_id] = now
                                    await target_skip.send_skip()
                                    self.log_event(ExclusionEvent(
                                        primary_id=primary_holder,
                                        target_id=target_id,
                                        tgid=victim_status.tgid,
                                        freq=victim_status.frequency,
                                        channel=victim_status.channel_name,
                                        reason=f"Mutual exclusion conflict under {p_mode.value}"
                                    ))

                    # Case B: Scanner B landed on a call held in active_exclusions by Scanner A
                    elif status_b.receiving and (status_b.tgid or status_b.frequency):
                        key = status_b.tgid or str(status_b.frequency)
                        if key in self.active_exclusions and self.active_exclusions[key]["holder"] == "scanner_a":
                            target_id = "scanner_b"
                            currently_excluded.add(target_id)
                            self.audio_recorder.abort_session(target_id)

                            if now - self._last_skip_time.get(target_id, 0.0) >= 1.0:
                                self._last_skip_time[target_id] = now
                                logger.info(f"Scanner B collided with active exclusion hold on {key} (held by Scanner A)")
                                await self.scanner_b.send_skip()
                                self.log_event(ExclusionEvent(
                                    primary_id="scanner_a",
                                    target_id="scanner_b",
                                    tgid=status_b.tgid,
                                    freq=status_b.frequency,
                                    channel=status_b.channel_name,
                                    reason="TGID active in Scanner A exclusion registry"
                                ))

                    # Case C: Scanner A landed on a call held in active_exclusions by Scanner B
                    elif status_a.receiving and (status_a.tgid or status_a.frequency):
                        key = status_a.tgid or str(status_a.frequency)
                        if key in self.active_exclusions and self.active_exclusions[key]["holder"] == "scanner_b":
                            if self.config.sync.priority_mode == PriorityMode.PRIMARY_SECONDARY and self.config.sync.primary_scanner_id == "scanner_a":
                                target_id = "scanner_b"
                                currently_excluded.add(target_id)
                                self.audio_recorder.abort_session(target_id)

                                if now - self._last_skip_time.get(target_id, 0.0) >= 1.0:
                                    self._last_skip_time[target_id] = now
                                    logger.info(f"Scanner A (Primary) pre-empted Scanner B on hold {key}")
                                    await self.scanner_b.send_skip()
                                    self.log_event(ExclusionEvent(
                                        primary_id="scanner_a",
                                        target_id="scanner_b",
                                        tgid=status_a.tgid,
                                        freq=status_a.frequency,
                                        channel=status_a.channel_name,
                                        reason="Scanner A (Primary) pre-empted Scanner B"
                                    ))
                            else:
                                target_id = "scanner_a"
                                currently_excluded.add(target_id)
                                self.audio_recorder.abort_session(target_id)

                                if now - self._last_skip_time.get(target_id, 0.0) >= 1.0:
                                    self._last_skip_time[target_id] = now
                                    logger.info(f"Scanner A collided with active exclusion hold on {key} (held by Scanner B)")
                                    await self.scanner_a.send_skip()
                                    self.log_event(ExclusionEvent(
                                        primary_id="scanner_b",
                                        target_id="scanner_a",
                                        tgid=status_a.tgid,
                                        freq=status_a.frequency,
                                        channel=status_a.channel_name,
                                        reason="TGID active in Scanner B exclusion registry"
                                    ))

                # 3. Trigger audio recording state machine based on serial squelch (suppressed if excluded)
                if status_a.receiving and "scanner_a" not in currently_excluded:
                    self.audio_recorder.on_squelch_open("scanner_a", status_a)
                elif not status_a.receiving and self._prev_receiving_a:
                    self.audio_recorder.on_squelch_close("scanner_a", status_a)
                self._prev_receiving_a = status_a.receiving and ("scanner_a" not in currently_excluded)

                if status_b.receiving and "scanner_b" not in currently_excluded:
                    self.audio_recorder.on_squelch_open("scanner_b", status_b)
                elif not status_b.receiving and self._prev_receiving_b:
                    self.audio_recorder.on_squelch_close("scanner_b", status_b)
                self._prev_receiving_b = status_b.receiving and ("scanner_b" not in currently_excluded)

                # 4. Process hang time expirations
                self.audio_recorder.check_hang_time_expirations()

                # 5. Sync audio playback loopback mute: mute if currently excluded or avoided
                self.audio_recorder.mute_scanner("scanner_a", ("scanner_a" in currently_excluded) or status_a.avoided)
                self.audio_recorder.mute_scanner("scanner_b", ("scanner_b" in currently_excluded) or status_b.avoided)

                # 6. Update exclusion holds for currently active non-excluded receiving scanner
                if status_a.receiving and "scanner_a" not in currently_excluded and (status_a.tgid or status_a.frequency):
                    key = status_a.tgid or str(status_a.frequency)
                    self.active_exclusions[key] = {
                        "holder": "scanner_a",
                        "release_at": now + self.config.sync.hang_time_seconds,
                        "channel": status_a.channel_name
                    }

                if status_b.receiving and "scanner_b" not in currently_excluded and (status_b.tgid or status_b.frequency):
                    key = status_b.tgid or str(status_b.frequency)
                    self.active_exclusions[key] = {
                        "holder": "scanner_b",
                        "release_at": now + self.config.sync.hang_time_seconds,
                        "channel": status_b.channel_name
                    }

                # 5. Broadcast live state via WebSockets if callback set
                if self.ws_broadcast_callback:
                    await self.ws_broadcast_callback(self.get_full_state())

            except Exception as e:
                logger.error(f"Error in SyncManager loop: {e}", exc_info=True)

            poll_ms = min(self.config.scanner_a.poll_interval_ms, self.config.scanner_b.poll_interval_ms)
            await asyncio.sleep(poll_ms / 1000.0)

    def get_full_state(self) -> Dict[str, Any]:
        return {
            "scanner_a": self.scanner_a.get_status_dict() if self.scanner_a else {},
            "scanner_b": self.scanner_b.get_status_dict() if self.scanner_b else {},
            "config": self.config.model_dump(),
            "active_exclusions": self.active_exclusions,
            "event_log": [e.to_dict() for e in self.event_log[-20:]],
            "audio_levels": self.audio_recorder.get_levels() if self.audio_recorder else {},
            "audio_level": self.audio_recorder.live_level_pct if self.audio_recorder else 0
        }
