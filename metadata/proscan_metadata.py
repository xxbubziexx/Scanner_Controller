import os
import struct
import datetime
import re
from dataclasses import dataclass, field
from typing import Dict, Any, Optional, Tuple, List

@dataclass
class ProScanMetadata:
    """
    ProScan Transmission and Scanner Metadata Record.
    Reverse-engineered from ProScan.exe (FileRecording.cs, General.cs, Common_PS_RF.cs).
    Maintains exact 22-field payload sequence and fixed-width padding for binary compatibility.
    """
    scanner: str = "BCD996P2"
    ending_date: str = "00000000000000"  # yyyyMMddHHmmss (14 chars)
    favorite_name: str = ""
    system_name: str = ""
    site_name: str = ""
    department_name: str = ""  # Corresponds to GroupName in ProScan (%G, %B, %DE)
    channel_name: str = ""     # Corresponds to ChannelName (%C, %L1)
    frequency: str = ""        # Corresponds to Frequency (%F)
    modulation: str = "NFM"    # Corresponds to Modulation (%MD)
    tone: str = ""             # Padded right to 24 chars in pros chunk (%T)
    tgid: str = ""             # TalkGroup (%TG)
    uid: str = ""              # Padded right to 64 chars in pros chunk (%U1)
    uid_number: str = ""       # Padded right to 16 chars in pros chunk (%U2, %U3)
    ftoa: str = ""             # FTOA (%TA)
    ftob: str = ""             # FTOB (%TB)
    rssi: str = ""             # RSSI (%R)
    alert_color: str = ""      # AlertColor (%AC)
    service_type: str = ""     # ServiceType (%ST1)
    service_color: str = ""    # ServiceColor (%STC)
    displayed_system_type: str = ""
    digital_status: str = ""
    dmr_slot: str = ""         # Concatenated to frequency in %F as S1 or S2; '/' replaced with 'S'
    scanner_mode: str = "Scan" # ScannerMode in pros chunk (up to first comma)
    info_sc_state: str = ""    # Feeds %SC
    comm_port: str = "5"       # Feeds %P (bare digits or port name)
    program_path: str = ""     # Feeds %AF1
    program_filename: str = "" # Feeds %AF2

    def to_dict(self) -> Dict[str, str]:
        return {
            "Scanner": self.scanner,
            "EndingDate": self.ending_date,
            "FavoriteName": self.favorite_name,
            "SystemName": self.system_name,
            "SiteName": self.site_name,
            "DepartmentName": self.department_name,
            "ChannelName": self.channel_name,
            "Frequency": self.frequency,
            "Modulation": self.modulation,
            "Tone": self.tone,
            "TGID": self.tgid,
            "UID": self.uid,
            "UID#": self.uid_number,
            "FTOA": self.ftoa,
            "FTOB": self.ftob,
            "RSSI": self.rssi,
            "AlertColor": self.alert_color,
            "ServiceType": self.service_type,
            "DisplayedSystemType": self.displayed_system_type,
            "DigitalStatus": self.digital_status,
            "DMRSlot": self.dmr_slot,
            "ScannerMode": self.scanner_mode
        }


def clean_ascii_1(text: str, max_len: int = 253) -> str:
    """
    ProScan General.CleanASCII_1 (General.cs:35949):
    Filters text to printable ASCII (Asc > 31 and Asc < 127), trims, and truncates to max_len chars.
    Used for ID3 and WAV tags.
    """
    if not text:
        return ""
    filtered = "".join(c for c in text if 31 < ord(c) < 127).strip()
    return filtered[:max_len]


