import time
import asyncio
import logging
import re
import xml.etree.ElementTree as ET
from typing import Optional, List
from drivers.base_driver import BaseScannerDriver, ScannerStatus, normalize_frequency
from config import ScannerConfig, ConnectionMode

logger = logging.getLogger("BCD436HPDriver")


def extract_section(data: str, key: str) -> Optional[str]:
    """
    Extracts the payload of a specific command tag from a concatenated multi-command response.
    Example: extract_section(resp, "GLG,") finds '\\rGLG,<payload>\\r' or '^GLG,<payload>\\r'.
    """
    pattern = rf'(?:^|[\r\n]){key}(.*?)(?:[\r\n]|$)'
    m = re.search(pattern, data)
    return m.group(1).strip() if m else None


class BCD436HPDriver(BaseScannerDriver):
    """
    Driver for Uniden HomePatrol / x36HP / SDS Series Radios (ScannerClass 3 & 4):
    - Models: BCD436HP, BCD536HP, SDS100, SDS200, SDS150, SDS100E, SDS200E, UBCD436PT, UBCD536PT, USDS100.
    
    Uses verified ProScan pipelined multi-command protocol:
    - Poll command: MDL\\rSTS\\rGLG\\rVOL\\rSQL\\rPWR\\rGSI,1\\r
    - GLG[4]: System Name
    - GLG[Len - 5]: Activity Flag ('1' = Receiving)
    - PWR[0]: RSSI signal level (!= "-999")
    - PWR[1]: Frequency
    - GSI,1 XML: <Department Name="...">, <ConvFrequency Name="..." TGID="..." Freq="...">,
                 <Property P25Status="Enc" Mute="..." RecSlot="..." U_Id="...">
    - STS fallback: Physical screen representation when XML is unavailable
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
                # Ensure hardware flow / DTR / RTS lines are asserted for Uniden USB serial
                self._serial.rts = True
                self._serial.dtr = True
                self._serial.reset_input_buffer()
                self.status.connected = True
                self._logged_connection_error = False
                logger.info(f"BCD436HP connected on Serial port {self.config.port} at {self.config.baud_rate} baud")
                return True
            else:
                self._reader, self._writer = await asyncio.wait_for(
                    asyncio.open_connection(self.config.ip_host, self.config.ip_port),
                    timeout=3.0
                )
                self.status.connected = True
                self._logged_connection_error = False
                logger.info(f"BCD436HP connected on IP {self.config.ip_host}:{self.config.ip_port}")
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
            logger.error(f"Error communicating with BCD436HP: {e}")
            self.status.connected = False
        return ""

    @staticmethod
    def _clean_text(text: str) -> str:
        """Strip non-printable display icons and extraneous whitespace"""
        return re.sub(r'[\x00-\x1f\x7f-\xff]', ' ', text).strip()

    def _parse_sts_fallback(self, res: str) -> ScannerStatus:
        """Original LCD divider screen representation parser for STS responses"""
        parts = [p.strip() for p in res.split(",")]
        dividers = [i for i, p in enumerate(parts) if set(p) == {'_'}]

        sys_name = "Scanning..."
        dept_name = ""
        chan_name = ""
        freq_val = None
        tgid_val = None
        sql_open = False
        rssi_val = 0

        if len(dividers) >= 3:
            sys_lines = [self._clean_text(parts[i]) for i in range(dividers[0]+1, dividers[1]) if self._clean_text(parts[i]) and set(parts[i]) != {'_'}]
            dept_lines = [self._clean_text(parts[i]) for i in range(dividers[1]+1, dividers[2]) if self._clean_text(parts[i]) and set(parts[i]) != {'_'}]
            end_chan = dividers[3] if len(dividers) > 3 else len(parts)
            chan_lines = [self._clean_text(parts[i]) for i in range(dividers[2]+1, end_chan) if self._clean_text(parts[i]) and set(parts[i]) != {'_'}]

            if sys_lines:
                sys_name = ' '.join(sys_lines[:2])
            if dept_lines:
                dept_name = dept_lines[0]
            if chan_lines:
                chan_name = chan_lines[0]

            chan_text_all = ' '.join(chan_lines)
            m_freq = re.search(r'(\d{2,4}\.\d{3,5})', chan_text_all)
            if m_freq:
                try:
                    freq_val = float(m_freq.group(1))
                except ValueError:
                    pass

            m_tgid = re.search(r'(?:TGID:\s*|TG:)(\d+)', chan_text_all, re.IGNORECASE)
            if m_tgid:
                tgid_val = m_tgid.group(1)

            if len(parts) > 31:
                try:
                    sql_open = (parts[30].strip() == "1")
                    rssi_val = int(parts[31].strip())
                except (ValueError, IndexError):
                    pass
        else:
            if len(parts) > 6:
                sys_name = self._clean_text(parts[6]) or "Scanning..."
            if len(parts) > 12:
                dept_name = self._clean_text(parts[12])
            if len(parts) > 18:
                chan_name = self._clean_text(parts[18])

        is_stopped = bool((chan_name and chan_name not in ["Scanning...", "SCAN", "ID SEARCH", ""]) or (sql_open and (freq_val or tgid_val or dept_name)))

        if is_stopped:
            is_receiving = sql_open
            self.status.system_name = sys_name
            self.status.dept_name = dept_name
            self.status.channel_name = chan_name
            self.status.frequency = freq_val
            self.status.tgid = tgid_val
            self.status.receiving = is_receiving
            self.status.rssi = rssi_val if rssi_val > 0 else (4 if is_receiving else 0)
            self.status.status_line = f"RECV: {chan_name}" if is_receiving else f"HOLD: {chan_name}"
        else:
            self.status.system_name = sys_name if sys_name else "Scanning..."
            self.status.dept_name = dept_name
            self.status.channel_name = "Scanning..."
            self.status.frequency = None
            self.status.tgid = None
            self.status.receiving = False
            self.status.avoided = False
            self.status.rssi = 0
            self.status.status_line = "SCANNING"

        self.status.last_update = time.time()
        return self.status

    async def poll(self) -> ScannerStatus:
        """
        Polls the HP/SDS scanner using the verified ProScan multi-command pipeline:
        Sends 'MDL\\rSTS\\rGLG\\rVOL\\rSQL\\rPWR\\rGSI,1\\r' in a single write.
        Parses structured GLG, PWR, and XML fields with fallback to STS screen scraping.
        """
        if not self.status.connected:
            await self.connect()
            if not self.status.connected:
                return self.status

        # Query the pipelined HP/SDS command block
        res = await self._send_command("MDL\rSTS\rGLG\rVOL\rSQL\rPWR\rGSI,1")
        if not res:
            return self.status

        # 1. Model identification (MDL,)
        mdl = extract_section(res, "MDL,")
        if mdl:
            self.status.model = mdl.strip()

        # 2. Extract PWR block (PWR,rssi,freq)
        pwr_str = extract_section(res, "PWR,")
        pwr_tokens: List[str] = [p.strip() for p in pwr_str.split(",")] if pwr_str else []

        freq_val: Optional[float] = None
        rssi_val: int = 0
        if len(pwr_tokens) >= 2:
            if pwr_tokens[0] != "-999":
                try:
                    rssi_val = int(pwr_tokens[0])
                except ValueError:
                    pass
            try:
                raw_freq = float(pwr_tokens[1])
                freq_val = normalize_frequency(raw_freq)
            except ValueError:
                pass

        # 3. Check for GLG block
        glg_str = extract_section(res, "GLG,")
        if not glg_str and "GLG," in res:
            glg_str = res.split("GLG,", 1)[1].split("\r")[0].strip()

        # 4. Check for GSI,1 XML payload
        xml_payload = None
        if "<ScannerInfo>" in res and "</ScannerInfo>" in res:
            try:
                xml_raw = res.split("<ScannerInfo>", 1)[1].split("</ScannerInfo>", 1)[0]
                xml_payload = f"<ScannerInfo>{xml_raw}</ScannerInfo>"
            except Exception:
                pass

        # If GLG or XML was received, perform high-precision structured parsing
        if glg_str or xml_payload:
            sys_name = ""
            dept_name = ""
            chan_name = ""
            tgid_val: Optional[str] = None
            is_receiving = False
            digital_status = ""

            if glg_str:
                glg = [p.strip() for p in glg_str.split(",")]
                is_dma_model = getattr(self.status, "model", "") in [
                    "BCD996P2", "BCD325P2", "BCD996XT", "BCD396XT",
                    "BCT15X", "BCT15", "BC346XT", "BC346XTC", "BR330T",
                    "BCD996T", "BCD396T"
                ]

                # Polymorphic field at index 0 (Frequency or Talkgroup)
                item0 = glg[0].replace("ID:", "").strip() if len(glg) > 0 else ""
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
                elif item0:
                    tgid_val = item0

                # Modulation at index 1
                if len(glg) > 1 and glg[1]:
                    self.status.modulation = glg[1]

                # Hierarchy names in standard Uniden sequence
                if len(glg) > 4 and glg[4]:
                    sys_name = glg[4]
                if len(glg) > 5 and glg[5]:
                    dept_name = glg[5]
                if len(glg) > 6 and glg[6]:
                    chan_name = glg[6]

                # Activity flag
                if is_dma_model:
                    is_receiving = (glg[7] == "1") if len(glg) > 7 else False
                else:
                    if len(glg) >= 5:
                        is_receiving = (glg[len(glg) - 5] == "1")
                    if not is_receiving and not xml_payload and len(glg) > 7 and glg[7] == "1":
                        is_receiving = True

            # Parse XML fields from GSI,1 if returned by HP/SDS firmware
            if xml_payload:
                try:
                    root = ET.fromstring(xml_payload)
                    dept_elem = root.find("Department")
                    if dept_elem is not None and "Name" in dept_elem.attrib and dept_elem.attrib["Name"].strip():
                        dept_name = dept_elem.attrib["Name"].strip()

                    freq_elem = root.find("ConvFrequency")
                    if freq_elem is None:
                        freq_elem = root.find("TrunkFrequency")

                    if freq_elem is not None:
                        if "Name" in freq_elem.attrib and freq_elem.attrib["Name"].strip():
                            chan_name = freq_elem.attrib["Name"].strip()
                        if "TGID" in freq_elem.attrib and freq_elem.attrib["TGID"].strip():
                            tgid_val = freq_elem.attrib["TGID"].strip()
                        if "Freq" in freq_elem.attrib and freq_elem.attrib["Freq"].strip():
                            try:
                                freq_val = normalize_frequency(float(freq_elem.attrib["Freq"].strip()))
                            except ValueError:
                                pass

                    prop_elem = root.find("Property")
                    if prop_elem is not None:
                        if "P25Status" in prop_elem.attrib:
                            digital_status = prop_elem.attrib["P25Status"].strip()
                            if digital_status.lower() == "enc":
                                is_receiving = True  # Encrypted activity counts as receiving
                        if "Rssi" in prop_elem.attrib and prop_elem.attrib["Rssi"] != "-999":
                            try:
                                rssi_val = int(prop_elem.attrib["Rssi"])
                            except ValueError:
                                pass
                except Exception as xml_err:
                    logger.debug(f"XML parse exception in GSI block: {xml_err}")

            if is_receiving and (chan_name or freq_val or tgid_val):
                if not chan_name:
                    if freq_val:
                        chan_name = f"{freq_val:.4f} MHz"
                    elif tgid_val:
                        chan_name = f"TGID {tgid_val}"
                    else:
                        chan_name = "Active Channel"

                self.status.system_name = sys_name or "Scanning..."
                self.status.dept_name = dept_name
                self.status.channel_name = chan_name
                self.status.frequency = freq_val
                self.status.tgid = tgid_val
                self.status.receiving = True
                self.status.rssi = rssi_val if rssi_val > 0 else 4
                self.status.status_line = f"RECV: {self.status.channel_name}"
            else:
                self.status.system_name = sys_name or "Scanning..."
                self.status.dept_name = dept_name
                self.status.channel_name = "Scanning..."
                self.status.frequency = None
                self.status.tgid = None
                self.status.receiving = False
                self.status.avoided = False
                self.status.rssi = 0
                self.status.status_line = "SCANNING"

            self.status.last_update = time.time()
            return self.status

        # 5. Fallback: Parse STS LCD screen text if GLG / XML were not returned
        sts_str = extract_section(res, "STS,")
        if sts_str:
            return self._parse_sts_fallback(f"STS,{sts_str}")
        elif res.startswith("STS,"):
            return self._parse_sts_fallback(res)

        return self.status

    async def send_skip(self) -> bool:
        """
        Advances past current transmission on BCD436HP without lockout or opening menus.
        Sends Rotary Knob Right (KEY,>,P) to advance to the next channel.
        Falls back to Channel Hold toggle (KEY,Z,P) to resume scan.
        NEVER sends KEY,S,P because 'S' triggers System Quick Key navigation on the 436HP.
        """
        logger.info(f"BCD436HP ({self.config.id}): Sending KEY,>,P (Rotary Knob Step / Skip)")
        res = await self._send_command("KEY,>,P")
        if "ERR" in res or not res or "NG" in res:
            res = await self._send_command("KEY,>, P")
        if "ERR" in res or not res or "NG" in res:
            # Channel Hold release toggle (KEY,Z,P): holds and unholds to resume scanning
            logger.info(f"BCD436HP ({self.config.id}): Knob step failed; sending Channel Hold cycle (KEY,Z,P)")
            await self._send_command("KEY,Z,P")
            await asyncio.sleep(0.05)
            res = await self._send_command("KEY,Z,P")
        return "OK" in res or res != ""

    async def dismiss_menu(self) -> bool:
        """Sends KEY,.,P (no/back) to dismiss any open Quick Key or setup menus."""
        logger.info(f"BCD436HP ({self.config.id}): Sending KEY,.,P to dismiss menus")
        res = await self._send_command("KEY,.,P")
        return "OK" in res or res != ""

    async def temporary_lockout(self, tgid_or_freq: str) -> bool:
        """
        Temporary avoid command for BCD436HP:
        Sends KEY,V,P (physical Avoid keypress, ScannerButton19) or softkey KEY,A,L
        """
        logger.info(f"BCD436HP ({self.config.id}): Sending KEY,V,P (Avoid button) for {tgid_or_freq}")
        res = await self._send_command("KEY,V,P")
        if "ERR" in res or not res or "NG" in res:
            res = await self._send_command("KEY,A,L")
        self.status.avoided = True
        return "OK" in res or res != ""

    async def release_lockout(self, tgid_or_freq: str) -> bool:
        """Clear avoid on BCD436HP"""
        logger.info(f"BCD436HP ({self.config.id}): Releasing avoid for {tgid_or_freq}")
        res = await self._send_command("KEY,V,P")
        self.status.avoided = False
        return True
