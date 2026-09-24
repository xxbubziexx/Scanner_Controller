import time
import asyncio
import logging
from typing import Optional
from drivers.base_driver import BaseScannerDriver, ScannerStatus
from config import ScannerConfig, ConnectionMode

logger = logging.getLogger("UnidenClassicDriver")

class UnidenClassicDriver(BaseScannerDriver):
    """
    Driver for Uniden Classic / BC125AT / BCD160DN / BCD260DN / BC780XLT series radios.
    Uses Uniden Classic protocol commands:
    - STS / STC / MDL: Get Reception Status / Display info
    - KEY,L/O,P or KEY,LO,P: Press Lockout keypress
    - KEY,SCAN,P: Send Scan keypress
    """

    def __init__(self, config: ScannerConfig):
        super().__init__(config)
        self._serial = None
        self._reader = None
        self._writer = None

    async def connect(self) -> bool:
        if not self.should_try_connect():
            return False

        try:
            if self.config.connection_mode == ConnectionMode.SERIAL:
                import serial
                self._serial = serial.Serial(
                    port=self.config.port,
                    baudrate=self.config.baud_rate,
                    timeout=0.3
                )
                self._serial.rts = True
                self._serial.dtr = True
                self._serial.reset_input_buffer()
                self.status.connected = True
                self._logged_connection_error = False
                logger.info(f"Uniden Classic ({self.config.model.value}) connected on Serial port {self.config.port}")
                return True
            else:
                self._reader, self._writer = await asyncio.wait_for(
                    asyncio.open_connection(self.config.ip_host, self.config.ip_port),
                    timeout=3.0
                )
                self.status.connected = True
                self._logged_connection_error = False
                logger.info(f"Uniden Classic ({self.config.model.value}) connected on IP {self.config.ip_host}:{self.config.ip_port}")
                return True
        except Exception as e:
            if not self._logged_connection_error:
                logger.warning(f"Could not connect {self.config.name} on {self.config.port} ({e}). Will retry periodically.")
                self._logged_connection_error = True
            self.status.connected = False
            self.status.status_line = f"PORT ERROR ({self.config.port})"
            return False

    async def disconnect(self) -> None:
        self.status.connected = False
        if self._serial:
            try:
                self._serial.close()
            except Exception:
                pass
            self._serial = None
        if self._writer:
            try:
                self._writer.close()
                await self._writer.wait_closed()
            except Exception:
                pass
            self._writer = None
            self._reader = None

    async def _send_command(self, cmd: str) -> str:
        """Send ASCII command terminated with \\r and read response"""
        if not self.status.connected:
            return ""
        
        full_cmd = (cmd + "\r").encode("ascii")
        try:
            if self.config.connection_mode == ConnectionMode.SERIAL and self._serial:
                self._serial.write(full_cmd)
                await asyncio.sleep(0.02)
                response = self._serial.read_until(b"\r").decode("ascii", errors="ignore").strip()
                return response
            elif self._writer and self._reader:
                self._writer.write(full_cmd)
                await self._writer.drain()
                line = await asyncio.wait_for(self._reader.readline(), timeout=0.5)
                return line.decode("ascii", errors="ignore").strip()
        except Exception as e:
            logger.error(f"Error communicating with Uniden Classic scanner: {e}")
            self.status.connected = False
        return ""

    async def poll(self) -> ScannerStatus:
        if not self.status.connected:
            await self.connect()
            if not self.status.connected:
                return self.status

        # Query STS or STC command
        res = await self._send_command("STS")
        if not res or "ERR" in res:
            res = await self._send_command("STC")

        if not res:
            return self.status

        # Parse CSV or tab response
        parts = res.split(",") if "," in res else res.split("\t")
        if len(parts) >= 3 and parts[0] in ["STS", "STC"]:
            system_name = parts[1].strip() if len(parts) > 1 else "Scanning..."
            channel_name = parts[2].strip() if len(parts) > 2 else ""
            freq_str = parts[3].strip() if len(parts) > 3 else ""

            freq_val = None
            if freq_str:
                try:
                    freq_val = float(freq_str)
                except ValueError:
                    pass

            is_receiving = bool(channel_name and channel_name not in ["Scanning...", "SCAN"] and (freq_val or len(parts) > 3))
            rssi_val = 4 if is_receiving else 0

            self.status.system_name = system_name if system_name else "Scanning..."
            self.status.dept_name = ""
            self.status.channel_name = channel_name if is_receiving else "Scanning..."
            self.status.tgid = None
            self.status.frequency = freq_val
            self.status.receiving = is_receiving
            if not is_receiving:
                self.status.avoided = False
            self.status.rssi = rssi_val
            self.status.status_line = f"RECV: {channel_name}" if is_receiving else "SCANNING"
            self.status.last_update = time.time()

        return self.status

    async def send_skip(self) -> bool:
        logger.info(f"Uniden Classic ({self.config.id}): Sending KEY,L/O,P (Skip/Lockout)")
        res = await self._send_command("KEY,L/O,P")
        if "ERR" in res or not res:
            res = await self._send_command("KEY,LO,P")
        self.status.avoided = True
        return "OK" in res or res != ""

    async def temporary_lockout(self, tgid_or_freq: str) -> bool:
        if str(self.status.frequency) == tgid_or_freq or self.status.tgid == tgid_or_freq:
            return await self.send_skip()
        res = await self._send_command(f"LOUT,{tgid_or_freq},TEMP")
        return "OK" in res or res != ""

    async def release_lockout(self, tgid_or_freq: str) -> bool:
        res = await self._send_command(f"UNLOCK,{tgid_or_freq}")
        self.status.avoided = False
        return True
