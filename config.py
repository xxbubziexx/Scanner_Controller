import os
import sys
import json
import logging
from pydantic import BaseModel, Field
from enum import Enum
from typing import Optional, List

logger = logging.getLogger("ConfigManager")

class ConnectionMode(str, Enum):
    SERIAL = "SERIAL"
    IP = "IP"

class ScannerModel(str, Enum):
    BCD436HP = "BCD436HP"
    BCD996P2 = "BCD996P2"
    BCD536HP = "BCD536HP"
    SDS100 = "SDS100"
    SDS200 = "SDS200"
    BCD325P2 = "BCD325P2"
    BCD996XT = "BCD996XT"
    BCD396XT = "BCD396XT"
    BCT15X = "BCT15X"
    BCT15 = "BCT15"
    BC346XT = "BC346XT"
    BC346XTC = "BC346XTC"
    BR330T = "BR330T"
    BCD396T = "BCD396T"
    BCD996T = "BCD996T"
    BC250D = "BC250D"
    BC296D = "BC296D"
    BC780XLT = "BC780XLT"
    BC785D = "BC785D"
    BC796D = "BC796D"
    SDS150 = "SDS150"
    SDS100E = "SDS100E"
    SDS200E = "SDS200E"
    USDS100 = "USDS100"
    UBCD3600XLT = "UBCD3600XLT"
    BCD160DN = "BCD160DN"
    BCD260DN = "BCD260DN"
    BC125AT = "BC125AT"
    UBCD160DN = "UBCD160DN"
    UBCD260DN = "UBCD260DN"
    UBCD436PT = "UBCD436PT"
    UBCD536PT = "UBCD536PT"
    UBC125XLT = "UBC125XLT"
    UBC126AT = "UBC126AT"

def get_allowed_baud_rates(model: ScannerModel) -> List[int]:
    """Returns model-specific allowed baud rates based on ProScan specifications"""
    m = model.value if isinstance(model, ScannerModel) else str(model)
    if m in ["BCD160DN", "BCD260DN", "BC125AT", "UBCD160DN", "UBCD260DN", "UBC125XLT", "UBC126AT"]:
        return [115200]
    elif m in ["BC250D", "BC780XLT", "BC785D"]:
        return [19200, 9600, 4800, 2400]
    elif m in ["BC296D", "BC796D"]:
        return [57600, 38400, 19200, 9600, 4800, 2400]
    return [115200, 57600, 38400, 19200, 9600, 4800]

class LockoutMethod(str, Enum):
    KEYPRESS = "KEYPRESS"
    MEMORY = "MEMORY"

class PriorityMode(str, Enum):
    EQUAL_PEERS = "EQUAL_PEERS"
    PRIMARY_SECONDARY = "PRIMARY_SECONDARY"
    DEPARTMENT_PRIORITY = "DEPARTMENT_PRIORITY"

class ExclusionMode(str, Enum):
    TGID_AND_FREQ = "TGID_AND_FREQ"
    TGID_ONLY = "TGID_ONLY"
    FREQ_ONLY = "FREQ_ONLY"

class AudioChannelMode(str, Enum):
    MONO = "MONO"
    LEFT = "LEFT"
    RIGHT = "RIGHT"

class ScannerConfig(BaseModel):
    id: str
    name: str
    model: ScannerModel
    connection_mode: ConnectionMode = ConnectionMode.SERIAL
    port: str = "COM3"
    baud_rate: int = 115200
    ip_host: str = "127.0.0.1"
    ip_port: int = 5000
    enabled: bool = True
    poll_interval_ms: int = 150
    lockout_method: LockoutMethod = LockoutMethod.KEYPRESS
    audio_device_index: Optional[int] = None
    audio_channel: AudioChannelMode = AudioChannelMode.MONO
    audio_enabled: bool = True

class FeederMode(str, Enum):
    DIRECTORY_DROP = "DIRECTORY_DROP"
    HTTP_WEBHOOK = "HTTP_WEBHOOK"
    LIVE_STREAM = "LIVE_STREAM"