def build_pros_chunk(meta: ProScanMetadata) -> bytes:
    """
    Builds the binary 'pros' scanner record chunk according to ProScan specifications (FileRecording.cs:459).
    Framing: 'pros' (4 ASCII bytes) + 4-byte uint32 little-endian payload length + payload.
    Payload consists of 22 key:value strings in exact order, separated by NUL (0x00).
    Fixed-width padded fields: Tone (24), UID (64), UID# (16).
    Length excludes the 8 header bytes, but INCLUDES the 0x00 pad byte if payload is odd.
    """
    ending_date_str = (meta.ending_date or "00000000000000").strip().ljust(14, '0')[:14]
    dmr_slot_str = meta.dmr_slot.strip().replace("/", "S") if meta.dmr_slot else ""
    scanner_mode_str = meta.scanner_mode.strip().split(",")[0]

    # 22 Key:Value payload items in exact ProScan sequence (RECORDING-METADATA-FORMAT.md §1)
    field_items = [
        ("Scanner:", meta.scanner.strip()),
        ("EndingDate:", ending_date_str),
        ("FavoriteName:", meta.favorite_name.strip()),
        ("SystemName:", meta.system_name.strip()),
        ("SiteName:", meta.site_name.strip()),
        ("DepartmentName:", meta.department_name.strip()),
        ("ChannelName:", meta.channel_name.strip()),
        ("Frequency:", meta.frequency.strip()),
        ("Modulation:", meta.modulation.strip()),
        ("Tone:", meta.tone.ljust(24)),
        ("TGID:", meta.tgid.strip()),
        ("UID:", meta.uid.ljust(64)),
        ("UID#:", meta.uid_number.ljust(16)),
        ("FTOA:", meta.ftoa.strip()),
        ("FTOB:", meta.ftob.strip()),
        ("RSSI:", str(meta.rssi).strip()),
        ("AlertColor:", meta.alert_color.strip()),
        ("ServiceType:", meta.service_type.strip()),
        ("DisplayedSystemType:", meta.displayed_system_type.strip()),
        ("DigitalStatus:", meta.digital_status.strip()),
        ("DMRSlot:", dmr_slot_str),
        ("ScannerMode:", scanner_mode_str)
    ]

    payload_parts = []
    for key, val in field_items:
        entry_str = f"{key}{val}\0"
        payload_parts.append(entry_str.encode("ascii", errors="ignore"))

    payload = b"".join(payload_parts)

    # If payload length is odd, append 0x00 pad byte and include it in length (FileRecording.cs:496-501)
    if len(payload) % 2 != 0:
        payload += b"\x00"

    payload_len = len(payload)
    header = b"pros" + struct.pack("<I", payload_len)
    return header + payload


def parse_pros_chunk(data: bytes) -> Optional[Tuple[ProScanMetadata, int]]:
    """
    Parses a binary blob containing a 'pros' chunk.
    Returns (ProScanMetadata, bytes_consumed) or None if 'pros' chunk is not found.
    """
    idx = data.find(b"pros")
    if idx == -1 or len(data) < idx + 8:
        return None

    payload_len = struct.unpack("<I", data[idx+4:idx+8])[0]
    start = idx + 8
    end = start + payload_len

    if len(data) < end:
        return None

    payload = data[start:end]
    raw_entries = payload.split(b"\x00")

    meta_dict = {}
    for raw in raw_entries:
        if not raw:
            continue
        try:
            entry = raw.decode("ascii", errors="ignore")
            if ":" in entry:
                k, v = entry.split(":", 1)
                meta_dict[k.strip()] = v
        except Exception:
            pass

    meta = ProScanMetadata(
        scanner=meta_dict.get("Scanner", "").strip(),
        ending_date=meta_dict.get("EndingDate", "").strip(),
        favorite_name=meta_dict.get("FavoriteName", "").strip(),
        system_name=meta_dict.get("SystemName", "").strip(),
        site_name=meta_dict.get("SiteName", "").strip(),
        department_name=meta_dict.get("DepartmentName", "").strip(),
        channel_name=meta_dict.get("ChannelName", "").strip(),
        frequency=meta_dict.get("Frequency", "").strip(),
        modulation=meta_dict.get("Modulation", "").strip(),
        tone=meta_dict.get("Tone", "").rstrip(),
        tgid=meta_dict.get("TGID", "").strip(),
        uid=meta_dict.get("UID", "").rstrip(),
        uid_number=meta_dict.get("UID#", "").rstrip(),
        ftoa=meta_dict.get("FTOA", "").strip(),
        ftob=meta_dict.get("FTOB", "").strip(),
        rssi=meta_dict.get("RSSI", "").strip(),
        alert_color=meta_dict.get("AlertColor", "").strip(),
        service_type=meta_dict.get("ServiceType", "").strip(),
        displayed_system_type=meta_dict.get("DisplayedSystemType", "").strip(),
        digital_status=meta_dict.get("DigitalStatus", "").strip(),
        dmr_slot=meta_dict.get("DMRSlot", "").strip(),
        scanner_mode=meta_dict.get("ScannerMode", "").strip()
    )

    bytes_consumed = (end - idx)
    return meta, bytes_consumed


