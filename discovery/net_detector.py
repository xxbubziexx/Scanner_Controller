import socket
import asyncio
import logging
from typing import List, Optional
from pydantic import BaseModel

logger = logging.getLogger("NetDetector")

class DiscoveredLANScanner(BaseModel):
    ip_address: str
    model: str
    name: str
    raw_response: str

def parse_sus_response(data: str, sender_ip: str) -> Optional[DiscoveredLANScanner]:
    """
    Parses Uniden UDP broadcast response.
    Format: SUS,UNIDEN,SCANNER,[Model],[Name]... (5 fields separated by 4 commas)
    """
    cleaned = data.strip()
    if not cleaned.startswith("SUS,"):
        return None
    
    parts = cleaned.split(",")
    if len(parts) >= 5:
        model = parts[3].strip() if len(parts) > 3 else "Uniden IP Scanner"
        name = parts[4].strip() if len(parts) > 4 else "Network Radio"
        return DiscoveredLANScanner(
            ip_address=sender_ip,
            model=model if model else "Uniden IP Scanner",
            name=name if name else "Network Scanner",
            raw_response=cleaned
        )
    elif len(parts) >= 3:
        return DiscoveredLANScanner(
            ip_address=sender_ip,
            model=parts[1].strip(),
            name=parts[2].strip() if len(parts) > 2 else "Network Scanner",
            raw_response=cleaned
        )
    return None

async def discover_lan_scanners(timeout_seconds: float = 2.0) -> List[DiscoveredLANScanner]:
    """
    Broadcasts Uniden discovery payload ('SUS,UNIDEN,SCANNER\\r') over UDP port 50536
    and collects responses from network-attached scanners (e.g., BCD536HP, SDS200).
    """
    results: List[DiscoveredLANScanner] = []
    seen_ips = set()

    def run_udp_probe() -> List[DiscoveredLANScanner]:
        scanners = []
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        sock.settimeout(timeout_seconds)

        try:
            # Attempt to bind to 50536 or default port
            try:
                sock.bind(("", 50536))
            except Exception:
                sock.bind(("", 0))

            payload = b"SUS,UNIDEN,SCANNER\r"
            # Send global broadcast
            sock.sendto(payload, ("255.255.255.255", 50536))

            start_time = asyncio.get_event_loop().time()
            while True:
                try:
                    data, addr = sock.recvfrom(1024)
                    ip_str = addr[0]
                    if ip_str not in seen_ips:
                        resp_str = data.decode("ascii", errors="ignore")
                        parsed = parse_sus_response(resp_str, ip_str)
                        if parsed:
                            seen_ips.add(ip_str)
                            scanners.append(parsed)
                except socket.timeout:
                    break
                except Exception:
                    break
        except Exception as e:
            logger.warning(f"UDP LAN Scanner Discovery exception: {e}")
        finally:
            sock.close()

        return scanners

    loop = asyncio.get_running_loop()
    results = await loop.run_in_executor(None, run_udp_probe)
    return results
