import time
import asyncio
from abc import ABC, abstractmethod
from typing import Dict, Any, Optional
from config import ScannerConfig, LockoutMethod

def normalize_frequency(val: Optional[float]) -> Optional[float]:
    """
    Normalizes raw scanner frequency to MHz.
    Standard Uniden scanner range is 25.0 to 1300.0 MHz.
    Handles:
    - Floats already in MHz (e.g. 155.7975, 851.25)
    - 100Hz units integer scaling (e.g. 1557975 -> 155.7975, 8512500 -> 851.25)
    - 10Hz units integer scaling (e.g. 15579750 -> 155.7975, 85350000 -> 853.5000)
    - Hz units integer scaling (e.g. 155797500 -> 155.7975)
    - Low-band 100Hz scaling (e.g. 296000 -> 29.6000)
    """
    if val is None or val <= 0:
        return None
    # If already in valid scanner range
    if 25.0 <= val <= 1300.0:
        return round(val, 5)
    
    # Try standard Uniden 100Hz scaling (/ 10000.0)
    scaled_100hz = val / 10000.0
    if 25.0 <= scaled_100hz <= 1300.0:
        return round(scaled_100hz, 5)
        
    # Try 10Hz scaling (/ 100000.0)
    scaled_10hz = val / 100000.0
    if 25.0 <= scaled_10hz <= 1300.0:
        return round(scaled_10hz, 5)
        
    # Try Hz scaling (/ 1000000.0)
    scaled_hz = val / 1000000.0
    if 25.0 <= scaled_hz <= 1300.0:
        return round(scaled_hz, 5)

    # Iterative fallback if outside standard divisors
    temp = val
    while temp > 1300.0:
        temp /= 10.0
    if 25.0 <= temp <= 1300.0:
        return round(temp, 5)
    return round(val, 5)

class ScannerStatus:
    def __init__(self, scanner_id: str, name: str, model: str):
        self.scanner_id: str = scanner_id
        self.name: str = name
        self.model: str = model
        self.connected: bool = False
        self.receiving: bool = False
        self.system_name: str = "Scanning..."
        self.dept_name: str = ""
        self.channel_name: str = ""
        self.tgid: Optional[str] = None
        self.frequency: Optional[float] = None
        self.rssi: int = 0  # 0 to 5 bars or RSSI %
        self.modulation: str = "NFM"
        self.unit_id: Optional[str] = None
        self.status_line: str = "SCANNING"
        self.avoided: bool = False
        self.last_update: float = time.time()

    def to_dict(self) -> Dict[str, Any]:
        return {
            "scanner_id": self.scanner_id,
            "name": self.name,
            "model": self.model,
            "connected": self.connected,
            "receiving": self.receiving,
            "system_name": self.system_name,
            "dept_name": self.dept_name,
            "channel_name": self.channel_name,
            "tgid": self.tgid,
            "frequency": self.frequency,
            "rssi": self.rssi,
            "modulation": self.modulation,
            "unit_id": self.unit_id,
            "status_line": self.status_line,
            "avoided": self.avoided,
            "last_update": self.last_update
        }

    def clone(self) -> 'ScannerStatus':
        c = ScannerStatus(self.scanner_id, self.name, self.model)
        c.connected = self.connected
        c.receiving = self.receiving
        c.system_name = self.system_name
        c.dept_name = self.dept_name
        c.channel_name = self.channel_name
        c.tgid = self.tgid
        c.frequency = self.frequency
        c.rssi = self.rssi
        c.modulation = self.modulation
        c.unit_id = self.unit_id
        c.status_line = self.status_line
        c.avoided = self.avoided
        c.last_update = self.last_update
        return c

class BaseScannerDriver(ABC):
    def __init__(self, config: ScannerConfig):
        self.config: ScannerConfig = config
        self.status: ScannerStatus = ScannerStatus(
            scanner_id=config.id,
            name=config.name,
            model=config.model.value
        )
        self._running: bool = False
        self._last_connect_attempt: float = 0.0
        self._connect_retry_interval: float = 4.0
        self._logged_connection_error: bool = False

    def should_try_connect(self) -> bool:
        if not self.config.enabled:
            self.status.connected = False
            self.status.status_line = "DISABLED"
            return False
            
        port_val = (self.config.port or "").strip().upper()
        if not port_val or port_val in ["NONE", "DISABLED"]:
            self.status.connected = False
            self.status.status_line = "NO PORT (SELECT IN SETTINGS)"
            return False
            
        now = time.time()
        if now - self._last_connect_attempt < self._connect_retry_interval:
            return False
            
        self._last_connect_attempt = now
        return True

    @abstractmethod
    async def connect(self) -> bool:
        """Establish connection (serial COM port or IP TCP socket)"""
        pass

    @abstractmethod
    async def disconnect(self) -> None:
        """Close connection"""
        pass

    @abstractmethod
    async def poll(self) -> ScannerStatus:
        """Query scanner for live status and update status object"""
        pass

    @abstractmethod
    async def send_skip(self) -> bool:
        """Issue instant skip/avoid (e.g. KEY,AVD or KEY,L/O)"""
        pass

    @abstractmethod
    async def temporary_lockout(self, tgid_or_freq: str) -> bool:
        """Issue temporary avoid/lockout for a specific talkgroup or frequency"""
        pass

    @abstractmethod
    async def release_lockout(self, tgid_or_freq: str) -> bool:
        """Clear temporary avoid/lockout for a specific talkgroup or frequency"""
        pass

    def get_status_dict(self) -> Dict[str, Any]:
        return self.status.to_dict()
