import os
import sys
import time
import asyncio
import logging
from typing import List, Dict, Any
from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException
from fastapi.staticfiles import StaticFiles
from fastapi.responses import HTMLResponse, FileResponse
from pydantic import BaseModel

from config import AppConfig, load_config, save_config
from engine.sync_manager import SyncManager

# Setup logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s"
)
logger = logging.getLogger("ScannerServer")

app = FastAPI(title="Uniden Scanner Mutual Exclusion Controller")

# Global Application Configuration and Sync Engine Instance
config = load_config()
sync_manager = SyncManager(config)

class ConnectionManager:
    def __init__(self):
        self.active_connections: List[WebSocket] = []

    async def connect(self, websocket: WebSocket):
        await websocket.accept()
        self.active_connections.append(websocket)
        logger.info(f"WebSocket client connected. Total clients: {len(self.active_connections)}")

    def disconnect(self, websocket: WebSocket):
        if websocket in self.active_connections:
            self.active_connections.remove(websocket)
            logger.info(f"WebSocket client disconnected. Total clients: {len(self.active_connections)}")

    async def broadcast(self, message: Dict[str, Any]):
        for connection in list(self.active_connections):
            try:
                await connection.send_json(message)
            except Exception:
                self.disconnect(connection)

connection_manager = ConnectionManager()

async def ws_broadcast_handler(state: Dict[str, Any]):
    await connection_manager.broadcast(state)

sync_manager.ws_broadcast_callback = ws_broadcast_handler

@app.on_event("startup")
async def startup_event():
    logger.info("Starting Scanner Sync Engine...")
    await sync_manager.start()

@app.on_event("shutdown")
async def shutdown_event():
    logger.info("Shutting down Scanner Sync Engine...")
    await sync_manager.stop()

def get_resource_dir(name: str) -> str:
    """Locates assets both in standard python and PyInstaller frozen execution."""
    if getattr(sys, "frozen", False):
        exe_dir = os.path.dirname(os.path.abspath(sys.executable))
        ext_path = os.path.join(exe_dir, name)
        if os.path.isdir(ext_path):
            return ext_path
        meipass = getattr(sys, "_MEIPASS", exe_dir)
        bundled_path = os.path.join(meipass, name)
        if os.path.isdir(bundled_path):
            return bundled_path
        return ext_path
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), name)

# Static Files
static_dir = get_resource_dir("static")
if not os.path.exists(static_dir):
    os.makedirs(static_dir, exist_ok=True)

app.mount("/static", StaticFiles(directory=static_dir), name="static")

@app.get("/favicon.ico", include_in_schema=False)
async def favicon():
    return HTMLResponse("", status_code=204)

@app.get("/")
async def get_root():
    index_path = os.path.join(static_dir, "index.html")
    if os.path.exists(index_path):
        return FileResponse(index_path)
    return HTMLResponse("<h2>Scanner Controller Server Running</h2><p>Static index.html not found.</p>")

@app.get("/api/state")
async def get_state():
    return sync_manager.get_full_state()

@app.post("/api/config")
async def update_config(new_config: AppConfig):
    global config
    config = new_config
    save_config(config)
    await sync_manager.stop()
    sync_manager.update_config(new_config)
    await sync_manager.start()
    return {"status": "success", "config": config.model_dump()}

class ModelChangePayload(BaseModel):
    model: str

@app.post("/api/scanner/{scanner_id}/model")
async def api_change_scanner_model(scanner_id: str, payload: ModelChangePayload):
    global config
    if scanner_id not in ("scanner_a", "scanner_b"):
        raise HTTPException(status_code=400, detail="Invalid scanner ID")
    new_model = ScannerModel(payload.model)
    if scanner_id == "scanner_a":
        config.scanner_a.model = new_model
    else:
        config.scanner_b.model = new_model
    save_config(config)
    await sync_manager.stop()
    sync_manager.update_config(config)
    await sync_manager.start()
    return {"status": "success", "scanner_id": scanner_id, "model": payload.model}

@app.post("/api/skip/{scanner_id}")
async def manual_skip(scanner_id: str):
    driver = sync_manager.scanner_a if scanner_id == "scanner_a" else sync_manager.scanner_b
    if not driver:
        raise HTTPException(status_code=404, detail="Scanner not found")
    success = await driver.send_skip()
    return {"status": "success" if success else "failed", "scanner_id": scanner_id}

@app.get("/api/audio/devices")
async def api_audio_devices():
    """Lists all audio capture devices (soundcards, virtual cables, line-in)"""
    from audio.recorder import list_audio_input_devices
    return {"status": "success", "devices": list_audio_input_devices()}

@app.get("/api/audio/output_devices")
async def api_audio_output_devices():
    """Lists all audio playback output devices (speakers, headphones)"""
    from audio.recorder import list_audio_output_devices
    return {"status": "success", "devices": list_audio_output_devices()}

# --- Discovery & Port Auto-Detect API Endpoints ---
from discovery.serial_detector import detect_serial_ports, DetectedPort
from discovery.net_detector import discover_lan_scanners, DiscoveredLANScanner
from config import ScannerModel, get_allowed_baud_rates

