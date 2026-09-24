import re
import time
import asyncio
import logging
from typing import List, Dict, Any, Optional
from pydantic import BaseModel

logger = logging.getLogger("SerialDetector")

CANDIDATE_BAUDS = [115200, 57600, 38400, 19200, 9600, 4800, 2400]

class DetectedPort(BaseModel):
    port: str             # "COM3"
    port_digits: str      # "3"
    status: str           # "Available", "In Use By Another Program", "Check Driver"
    model: str = ""       # e.g. "BCD436HP"
    baud_rate: int = 0    # e.g. 115200

def parse_mdl_response(response: str) -> Optional[str]:
    """Parse Uniden MDL command response: \rMDL,BCD436HP\r -> BCD436HP"""
    if "MDL," in response:
        idx = response.find("MDL,")
        tail = response[idx + 4:].strip()
        parts = tail.split(",")
        model_candidate = parts[0].strip()
        if model_candidate:
            return model_candidate
    return None

def parse_si_response(response: str) -> Optional[str]:
    """Parse legacy Uniden SI command response: \rSI BC346XT,...\r -> BC346XT"""
    if "SI " in response:
        idx = response.find("SI ")
        tail = response[idx + 3:].strip()
        parts = tail.split(",")
        model_candidate = parts[0].strip()
        if model_candidate:
            return model_candidate
    return None

def extract_port_digits(port_name: str) -> int:
    """Extract numeric digits from COM port string for numerical sorting (COM10 -> 10)"""
    digits = re.sub(r"[^0-9]", "", port_name)
    return int(digits) if digits else 9999

async def detect_serial_ports(active_ports: Optional[List[str]] = None) -> List[DetectedPort]:
    """
    Probes system serial COM ports at standard baud rates, issuing \rMDL\r and \rSI\r commands.
    Categorizes ports into 'Available', 'In Use By Another Program', 'Check Driver'.
    Returns list of DetectedPort objects sorted numerically by COM port number.
    """
    if active_ports is None:
        active_ports = []

    try:
        import serial.tools.list_ports
        raw_ports = serial.tools.list_ports.comports()
    except Exception as e:
        logger.error(f"Error enumerating serial ports: {e}")
        return []

    # Sort ports numerically (COM1, COM2, COM9, COM10)
    sorted_ports = sorted(raw_ports, key=lambda p: extract_port_digits(p.device))
    results: List[DetectedPort] = []

    for port_info in sorted_ports:
        port_name = port_info.device  # e.g. "COM3"
        digits_str = str(extract_port_digits(port_name))

        status = ""
        found_model = ""
        found_baud = 0

        # Check if held open by our active process
        if port_name in active_ports or digits_str in active_ports:
            results.append(DetectedPort(
                port=port_name,
                port_digits=digits_str,
                status="In Use By This Instance",
                model="",
                baud_rate=115200
            ))
            continue

        for idx, baud in enumerate(CANDIDATE_BAUDS):
            try:
                import serial
                ser = serial.Serial(
                    port=port_name,
                    baudrate=baud,
                    timeout=0.2,
                    write_timeout=0.2
                )
                ser.rts = True
                ser.dtr = True

                if idx == 0:
                    status = "Available"

                # Send wire probe 1: \rMDL\r
                ser.write(b"\rMDL\r")
                await asyncio.sleep(0.05)
                res = ser.read_all().decode("ascii", errors="ignore")

                model_name = parse_mdl_response(res)
                if not model_name:
                    # Send wire probe 2 (fallback): \rSI\r
                    ser.write(b"\rSI\r")
                    await asyncio.sleep(0.05)
                    res_si = ser.read_all().decode("ascii", errors="ignore")
                    model_name = parse_si_response(res_si)

                ser.close()

                if model_name:
                    found_model = model_name
                    found_baud = baud
                    break

            except serial.SerialException as e:
                err_str = str(e).lower()
                if idx == 0 and ("access" in err_str or "denied" in err_str or "permission" in err_str):
                    status = "In Use By Another Program"
                    break
                # Non-permission error at this baud rate: try the next one
                continue
            except Exception:
                break

        if not status:
            status = "Check Driver"

        results.append(DetectedPort(
            port=port_name,
            port_digits=digits_str,
            status=status,
            model=found_model,
            baud_rate=found_baud if found_baud > 0 else 115200
        ))

    return results