def sanitize_filename(filename: str) -> str:
    """
    ProScan FilterFileNamesLegalCharactersOnly (Recorder.cs:1220, General.cs:34069):
    1. Replaces '/' and ':' with '-'
    2. Character allowlist: ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz1234567890!@#$%^&()-_+={}[];,. space
    3. Trims whitespace
    4. Strips trailing dots
    5. Fallback: 'No Custom Formatters Specified'
    """
    path = (filename or "").replace("/", "-").replace("\\", "-").replace(":", "-")
    # Allowlists characters strictly matching General.cs:34069
    path = re.sub(r'[^A-Za-z0-9!@#$%^&()\-_+={}\[\];,. ]', '', path).strip()
    while path.endswith("."):
        path = path[:-1]
    if not path:
        path = "No Custom Formatters Specified"
    return path


def sanitize_folder_path(folder_path: str) -> str:
    """
    ProScan FilterFolderLegalCharactersOnly (Recorder.cs:1204, General.cs:34031):
    1. Replaces '/' with '-'
    2. Applies folder allowlist (permits '\\' path separators)
    3. Drive-letter guard: protects first 3 characters ('C:\\'), then turns colons to '-'
    4. Strips trailing dots
    5. Ensures trailing '\\'
    """
    raw = (folder_path or "").replace("/", "-")
    # Folder allowlist permits '\\'
    cleaned = re.sub(r'[^A-Za-z0-9!@#$%^&()\-_+={}\[\];,. \\\:]', '', raw).strip()
    
    # Drive-letter guard (Strings.Mid(Path, 1, 3) + Strings.Mid(Path, 4).Replace(":", "-"))
    if len(cleaned) >= 3 and cleaned[1:3] in (":\\", ":/"):
        path = cleaned[:3] + cleaned[3:].replace(":", "-")
    else:
        path = cleaned.replace(":", "-")

    while path.endswith("."):
        path = path[:-1]
    if not path.endswith("\\") and not path.endswith("/"):
        path += "\\"
    return path


def resolve_collision_filename(directory: str, base_name: str, ext: str) -> str:
    """
    Resolves filename collisions using the ProScan ' #num' convention,
    but placing the suffix BEFORE the file extension (fixing ProScan's dot-in-frequency bug).
    """
    full_path = os.path.join(directory, f"{base_name}.{ext}")
    if not os.path.exists(full_path):
        return base_name

    num = 2
    while True:
        candidate = f"{base_name} #{num}"
        cand_path = os.path.join(directory, f"{candidate}.{ext}")
        if not os.path.exists(cand_path):
            return candidate
        num += 1


