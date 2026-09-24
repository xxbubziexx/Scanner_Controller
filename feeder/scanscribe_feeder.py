import os
import re
import json
import time
import datetime
import logging
import urllib.request
import urllib.parse
from typing import Dict, Any, Optional, Tuple
from pydantic import BaseModel
from config import ScanScribeFeederConfig, FeederMode
from drivers.base_driver import normalize_frequency
from metadata.proscan_metadata import (
    ProScanMetadata,
    write_mp3_id3v23,
    write_wav_riff_info,
    format_template,
    format_streaming_metadata,
    sanitize_filename,
    sanitize_folder_path,
    resolve_collision_filename
)

logger = logging.getLogger("ScanScribeFeeder")

class CallTransmissionPayload(BaseModel):
    scanner_id: str
    scanner_model: str = "BCD436HP"
    system_name: str = "Trunk System"
    department_name: str = "Dispatch"
    channel_name: str = "Channel 1"
    tgid: Optional[str] = None
    frequency: Optional[float] = None
    modulation: str = "NFM"
    rssi: int = 0
    timestamp: float = 0.0
    duration_seconds: float = 0.0
    audio_pcm: Optional[bytes] = None

class ScanScribeFeeder:
    """
    Feeder module that packages completed radio transmissions with ProScan metadata
    and dispatches them directly to the ScanScribe audio transcription pipeline.
    """

    def __init__(self, config: ScanScribeFeederConfig):
        self.config: ScanScribeFeederConfig = config

    def update_config(self, new_config: ScanScribeFeederConfig):
        self.config = new_config

    def _build_proscan_metadata(self, payload: CallTransmissionPayload) -> ProScanMetadata:
        dt = datetime.datetime.fromtimestamp(payload.timestamp or time.time())
        ending_date = dt.strftime("%Y%m%d%H%M%S")

        norm_freq = normalize_frequency(payload.frequency) if payload.frequency else None
        chan = (payload.channel_name or "").strip()
        sys_name = (payload.system_name or "").strip()
        dept_name = (payload.department_name or "").strip()
        tgid_val = (payload.tgid or "").strip()
        freq_str = f"{norm_freq:.4f}" if norm_freq else ""

        # Fallback if channel name is generic or empty
        if not chan or chan.lower() in ("scanning", "scanning...", "active call", "channel", "unknown channel", "active channel"):
            if norm_freq:
                chan = f"{norm_freq:.4f} MHz"
            elif tgid_val:
                chan = f"TGID {tgid_val}"

        return ProScanMetadata(
            scanner=payload.scanner_model or "Scanner",
            ending_date=ending_date,
            system_name=sys_name,
            department_name=dept_name,
            channel_name=chan,
            frequency=freq_str,
            modulation=payload.modulation or "NFM",
            tgid=tgid_val,
            rssi=str(payload.rssi) if payload.rssi is not None else ""
        )

    def _generate_audio_bytes(self, payload: CallTransmissionPayload, meta: ProScanMetadata) -> Tuple[bytes, str]:
        """Generate ProScan-tagged audio bytes (MP3 or WAV) and return (bytes, file_extension)"""
        pcm = payload.audio_pcm if payload.audio_pcm else (b"\x00\x00" * 8000 * 3)
        requested_ext = self.config.audio_format.lower()
        sample_rate = self.config.sample_rate or 16000

        # Check if payload is already encoded MP3
        is_mp3_payload = pcm.startswith(b"ID3") or (len(pcm) > 2 and pcm[0] == 0xFF and (pcm[1] & 0xE0) == 0xE0)

        tit2_template = getattr(self.config, "tit2_template", None) or self.config.filename_template or "%C (%TG)"

        if requested_ext == "mp3":
            if is_mp3_payload:
                audio_data = write_mp3_id3v23(pcm, meta, title_template=tit2_template, artist_template="%S")
                return audio_data, "mp3"
            else:
                try:
                    import lameenc
                    encoder = lameenc.Encoder()
                    encoder.set_bit_rate(128)
                    encoder.set_in_sample_rate(sample_rate)
                    encoder.set_channels(1)
                    encoder.set_quality(2)
                    mp3_data = encoder.encode(pcm) + encoder.flush()
                    audio_data = write_mp3_id3v23(mp3_data, meta, title_template=tit2_template, artist_template="%S")
                    return audio_data, "mp3"
                except Exception:
                    logger.warning("MP3 encoder (lameenc) unavailable for raw PCM payload; encoding to RIFF WAV audio format for ScanScribe compatibility.")
                    audio_data = write_wav_riff_info(pcm, meta, title_template=tit2_template, artist_template="%S", sample_rate=sample_rate)
                    return audio_data, "wav"
        else:
            audio_data = write_wav_riff_info(pcm, meta, title_template=tit2_template, artist_template="%S", sample_rate=sample_rate)
            return audio_data, "wav"

    def _resolve_inbox_directory(self) -> str:
        """
        Cleans and resolves the target inbox directory.
        Handles bare drive roots (e.g. A:\\) by pointing into Scanscribe's inbox or a recordings subfolder.
        """
        raw = (self.config.inbox_directory or "").strip("\"' \t\r\n")
        if not raw:
            raw = os.path.join(os.getcwd(), "recordings")

        target = os.path.abspath(raw)
        drive, rest = os.path.splitdrive(target)
        if rest in ["\\", "/", ""]:
            # If pointing at root drive (e.g. A:\ or C:\), avoid dumping files to root directly
            candidate = os.path.join(target, "Scanscribe_Projects", "scanscribe", "inbox")
            if os.path.exists(candidate):
                target = candidate
            else:
                target = os.path.join(target, "recordings")
        return target

    def dispatch_call(self, payload: CallTransmissionPayload) -> Dict[str, Any]:
        """Dispatches a completed radio call to ScanScribe based on configured FeederMode"""
        if not self.config.enable_feeder:
            return {"status": "disabled", "message": "ScanScribe Feeder is disabled in config"}

        if not payload.timestamp:
            payload.timestamp = time.time()

        if payload.frequency:
            payload.frequency = normalize_frequency(payload.frequency)

        dt = datetime.datetime.fromtimestamp(payload.timestamp)

        meta = self._build_proscan_metadata(payload)
        template = self.config.filename_template if self.config.filename_template else "%DT - %S - %C (%TG)"
        formatted_stem = format_template(template, meta, dt)
        filename_base = sanitize_filename(formatted_stem)

        # Mode 1: Directory Drop Feeder
        if self.config.feeder_mode == FeederMode.DIRECTORY_DROP:
            target_dir = self._resolve_inbox_directory()
            audio_bytes, ext = self._generate_audio_bytes(payload, meta)
            final_filename = resolve_collision_filename(target_dir, filename_base, ext)

            chan_name = meta.channel_name
            sys_name = meta.system_name
            dept_name = meta.department_name
            tg_val = meta.tgid if meta.tgid else None
            tg_num = int(meta.tgid) if (meta.tgid and meta.tgid.isdigit()) else None

            sidecar_data = {
                "timestamp": payload.timestamp,
                "datetime": dt.strftime("%Y-%m-%d %H:%M:%S"),
                "scanner_id": payload.scanner_id,
                "scanner_model": payload.scanner_model,
                "system_name": sys_name,
                "department_name": dept_name,
                "channel_name": chan_name,
                "tgid": tg_val,
                "talkgroup": tg_val,
                "talkgroup_id": tg_val,
                "talkgroup_num": tg_num,
                "talkgroup_tag": chan_name,
                "talkgroup_name": chan_name,
                "talkgroup_group": dept_name,
                "group_name": dept_name,
                "frequency": payload.frequency,
                "frequency_mhz": f"{payload.frequency:.4f}" if payload.frequency else "",
                "modulation": payload.modulation,
                "rssi": payload.rssi,
                "duration": payload.duration_seconds,
                "duration_seconds": payload.duration_seconds,
                "audio_filename": f"{final_filename}.{ext}",
                "proscan_metadata": meta.to_dict()
            }

            try:
                os.makedirs(target_dir, exist_ok=True)
                audio_file_path = os.path.join(target_dir, f"{final_filename}.{ext}")
                sidecar_path = os.path.join(target_dir, f"{final_filename}.json")

                with open(audio_file_path, "wb") as f:
                    f.write(audio_bytes)

                if self.config.enable_sidecar_json:
                    with open(sidecar_path, "w", encoding="utf-8") as f:
                        json.dump(sidecar_data, f, indent=2)

                logger.info(f"ScanScribe Feeder: Dropped audio call to inbox {audio_file_path}")
                return {
                    "status": "success",
                    "mode": "DIRECTORY_DROP",
                    "audio_file": audio_file_path,
                    "sidecar_file": sidecar_path
                }
            except Exception as e:
                logger.error(f"ScanScribe Feeder directory drop error on '{target_dir}': {e}. Attempting local fallback.")
                try:
                    fallback_dir = os.path.join(os.getcwd(), "recordings")
                    os.makedirs(fallback_dir, exist_ok=True)
                    fallback_final = resolve_collision_filename(fallback_dir, filename_base, ext)
                    fallback_audio = os.path.join(fallback_dir, f"{fallback_final}.{ext}")
                    fallback_sidecar = os.path.join(fallback_dir, f"{fallback_final}.json")

                    with open(fallback_audio, "wb") as f:
                        f.write(audio_bytes)

                    if self.config.enable_sidecar_json:
                        with open(fallback_sidecar, "w", encoding="utf-8") as f:
                            json.dump(sidecar_data, f, indent=2)

                    logger.warning(f"ScanScribe Feeder: Safely saved audio call to fallback inbox: {fallback_audio}")
                    return {
                        "status": "success",
                        "mode": "DIRECTORY_DROP_FALLBACK",
                        "audio_file": fallback_audio,
                        "sidecar_file": fallback_sidecar,
                        "warning": f"Primary inbox failed ({e}); saved to fallback."
                    }
                except Exception as fb_err:
                    logger.error(f"ScanScribe Feeder fallback also failed: {fb_err}")
                    return {"status": "error", "error": f"{e} (fallback: {fb_err})"}

        # Mode 2: HTTP Webhook Ingest Feeder
        elif self.config.feeder_mode == FeederMode.HTTP_WEBHOOK:
            try:
                audio_bytes, ext = self._generate_audio_bytes(payload, meta)
                chan_name = meta.channel_name
                sys_name = meta.system_name
                dept_name = meta.department_name
                tg_val = meta.tgid if meta.tgid else None
                tg_num = int(meta.tgid) if (meta.tgid and meta.tgid.isdigit()) else None

                sidecar_data = {
                    "timestamp": payload.timestamp,
                    "datetime": dt.strftime("%Y-%m-%d %H:%M:%S"),
                    "scanner_id": payload.scanner_id,
                    "scanner_model": payload.scanner_model,
                    "system_name": sys_name,
                    "department_name": dept_name,
                    "channel_name": chan_name,
                    "tgid": tg_val,
                    "talkgroup": tg_val,
                    "talkgroup_id": tg_val,
                    "talkgroup_num": tg_num,
                    "talkgroup_tag": chan_name,
                    "talkgroup_name": chan_name,
                    "talkgroup_group": dept_name,
                    "group_name": dept_name,
                    "frequency": payload.frequency,
                    "frequency_mhz": f"{payload.frequency:.4f}" if payload.frequency else "",
                    "duration": payload.duration_seconds,
                    "duration_seconds": payload.duration_seconds
                }

                # Construct multipart HTTP request
                boundary = f"----ScanScribeBoundary{int(time.time())}"
                body_parts = []
                
                # Add JSON metadata part
                body_parts.append(f"--{boundary}\r\n".encode("utf-8"))
                body_parts.append(b'Content-Disposition: form-data; name="metadata"\r\nContent-Type: application/json\r\n\r\n')
                body_parts.append(json.dumps(sidecar_data).encode("utf-8") + b"\r\n")

                # Add Audio file part
                body_parts.append(f"--{boundary}\r\n".encode("utf-8"))
                body_parts.append(f'Content-Disposition: form-data; name="file"; filename="{filename_base}.{ext}"\r\nContent-Type: audio/{ext}\r\n\r\n'.encode("utf-8"))
                body_parts.append(audio_bytes + b"\r\n")
                body_parts.append(f"--{boundary}--\r\n".encode("utf-8"))

                body_data = b"".join(body_parts)
                req = urllib.request.Request(
                    self.config.webhook_url,
                    data=body_data,
                    headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
                    method="POST"
                )

                # Attempt webhook POST
                with urllib.request.urlopen(req, timeout=3.0) as resp:
                    resp_data = resp.read().decode("utf-8", errors="ignore")
                    logger.info(f"ScanScribe Feeder: Posted call to Webhook {self.config.webhook_url}")
                    return {"status": "success", "mode": "HTTP_WEBHOOK", "response": resp_data}

            except Exception as e:
                logger.warning(f"ScanScribe Feeder HTTP Webhook dispatch error (ScanScribe server may be offline): {e}")
                return {"status": "error", "mode": "HTTP_WEBHOOK", "error": str(e)}

        return {"status": "ignored", "mode": self.config.feeder_mode.value}