@app.get("/api/ports/available")
async def api_available_ports():
    """Lists all COM ports currently detected by Windows OS with Uniden scanner highlighting"""
    import re
    import serial.tools.list_ports
    
    raw_ports = serial.tools.list_ports.comports()
    
    def get_digits(name: str) -> int:
        d = re.sub(r"[^0-9]", "", name)
        return int(d) if d else 9999
        
    sorted_ports = sorted(raw_ports, key=lambda p: get_digits(p.device))
    ports_list = []
    
    for p in sorted_ports:
        hwid = p.hwid or ""
        desc = p.description or p.device
        is_uniden = ("1965:" in hwid) or ("uniden" in desc.lower()) or ("uniden" in p.manufacturer.lower() if p.manufacturer else False)
        
        display_label = f"{p.device} - {desc}"
        if is_uniden:
            display_label += " (Uniden Radio Detected)"
            
        ports_list.append({
            "device": p.device,
            "description": desc,
            "hwid": hwid,
            "is_uniden": is_uniden,
            "label": display_label
        })
        
    return {"status": "success", "ports": ports_list}

@app.get("/api/ports/detect")
async def api_detect_ports():
    # Identify currently connected active COM ports to mark 'In Use By This Instance'
    active_ports = []
    if sync_manager.scanner_a and sync_manager.scanner_a.config.connection_mode.value == "SERIAL":
        active_ports.append(sync_manager.scanner_a.config.port)
    if sync_manager.scanner_b and sync_manager.scanner_b.config.connection_mode.value == "SERIAL":
        active_ports.append(sync_manager.scanner_b.config.port)

    detected = await detect_serial_ports(active_ports)
    return {"status": "success", "ports": [p.model_dump() for p in detected]}

@app.get("/api/scanners/discover_lan")
async def api_discover_lan():
    discovered = await discover_lan_scanners()
    return {"status": "success", "scanners": [s.model_dump() for s in discovered]}

@app.get("/api/models/catalog")
async def api_models_catalog():
    catalog = []
    for m in ScannerModel:
        catalog.append({
            "model": m.value,
            "allowed_baud_rates": get_allowed_baud_rates(m)
        })
    return {"status": "success", "models": catalog}

# --- ScanScribe Feeder API Endpoints ---
from feeder.scanscribe_feeder import ScanScribeFeeder, CallTransmissionPayload
from config import ScanScribeFeederConfig

@app.post("/api/feeder/config")
async def api_update_feeder_config(new_feeder_cfg: ScanScribeFeederConfig):
    config.feeder = new_feeder_cfg
    save_config(config)
    sync_manager.feeder.update_config(new_feeder_cfg)
    return {"status": "success", "feeder_config": config.feeder.model_dump()}

@app.post("/api/feeder/dispatch_test")
async def api_dispatch_feeder_test():
    payload = CallTransmissionPayload(
        scanner_id="scanner_a",
        scanner_model=config.scanner_a.model.value,
        system_name="Metropolitan P25 Trunk",
        department_name="Fire & Rescue",
        channel_name="Dispatch North",
        tgid="10401",
        frequency=851.2500,
        rssi=5,
        duration_seconds=5.4,
        timestamp=time.time()
    )
    result = sync_manager.feeder.dispatch_call(payload)
    return {"status": "success", "dispatch_result": result}

@app.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    await connection_manager.connect(websocket)
    try:
        # Send initial state
        await websocket.send_json(sync_manager.get_full_state())
        while True:
            # Handle incoming WebSocket commands from UI
            data = await websocket.receive_json()
            cmd_type = data.get("type")
            if cmd_type == "SKIP":
                scanner_id = data.get("scanner_id")
                if scanner_id == "scanner_a" and sync_manager.scanner_a:
                    await sync_manager.scanner_a.send_skip()
                elif scanner_id == "scanner_b" and sync_manager.scanner_b:
                    await sync_manager.scanner_b.send_skip()
            elif cmd_type == "TOGGLE_SYNC":
                enabled = data.get("enabled", True)
                config.sync.enable_sync = enabled
                sync_manager.config.sync.enable_sync = enabled
                save_config(config)
            elif cmd_type == "SET_PRIORITY":
                p_mode = data.get("priority_mode")
                if p_mode:
                    config.sync.priority_mode = p_mode
                    sync_manager.config.sync.priority_mode = p_mode
                    save_config(config)
            elif cmd_type == "CHANGE_MODEL":
                scanner_id = data.get("scanner_id")
                new_model_str = data.get("model")
                if scanner_id in ("scanner_a", "scanner_b") and new_model_str:
                    new_model = ScannerModel(new_model_str)
                    if scanner_id == "scanner_a":
                        config.scanner_a.model = new_model
                    else:
                        config.scanner_b.model = new_model
                    save_config(config)
                    await sync_manager.stop()
                    sync_manager.update_config(config)
                    await sync_manager.start()

    except WebSocketDisconnect:
        connection_manager.disconnect(websocket)
    except Exception as e:
        logger.error(f"WebSocket error: {e}")
        connection_manager.disconnect(websocket)

if __name__ == "__main__":
    import multiprocessing
    multiprocessing.freeze_support()
    import uvicorn
    import webbrowser
    import threading

    def open_browser():
        time.sleep(1.2)
        try:
            webbrowser.open("http://127.0.0.1:8000")
        except Exception:
            pass

    threading.Thread(target=open_browser, daemon=True).start()

    is_frozen = getattr(sys, "frozen", False)
    if is_frozen:
        logger.info("Starting Uniden Scanner Controller Engine in portable mode...")
        logger.info("Web Control Dashboard available at http://127.0.0.1:8000")
        uvicorn.run(app, host="127.0.0.1", port=8000, log_level="info")
    else:
        uvicorn.run("server:app", host="127.0.0.1", port=8000, reload=True)