def format_template(
    template_str: str,
    meta: ProScanMetadata,
    now: Optional[datetime.datetime] = None,
    month_day_year_format: str = "MM/dd/yy",
    military_time: bool = False
) -> str:
    """
    Engine A: ProScan General.GetFormatters() (General.cs:33805).
    Drives filenames, folder paths, window caption, and recording tags (TIT2, TPE1, TPE2, INAM, IART).
    Implements all 37 specifiers in the exact sequential order verified by reverse engineering.
    Preserves longest-first ordering and counterintuitive inversions:
      %MM = Month Name (e.g. Sep), %M = Month Number (e.g. 09)
      %YY = 4-digit Year (e.g. 2026), %Y = 2-digit Year (e.g. 26)
      %D = Full Date (e.g. 09/24/26), %DD = 2-digit Day (e.g. 24)
    """
    if not template_str:
        return ""
    if not now:
        now = datetime.datetime.now()

    # Time formatting (Common_PS_RF.cs:346)
    if military_time:
        time_str = now.strftime("%H:%M:%S")
    else:
        # 12-hour with AM/PM (h:mm:ss tt)
        time_str = now.strftime("%I:%M:%S %p").lstrip("0")

    # Date formatting (MonthDayYearFormat)
    if "yyyy" in month_day_year_format or "%Y" in month_day_year_format:
        date_str = now.strftime("%m/%d/%Y") if "/" in month_day_year_format else now.strftime("%m-%d-%Y")
    else:
        date_str = now.strftime("%m/%d/%y") if "/" in month_day_year_format else now.strftime("%m-%d-%y")

    # Full datetime (%DT = date + single space + time)
    datetime_str = f"{date_str} {time_str}"

    dmr_slot_clean = meta.dmr_slot.strip().replace("/", "S") if meta.dmr_slot else ""
    if dmr_slot_clean and not dmr_slot_clean.startswith("S"):
        dmr_slot_suffix = "S" + dmr_slot_clean
    else:
        dmr_slot_suffix = dmr_slot_clean
    freq_with_dmr = (meta.frequency + dmr_slot_suffix) if meta.frequency else ""

    # %FT: talkgroup if set, else frequency + DMR slot; empty if neither set
    ft_val = meta.tgid if meta.tgid else freq_with_dmr

    # Clean bare comm port representation (bare digits per port doc, or port string)
    port_str = meta.comm_port
    if port_str.upper().startswith("COM") and port_str[3:].isdigit():
        port_val = port_str[3:]
    else:
        port_val = port_str

    prog_filename = meta.program_filename or (os.path.basename(meta.program_path) if meta.program_path else "")

    # Sequential replacements matching General.cs:33805 GetFormatters()
    replacements = [
        ("%FT", ft_val),
        ("%TG", meta.tgid),
        ("%L2", meta.system_name),
        ("%L1", meta.channel_name),
        ("%ST1", meta.service_type),
        ("%STC", meta.service_color),
        ("%AC", meta.alert_color),
        ("%SC", meta.info_sc_state),
        ("%ST", meta.scanner),
        ("%DD", now.strftime("%d")),
        ("%DT", datetime_str),
        ("%TT", time_str),
        ("%H", now.strftime("%H")),
        ("%MM", now.strftime("%b")),
        ("%YY", now.strftime("%Y")),
        ("%AF1", meta.program_path),
        ("%AF2", prog_filename),
        ("%FA", meta.favorite_name),
        ("%SI", meta.site_name),
        ("%S", meta.system_name),
        ("%G", meta.department_name),
        ("%B", meta.department_name),
        ("%DE", meta.department_name),
        ("%C", meta.channel_name),
        ("%F", freq_with_dmr),
        ("%TA", meta.ftoa),
        ("%TB", meta.ftob),
        ("%T", meta.tone.strip()),
        ("%MD", meta.modulation),
        ("%R", str(meta.rssi)),
        ("%U1", meta.uid.strip()),
        ("%U2", meta.uid_number.strip()),
        ("%U3", meta.uid_number.strip()),
        ("%P", port_val),
        ("%D", date_str),
        ("%M", now.strftime("%m")),
        ("%Y", now.strftime("%y")),
    ]

    res = template_str
    for spec, val in replacements:
        res = res.replace(spec, str(val) if val is not None else "")
    return res


def format_streaming_metadata(template_str: str, meta: ProScanMetadata, listener_count: int = 0) -> str:
    """
    Engine B: ProScan Common_PS_RF.cs:784 / :801.
    Applied to streamed metadata (Source Client and Web Server).
    Contains 19 specifiers, including %CL (listener count).
    Date/time specifiers are NOT supported and pass through literally.
    """
    if not template_str:
        return ""

    dmr_slot_clean = meta.dmr_slot.strip().replace("/", "S") if meta.dmr_slot else ""
    if dmr_slot_clean and not dmr_slot_clean.startswith("S"):
        dmr_slot_suffix = "S" + dmr_slot_clean
    else:
        dmr_slot_suffix = dmr_slot_clean
    freq_with_dmr = (meta.frequency + dmr_slot_suffix) if meta.frequency else ""
    ft_val = meta.tgid if meta.tgid else freq_with_dmr

    # Sequential replacements matching Common_PS_RF.cs:784/801
    replacements = [
        ("%FT", ft_val),
        ("%TG", meta.tgid),
        ("%L2", meta.system_name),
        ("%L1", meta.channel_name),
        ("%FA", meta.favorite_name),
        ("%SI", meta.site_name),
        ("%S", meta.system_name),
        ("%G", meta.department_name),
        ("%B", meta.department_name),
        ("%DE", meta.department_name),
        ("%CL", str(listener_count)),
        ("%C", meta.channel_name),
        ("%F", freq_with_dmr),
        ("%T", meta.tone.strip()),
        ("%MD", meta.modulation),
        ("%R", str(meta.rssi)),
        ("%U1", meta.uid.strip()),
        ("%U2", meta.uid_number.strip()),
        ("%U3", meta.uid_number.strip()),
    ]

    res = template_str
    for spec, val in replacements:
        res = res.replace(spec, str(val) if val is not None else "")
    return res