def get_default_inbox_dir() -> str:
    if "SCANSCRIBE_INBOX_DIR" in os.environ:
        return os.environ["SCANSCRIBE_INBOX_DIR"]
    if os.name != "nt":
        return "/var/scanscribe/inbox"
    return r"A:\Scanscribe_Projects\scanscribe\inbox"

def get_default_scanner_a_port() -> str:
    if "SCANNER_A_PORT" in os.environ:
        return os.environ["SCANNER_A_PORT"]
    if os.name != "nt":
        return "/dev/ttyACM0"
    return "COM5"

def get_default_scanner_b_port() -> str:
    if "SCANNER_B_PORT" in os.environ:
        return os.environ["SCANNER_B_PORT"]
    return "NONE"

class ScanScribeFeederConfig(BaseModel):
    enable_feeder: bool = True
    feeder_mode: FeederMode = FeederMode.DIRECTORY_DROP
    inbox_directory: str = Field(default_factory=get_default_inbox_dir)
    filename_template: str = "%DT - %S - %C (%TG)"
    tit2_template: str = "%C (%TG)"
    webhook_url: str = "http://127.0.0.1:5000/api/ingest"
    audio_format: str = "WAV"
    audio_device_index: Optional[int] = None
    sample_rate: int = 16000
    enable_sidecar_json: bool = True
    enable_proscan_tags: bool = True

class SyncConfig(BaseModel):
    enable_sync: bool = True
    exclusion_mode: ExclusionMode = ExclusionMode.TGID_AND_FREQ
    priority_mode: PriorityMode = PriorityMode.EQUAL_PEERS
    primary_scanner_id: str = "scanner_a"
    hang_time_seconds: float = 2.0
    auto_release_seconds: float = 30.0

class AudioLoopbackConfig(BaseModel):
    enabled: bool = True
    output_device_index: Optional[int] = None
    volume: float = 0.8
    pan_mode: str = "STEREO_SPLIT" # "STEREO_SPLIT" (A=Left, B=Right) or "CENTER"

class AppConfig(BaseModel):
    scanner_a: ScannerConfig = Field(
        default_factory=lambda: ScannerConfig(
            id="scanner_a",
            name="Scanner A",
            model=ScannerModel.BCD436HP,
            port=get_default_scanner_a_port(),
            baud_rate=115200,
            audio_channel=AudioChannelMode.LEFT
        )
    )
    scanner_b: ScannerConfig = Field(
        default_factory=lambda: ScannerConfig(
            id="scanner_b",
            name="Scanner B",
            model=ScannerModel.BCD996P2,
            port=get_default_scanner_b_port(),
            baud_rate=115200,
            audio_channel=AudioChannelMode.RIGHT
        )
    )
    sync: SyncConfig = Field(default_factory=SyncConfig)
    feeder: ScanScribeFeederConfig = Field(default_factory=ScanScribeFeederConfig)
    audio_loopback: AudioLoopbackConfig = Field(default_factory=AudioLoopbackConfig)

def get_config_file_path() -> str:
    """Returns the absolute path to config.json, respecting frozen PyInstaller directory or environment."""
    if "SCANNER_CONFIG_FILE" in os.environ:
        return os.environ["SCANNER_CONFIG_FILE"]
    if getattr(sys, "frozen", False):
        exe_dir = os.path.dirname(os.path.abspath(sys.executable))
        return os.path.join(exe_dir, "config.json")
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.json")

def load_config(config_path: Optional[str] = None) -> AppConfig:
    """Loads AppConfig from JSON file. If file does not exist or is invalid, returns default AppConfig and saves it."""
    path = config_path or get_config_file_path()
    if os.path.exists(path):
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
            config = AppConfig.model_validate(data)
            logger.info(f"Loaded persistent configuration from {path}")
            return config
        except Exception as e:
            logger.warning(f"Could not parse config file at {path} ({e}). Falling back to default configuration.")

    config = AppConfig()
    save_config(config, path)
    return config

def save_config(config: AppConfig, config_path: Optional[str] = None) -> bool:
    """Saves AppConfig to JSON file."""
    path = config_path or get_config_file_path()
    try:
        data = config.model_dump()
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
        logger.info(f"Saved persistent configuration to {path}")
        return True
    except Exception as e:
        logger.error(f"Failed to save configuration to {path}: {e}")
        return False
