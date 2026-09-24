import time
import asyncio
import logging
import re
from typing import Optional, List
from drivers.base_driver import BaseScannerDriver, ScannerStatus, normalize_frequency
from config import ScannerConfig, ConnectionMode

logger = logging.getLogger("BCD996P2Driver")


def extract_section(data: str, key: str) -> Optional[str]:
    """
    Extracts the payload of a specific command tag from a concatenated multi-command response.
    Example: extract_section(resp, "GLG,") finds '\\rGLG,<payload>\\r' or '^GLG,<payload>\\r'.
    """
    pattern = rf'(?:^|[\r\n]){key}(.*?)(?:[\r\n]|$)'
    m = re.search(pattern, data)
    return m.group(1).strip() if m else None


class BCD996P2Driver(BaseScannerDriver):
    """
    Driver for Uniden DMA Series Radios (ScannerClass 2):
    - Models: BCD996P2, BCD325P2, BCD996XT, BCD396XT, BCT15, BCT15X, BC346XT, BR330T.
    
    Uses Uniden DMA multi-command pipelined protocol verified from ProScan decompile:
    - Poll command: MDL\\rSTS\\rGLG\\rVOL\\rSQL\\rPWR\\r
    - GLG[0]: Polymorphic Frequency or TalkGroup
    - GLG[1]: Modulation
    - GLG[3]: CTCSS/DCS Tone (or GLG[11] digital code)
    - GLG[4]: System Name
    - GLG[5]: Group / Department Name
    - GLG[6]: Channel Name
    - GLG[7]: Activity Flag ('1' = Receiving)
    - PWR[0]: RSSI signal level
    - PWR[1]: Authoritative Frequency
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
                logger.info(f"BCD996P2 connected on Serial port {self.config.port}")
                return True
            else:
                self._reader, self._writer = await asyncio.wait_for(
                    asyncio.open_connection(self.config.ip_host, self.config.ip_port),
                    timeout=3.0
                )
                self.status.connected = True
                self._logged_connection_error = False
                logger.info(f"BCD996P2 connected on IP {self.config.ip_host}:{self.config.ip_port}")
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
                self._serial.reset_input_buffer()
                self._serial.write(full_cmd)
                await asyncio.sleep(0.04)
                chunks = []
                while True:
                    waiting = self._serial.in_waiting
                    if waiting > 0:
                        chunks.append(self._serial.read(waiting))
                        await asyncio.sleep(0.02)
                    else:
                        break
                if not chunks:
                    resp = self._serial.read_until(b"\r")
                    chunks.append(resp)
                return b"".join(chunks).decode("ascii", errors="ignore").strip()
            elif self._writer and self._reader:
                self._writer.write(full_cmd)
                await self._writer.drain()
                line = await asyncio.wait_for(self._reader.readline(), timeout=0.5)
                return line.decode("ascii", errors="ignore").strip()
        except Exception as e:
            logger.error(f"Error communicating with {self.config.id}: {e}")
            self.status.connected = False
        return ""

    async def poll(self) -> ScannerStatus:
        """
        Polls the DMA scanner using the verified ProScan pipeline:
        Sends 'MDL\\rSTS\\rGLG\\rVOL\\rSQL\\rPWR\\r' in a single write and parses the tokenized response.
        """
        if not self.status.connected:
            await self.connect()
            if not self.status.connected:
                return self.status

        # Query the pipelined DMA command block
        res = await self._send_command("MDL\rSTS\rGLG\rVOL\rSQL\rPWR")
        if not res:
            return self.status

        # 1. Model identification (MDL,)
        mdl = extract_section(res, "MDL,")
        if mdl:
            self.status.model = mdl.strip()

        # 2. Parse PWR block (PWR,rssi,freq)
        pwr_str = extract_section(res, "PWR,")
        pwr_tokens: List[str] = [p.strip() for p in pwr_str.split(",")] if pwr_str else []

        freq_val: Optional[float] = None
        rssi_val: int = 0

        if len(pwr_tokens) >= 2:
            try:
                rssi_val = int(pwr_tokens[0])
            except ValueError:
                pass
            try:
                raw_freq = float(pwr_tokens[1])
                freq_val = normalize_frequency(raw_freq)
            except ValueError:
                pass

        # 3. Parse GLG block (GLG,freq/tgid,mod,tone,system,group,channel,activity,...)
        glg_str = extract_section(res, "GLG,")
        if not glg_str and "GLG," in res:
            # Standalone GLG response without leading delimiter
            glg_str = res.split("GLG,", 1)[1].split("\r")[0].strip()

        if glg_str:
            glg = [p.strip() for p in glg_str.split(",")]
            if len(glg) > 7:
                # Activity / Squelch flag at index 7 ('1' == squelch open)
                is_receiving = (glg[7] == "1")

                # Index 0: Polymorphic Frequency or Talkgroup
                item0 = glg[0].replace("ID:", "").strip()
                tgid_val: Optional[str] = None
                if "." in item0:
                    try:
                        freq_val = normalize_frequency(float(item0))
                    except ValueError:
                        pass
                elif len(item0) in (7, 8) and item0.isdigit() and "-" not in item0:
                    try:
                        norm_f = normalize_frequency(float(item0))
                        if norm_f:
                            freq_val = norm_f
                        else:
                            tgid_val = item0
                    except ValueError:
                        tgid_val = item0 if item0 else None
                else:
                    tgid_val = item0 if item0 else None

                # Modulation at index 1
                if len(glg) > 1 and glg[1]:
                    self.status.modulation = glg[1]

                # Hierarchy names
                sys_name = glg[4] if len(glg) > 4 else ""
                dept_name = glg[5] if len(glg) > 5 else ""
                chan_name = glg[6] if len(glg) > 6 else ""

                # Digital code / color code at index 11
                if len(glg) > 11 and glg[11]:
                    code = glg[11].replace("NONE", "").strip()
                    if code.startswith("10") and len(code) == 4:
                        try:
                            # DMR color code
                            cc = int(code[2:], 16)
                            self.status.tone = f"CC {cc}"
                        except ValueError:
                            pass
                    elif code.startswith("8"):
                        try:
                            ran = int(code) - 8192
                            if 0 <= ran <= 63:
                                self.status.tone = f"R {ran}"
                        except ValueError:
                            pass
                elif len(glg) > 3 and glg[3] and glg[3] != "0":
                    t = glg[3]
                    self.status.tone = f"C {t}" if "." in t else f"D {t}"

                # Invalidate activity if frequency and TGID are both blank
                if freq_val is None and not tgid_val:
                    is_receiving = False

                if is_receiving and not chan_name:
                    if freq_val:
                        chan_name = f"{freq_val:.4f} MHz"
                    elif tgid_val:
                        chan_name = f"TGID {tgid_val}"
                    else:
                        chan_name = "Active Channel"

                self.status.system_name = sys_name or "Scanning..."
                self.status.dept_name = dept_name
                self.status.channel_name = chan_name if is_receiving else "Scanning..."
                self.status.frequency = freq_val
                self.status.tgid = tgid_val
                self.status.receiving = is_receiving
                if not is_receiving:
                    self.status.avoided = False
                self.status.rssi = rssi_val if is_receiving else 0
                self.status.status_line = f"RECV: {chan_name}" if is_receiving else "SCANNING"
                self.status.last_update = time.time()
                return self.status

        # Fallback: if GLG was not returned, check if STS was returned
        sts_str = extract_section(res, "STS,")
        if sts_str:
            sts_parts = [p.strip() for p in sts_str.split(",")]
            if len(sts_parts) > 6:
                self.status.system_name = sts_parts[6] or "Scanning..."
            self.status.receiving = False
            self.status.status_line = "SCANNING"
            self.status.last_update = time.time()

        return self.status

    async def send_skip(self) -> bool:
        """
        Sends the verified DMA resume scan / skip command to advance past channel.
        Sends KEY,SCAN,P (or KEY,S,P / KEY,>,P) without setting permanent lockout.
        """
        logger.info(f"BCD996P2 ({self.config.id}): Sending KEY,SCAN,P (Resume Scan / Skip)")
        res = await self._send_command("KEY,SCAN,P")
        if "ERR" in res or not res or "NG" in res:
            res = await self._send_command("KEY,S,P")
        if "ERR" in res or not res or "NG" in res:
            res = await self._send_command("KEY,>,P")
        return "OK" in res or res != ""

    async def temporary_lockout(self, tgid_or_freq: str) -> bool:
        """Temporary Lockout for BCD996P2"""
        logger.info(f"BCD996P2 ({self.config.id}): Temporarily locking out {tgid_or_freq}")
        if self.status.tgid == tgid_or_freq or str(self.status.frequency) == tgid_or_freq:
            return await self.send_skip()
        res = await self._send_command(f"LOUT,{tgid_or_freq},TEMP")
        return "OK" in res or res != ""

    async def release_lockout(self, tgid_or_freq: str) -> bool:
        """Release Temporary Lockout"""
        logger.info(f"BCD996P2 ({self.config.id}): Releasing temporary lockout for {tgid_or_freq}")
        res = await self._send_command(f"UNLOCK,{tgid_or_freq}")
        self.status.avoided = False
        return True