def _encode_syncsafe(val: int) -> bytes:
    """Encodes integer to 4-byte ID3v2 syncsafe integer (big-endian, 7 bits per byte)"""
    b1 = (val >> 21) & 0x7F
    b2 = (val >> 14) & 0x7F
    b3 = (val >> 7) & 0x7F
    b4 = val & 0x7F
    return bytes([b1, b2, b3, b4])


def _build_id3v23_text_frame(frame_id: str, text: str) -> bytes:
    """
    Builds a standard ID3v2.3 text frame with ISO-8859-1 (Latin1) encoding and NUL terminator.
    Byte layout:
      4 bytes: frame_id (ASCII)
      4 bytes: size (big-endian 32-bit uint = text_length + 2)
      2 bytes: flags (0x00 0x00)
      1 byte:  encoding (0x00)
      N bytes: text (CleanASCII_1 filtered, <= 253 chars)
      1 byte:  0x00 terminator
    """
    clean_text = clean_ascii_1(text, 253).encode("latin1", errors="ignore")
    frame_size = len(clean_text) + 2
    header = frame_id.encode("ascii") + struct.pack(">I", frame_size) + b"\x00\x00\x00"
    return header + clean_text + b"\x00"


def write_mp3_id3v23(
    audio_bytes: bytes,
    meta: ProScanMetadata,
    title_template: str = "%C",
    artist_template: str = "%S",
    band_template: str = "%G",
    now: Optional[datetime.datetime] = None
) -> bytes:
    """
    Wraps MP3 audio bytes with a ProScan-compliant ID3v2.3 header at file start (FileRecording.cs:333).
    Frames: TSEE ('ProScan'), TRDA, TIT2 (Title), TPE1 (Artist), TPE2 (Band), plus embedded 'pros' chunk.
    Only writes the tag if at least one of TIT2/TPE1/TPE2 is set.
    """
    if not now:
        now = datetime.datetime.now()

    trda_str = f"{now.strftime('%Y%m%d%H%M%S')}-00000000000000"

    title_val = format_template(title_template, meta, now) if title_template else ""
    artist_val = format_template(artist_template, meta, now) if artist_template else ""
    band_val = format_template(band_template, meta, now) if band_template else ""

    # Gated: ProScan FileRecording.cs:337 writes tag if any of TIT2/TPE1/TPE2 is non-empty
    if not (title_val.strip() or artist_val.strip() or band_val.strip()):
        return audio_bytes

    frames = [
        _build_id3v23_text_frame("TSEE", "ProScan"),
        _build_id3v23_text_frame("TRDA", trda_str),
        _build_id3v23_text_frame("TIT2", title_val),
        _build_id3v23_text_frame("TPE1", artist_val),
        _build_id3v23_text_frame("TPE2", band_val),
    ]

    tgid_str = (meta.tgid or "").strip()
    if tgid_str:
        frames.append(_build_id3v23_text_frame("TRCK", tgid_str))
        frames.append(_build_id3v23_text_frame("TCON", f"TGID {tgid_str}"))
        frames.append(_build_id3v23_text_frame("COMM", f"TGID: {tgid_str}"))

    frames.append(build_pros_chunk(meta))

    tag_payload = b"".join(frames)
    tag_size_syncsafe = _encode_syncsafe(len(tag_payload))

    id3_header = b"ID3\x03\x00\x00" + tag_size_syncsafe
    return id3_header + tag_payload + audio_bytes


def _build_riff_info_subchunk(chunk_id: str, text: str) -> bytes:
    """Builds RIFF INFO subchunk with NUL terminator and even byte alignment (FileRecording.cs:509)"""
    clean_text = clean_ascii_1(text, 253)
    raw_str = clean_text.encode("ascii", errors="ignore") + b"\x00"
    if len(raw_str) % 2 != 0:
        raw_str += b"\x00"
    length = len(raw_str)
    header = chunk_id.encode("ascii") + struct.pack("<I", length)
    return header + raw_str


def write_wav_riff_info(
    pcm_bytes: bytes,
    meta: ProScanMetadata,
    title_template: str = "%C",
    artist_template: str = "%S",
    sample_rate: int = 16000,
    channels: int = 1,
    bits_per_sample: int = 16,
    now: Optional[datetime.datetime] = None
) -> bytes:
    """
    Wraps raw PCM audio bytes into a canonical ProScan RIFF WAVE container (FileRecording.cs:403).
    Includes:
      RIFF/WAVE header
      LIST/INFO chunk with IPRD ('ProScan'), ICRD, INAM (Title), IART (Artist), and embedded 'pros' chunk
      fmt chunk (WAVEFORMATEX)
      data chunk (PCM) with accurate data length
      id3 chunk for universal player/mutagen metadata compatibility
    """
    if not now:
        now = datetime.datetime.now()

    icrd_str = f"{now.strftime('%Y%m%d%H%M%S')}-00000000000000"

    title_val = format_template(title_template, meta, now) if title_template else ""
    artist_val = format_template(artist_template, meta, now) if artist_template else ""
    tgid_str = (meta.tgid or "").strip()

    # 1. fmt chunk (WAVEFORMATEX)
    block_align = channels * (bits_per_sample // 8)
    byte_rate = sample_rate * block_align
    fmt_chunk = b"fmt \x10\x00\x00\x00" + struct.pack("<HHIIHH", 1, channels, sample_rate, byte_rate, block_align, bits_per_sample)

    # 2. data chunk
    pcm_len = len(pcm_bytes)
    data_payload = pcm_bytes
    if pcm_len % 2 != 0:
        data_payload += b"\x00"
    data_chunk = b"data" + struct.pack("<I", pcm_len) + data_payload

    # 3. RIFF LIST INFO chunk
    info_subchunks = [
        _build_riff_info_subchunk("IPRD", "ProScan"),
        _build_riff_info_subchunk("ICRD", icrd_str),
        _build_riff_info_subchunk("INAM", title_val),
        _build_riff_info_subchunk("IART", artist_val),
    ]

    if tgid_str:
        info_subchunks.append(_build_riff_info_subchunk("ICMT", f"TGID: {tgid_str}"))
        info_subchunks.append(_build_riff_info_subchunk("IGNR", tgid_str))
        info_subchunks.append(_build_riff_info_subchunk("ITRK", tgid_str))
        info_subchunks.append(_build_riff_info_subchunk("IKEY", f"TGID={tgid_str}"))
        info_subchunks.append(_build_riff_info_subchunk("ISBJ", f"Talkgroup {tgid_str} - {title_val}"))

    info_payload = b"INFO" + b"".join(info_subchunks)
    if len(info_payload) % 2 != 0:
        info_payload += b"\x00"
    list_chunk = b"LIST" + struct.pack("<I", len(info_payload)) + info_payload

    # 4. id3 chunk in WAV for Mutagen / ID3 readers
    id3_frames = [
        _build_id3v23_text_frame("TIT2", title_val),
        _build_id3v23_text_frame("TPE1", artist_val),
    ]
    if meta.department_name:
        id3_frames.append(_build_id3v23_text_frame("TPE2", meta.department_name))
    if tgid_str:
        id3_frames.append(_build_id3v23_text_frame("TRCK", tgid_str))
        id3_frames.append(_build_id3v23_text_frame("TCON", f"TGID {tgid_str}"))
        id3_frames.append(_build_id3v23_text_frame("COMM", f"TGID: {tgid_str}"))

    id3_tag_payload = b"".join(id3_frames)
    id3_full = b"ID3\x03\x00\x00" + _encode_syncsafe(len(id3_tag_payload)) + id3_tag_payload
    if len(id3_full) % 2 != 0:
        id3_full += b"\x00"
    id3_chunk = b"id3 " + struct.pack("<I", len(id3_full)) + id3_full

    # 5. ProScan binary scanner record chunk (pros)
    pros_chunk = build_pros_chunk(meta)

    # Assemble canonical RIFF WAVE file: fmt -> data -> LIST -> id3 -> pros
    riff_payload = b"WAVE" + fmt_chunk + data_chunk + list_chunk + id3_chunk + pros_chunk
    riff_header = b"RIFF" + struct.pack("<I", len(riff_payload))

    return riff_header + riff_payload
