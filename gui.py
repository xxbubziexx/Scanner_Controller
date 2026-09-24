import os
import sys
import time
import asyncio
import threading
import logging
from typing import Optional, Dict, Any, List

import customtkinter as ctk
from tkinter import filedialog, messagebox

# Set modern dark styling
ctk.set_appearance_mode("dark")
ctk.set_default_color_theme("blue")

from config import AppConfig, load_config, save_config, PriorityMode, ExclusionMode, FeederMode, ScannerModel, get_allowed_baud_rates, AudioChannelMode
from engine.sync_manager import SyncManager
from discovery.serial_detector import detect_serial_ports
from discovery.net_detector import discover_lan_scanners
from audio.recorder import list_audio_input_devices, list_audio_output_devices
from feeder.scanscribe_feeder import CallTransmissionPayload

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s"
)
logger = logging.getLogger("ScannerGUI")

POPULAR_SCANNER_MODELS = [
    "BCD996P2", "BCD436HP", "SDS100", "SDS200", "BCD536HP",
    "BCD325P2", "BCD996XT", "BCD396XT", "BCT15X", "BCT15",
    "BC125AT", "BCD160DN", "BCD260DN", "BC346XT", "BR330T",
    "BCD996T", "BCD396T", "BC250D", "BC296D", "BC780XLT",
    "BC785D", "BC796D", "SDS150", "SDS100E", "SDS200E",
    "USDS100", "UBCD3600XLT", "UBCD436PT", "UBCD536PT",
    "UBCD160DN", "UBCD260DN", "UBC125XLT", "UBC126AT", "BC346XTC"
]


class BackgroundEngine:
    """Manages the asyncio event loop for SyncManager on a dedicated background thread."""
    def __init__(self, config: AppConfig):
        self.config = config
        self.sync_manager = SyncManager(config)
        self.loop: Optional[asyncio.AbstractEventLoop] = None
        self.thread: Optional[threading.Thread] = None
        self.web_server_thread: Optional[threading.Thread] = None
        self.running = False

    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self._run_async_loop, daemon=True, name="SyncEngineThread")
        self.thread.start()

    def _run_async_loop(self):
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        self.loop.run_until_complete(self.sync_manager.start())
        try:
            self.loop.run_forever()
        finally:
            self.loop.run_until_complete(self.sync_manager.stop())
            self.loop.close()

    def stop(self):
        self.running = False
        if self.loop and self.loop.is_running():
            try:
                fut = asyncio.run_coroutine_threadsafe(self.sync_manager.stop(), self.loop)
                fut.result(timeout=2.0)
            except Exception:
                pass
            self.loop.call_soon_threadsafe(self.loop.stop)

    def update_config(self, new_config: AppConfig):
        self.config = new_config
        if self.loop and self.loop.is_running():
            self.sync_manager.update_config(new_config)
            asyncio.run_coroutine_threadsafe(self.sync_manager.start(), self.loop)

    def send_skip(self, scanner_id: str):
        if not self.loop or not self.loop.is_running():
            return
        driver = self.sync_manager.scanner_a if scanner_id == "scanner_a" else self.sync_manager.scanner_b
        if driver:
            asyncio.run_coroutine_threadsafe(driver.send_skip(), self.loop)

    def send_avoid(self, scanner_id: str):
        if not self.loop or not self.loop.is_running():
            return
        driver = self.sync_manager.scanner_a if scanner_id == "scanner_a" else self.sync_manager.scanner_b
        if driver:
            asyncio.run_coroutine_threadsafe(driver.send_skip(), self.loop)


class ScannerCardFrame(ctk.CTkFrame):
    """Visual LCD Display and Control Card for an individual scanner."""
    def __init__(self, master, scanner_id: str, title: str, initial_model: str = "BCD436HP", on_skip=None, on_avoid=None, on_model_change=None, **kwargs):
        super().__init__(
            master,
            corner_radius=12,
            fg_color="#1E293B",
            border_width=1,
            border_color="#334155",
            height=365,
            **kwargs
        )
        self.scanner_id = scanner_id
        self.on_skip = on_skip
        self.on_avoid = on_avoid
        self.on_model_change = on_model_change
        self.pack_propagate(False)
        self.grid_propagate(False)

        # Header: Name + Model Selector + Connection Status Badge
        header_frame = ctk.CTkFrame(self, fg_color="transparent", height=36)
        header_frame.pack(fill="x", padx=16, pady=(12, 6))
        header_frame.pack_propagate(False)

        self.title_label = ctk.CTkLabel(
            header_frame,
            text=title,
            font=ctk.CTkFont(family="Segoe UI", size=14, weight="bold"),
            text_color="#F8FAFC",
            anchor="w"
        )
        self.title_label.pack(side="left", padx=(0, 6))

        self.combo_model = ctk.CTkComboBox(
            header_frame,
            values=POPULAR_SCANNER_MODELS,
            width=135,
            height=28,
            font=ctk.CTkFont(size=12, weight="bold"),
            dropdown_font=ctk.CTkFont(size=11),
            fg_color="#0F172A",
            border_color="#0284C7",
            button_color="#0284C7",
            command=self._on_combo_model_selected
        )
        self.combo_model.set(initial_model)
        self.combo_model.pack(side="left", padx=(0, 8))

        self.status_pill = ctk.CTkLabel(
            header_frame,
            text="DISCONNECTED",
            font=ctk.CTkFont(family="Segoe UI", size=11, weight="bold"),
            fg_color="#334155",
            text_color="#94A3B8",
            corner_radius=6,
            padx=10,
            pady=3
        )
        self.status_pill.pack(side="right")

        self.rec_badge = ctk.CTkLabel(
            header_frame,
            text="REC ON",
            font=ctk.CTkFont(family="Segoe UI", size=10, weight="bold"),
            fg_color="#14532D",
            text_color="#86EFAC",
            corner_radius=6,
            padx=8,
            pady=3
        )
        self.rec_badge.pack(side="right", padx=(0, 8))

        # ProScan-Style Virtual Monospace LCD Screen (Locked, rigid geometry)
        self.lcd_screen = ctk.CTkFrame(
            self,
            fg_color="#090E17",
            corner_radius=8,
            border_width=1,
            border_color="#1E3A8A",
            height=162
        )
        self.lcd_screen.pack(fill="x", padx=16, pady=6)
        self.lcd_screen.pack_propagate(False)
        self.lcd_screen.grid_propagate(False)

        # LCD Line 1: System Name
        self.lbl_system = ctk.CTkLabel(
            self.lcd_screen,
            text="System: STANDBY / SCANNING",
            font=ctk.CTkFont(family="Consolas", size=12),
            text_color="#94A3B8",
            height=20,
            anchor="w"
        )
        self.lbl_system.pack(fill="x", padx=12, pady=(6, 1))

        # LCD Line 2: Department
        self.lbl_dept = ctk.CTkLabel(
            self.lcd_screen,
            text="Dept: -",
            font=ctk.CTkFont(family="Consolas", size=12),
            text_color="#CBD5E1",
            height=20,
            anchor="w"
        )
        self.lbl_dept.pack(fill="x", padx=12, pady=1)

        # LCD Line 3: Channel Name (Large Primary)
        self.lbl_channel = ctk.CTkLabel(
            self.lcd_screen,
            text="Ready / Idle",
            font=ctk.CTkFont(family="Consolas", size=18, weight="bold"),
            text_color="#38BDF8",
            height=32,
            anchor="w"
        )
        self.lbl_channel.pack(fill="x", padx=12, pady=2)

        # LCD Line 4: Freq / TGID / Mod
        freq_row = ctk.CTkFrame(self.lcd_screen, fg_color="transparent", height=24)
        freq_row.pack(fill="x", padx=12, pady=1)
        freq_row.pack_propagate(False)

        self.lbl_freq = ctk.CTkLabel(
            freq_row,
            text="Freq: ---.---- MHz",
            font=ctk.CTkFont(family="Consolas", size=13, weight="bold"),
            text_color="#34D399",
            anchor="w"
        )
        self.lbl_freq.pack(side="left")

        self.lbl_tgid = ctk.CTkLabel(
            freq_row,
            text="TGID: -",
            font=ctk.CTkFont(family="Consolas", size=12),
            text_color="#FCD34D",
            padx=12
        )
        self.lbl_tgid.pack(side="left")

        self.lbl_mod = ctk.CTkLabel(
            freq_row,
            text="MOD: NFM",
            font=ctk.CTkFont(family="Consolas", size=11),
            text_color="#94A3B8"
        )
        self.lbl_mod.pack(side="right")

        # LCD Line 5: Activity / Status
        self.lbl_status = ctk.CTkLabel(
            self.lcd_screen,
            text="STATUS: Idle",
            font=ctk.CTkFont(family="Consolas", size=11),
            text_color="#64748B",
            height=20,
            anchor="w"
        )
        self.lbl_status.pack(fill="x", padx=12, pady=(1, 6))

        # Metrics: Signal RSSI & Audio Meter
        meters_frame = ctk.CTkFrame(self, fg_color="transparent")
        meters_frame.pack(fill="x", padx=16, pady=8)

        # RSSI
        rssi_row = ctk.CTkFrame(meters_frame, fg_color="transparent")
        rssi_row.pack(fill="x", pady=2)
        ctk.CTkLabel(rssi_row, text="Signal (RSSI):", font=ctk.CTkFont(size=11), text_color="#94A3B8", width=80, anchor="w").pack(side="left")
        self.rssi_bar = ctk.CTkProgressBar(rssi_row, height=8, progress_color="#38BDF8", fg_color="#334155")
        self.rssi_bar.pack(side="left", fill="x", expand=True, padx=6)
        self.rssi_bar.set(0)
        self.rssi_val_label = ctk.CTkLabel(rssi_row, text="0 / 5", font=ctk.CTkFont(size=11), text_color="#94A3B8", width=35)
        self.rssi_val_label.pack(side="right")

        # Audio VU
        audio_row = ctk.CTkFrame(meters_frame, fg_color="transparent")
        audio_row.pack(fill="x", pady=2)
        ctk.CTkLabel(audio_row, text="Audio Level:", font=ctk.CTkFont(size=11), text_color="#94A3B8", width=80, anchor="w").pack(side="left")
        self.audio_bar = ctk.CTkProgressBar(audio_row, height=8, progress_color="#10B981", fg_color="#334155")
        self.audio_bar.pack(side="left", fill="x", expand=True, padx=6)
        self.audio_bar.set(0)
        self.audio_val_label = ctk.CTkLabel(audio_row, text="0%", font=ctk.CTkFont(size=11), text_color="#94A3B8", width=35)
        self.audio_val_label.pack(side="right")

        # Controls Toolbar
        btn_row = ctk.CTkFrame(self, fg_color="transparent")
        btn_row.pack(fill="x", padx=16, pady=(8, 14))

        self.btn_skip = ctk.CTkButton(
            btn_row,
            text="⏭ Skip / Next",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#0284C7",
            hover_color="#0369A1",
            height=32,
            command=lambda: self.on_skip(self.scanner_id)
        )
        self.btn_skip.pack(side="left", fill="x", expand=True, padx=(0, 6))

        self.btn_avoid = ctk.CTkButton(
            btn_row,
            text="🚫 Temp Avoid",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#475569",
            hover_color="#DC2626",
            height=32,
            command=lambda: self.on_avoid(self.scanner_id)
        )
        self.btn_avoid.pack(side="left", fill="x", expand=True, padx=(6, 0))

    def _on_combo_model_selected(self, new_model: str):
        if self.on_model_change:
            self.on_model_change(self.scanner_id, new_model)

    def update_telemetry(self, state: Dict[str, Any], audio_level: float, recording_enabled: bool = True):
        if not state:
            return

        connected = state.get("connected", False)
        receiving = state.get("receiving", False)
        avoided = state.get("avoided", False)

        # Update Recording Badge
        if not recording_enabled:
            self.rec_badge.configure(text="REC OFF", fg_color="#334155", text_color="#94A3B8")
        elif receiving:
            self.rec_badge.configure(text="🔴 REC", fg_color="#991B1B", text_color="#FECACA")
        else:
            self.rec_badge.configure(text="REC ON", fg_color="#14532D", text_color="#86EFAC")

        # Update Status Badge
        if not connected:
            self.status_pill.configure(text="DISCONNECTED", fg_color="#334155", text_color="#94A3B8")
        elif receiving:
            self.status_pill.configure(text="● TRANSMITTING", fg_color="#047857", text_color="#A7F3D0")
        elif avoided:
            self.status_pill.configure(text="⛔ MUTED / AVOIDED", fg_color="#B91C1C", text_color="#FECACA")
        else:
            self.status_pill.configure(text="● MONITORING", fg_color="#1E3A8A", text_color="#BAE6FD")

        # Sync detected hardware model if reported by scanner
        detected_model = state.get("model")
        if connected and detected_model and detected_model in POPULAR_SCANNER_MODELS:
            if hasattr(self, "combo_model") and self.combo_model.get() != detected_model:
                self.combo_model.set(detected_model)

        # Clean text and prevent multi-line expansions
        def _clean(val: Any, default: str, max_len: int) -> str:
            t = str(val or default).replace("\n", " ").replace("\r", "").strip()
            return (t[:max_len - 2] + "..") if len(t) > max_len else t

        sys_name = _clean(state.get("system_name"), "Scanning...", 38)
        dept_name = _clean(state.get("dept_name"), "-", 38)
        ch_name = _clean(state.get("channel_name"), "Idle", 28)
        freq = state.get("frequency")
        tgid = _clean(state.get("tgid"), "-", 14)
        mod = _clean(state.get("modulation"), "NFM", 8)
        status_line = _clean(state.get("status_line"), "Ready", 36)

        self.lbl_system.configure(text=f"System: {sys_name}")
        self.lbl_dept.configure(text=f"Dept: {dept_name}")
        self.lbl_channel.configure(text=ch_name)

        if freq:
            self.lbl_freq.configure(text=f"{freq:.4f} MHz")
        else:
            self.lbl_freq.configure(text="---.---- MHz")

        self.lbl_tgid.configure(text=f"TGID: {tgid}")
        self.lbl_mod.configure(text=f"MOD: {mod}")
        self.lbl_status.configure(text=f"STATUS: {status_line}")

        # Update Signal RSSI
        rssi = state.get("rssi", 0)
        rssi_norm = min(1.0, max(0.0, rssi / 5.0))
        self.rssi_bar.set(rssi_norm)
        self.rssi_val_label.configure(text=f"{rssi} / 5")

        # Update Audio VU Meter
        audio_norm = min(1.0, max(0.0, audio_level / 100.0))
        self.audio_bar.set(audio_norm)
        self.audio_val_label.configure(text=f"{int(audio_level)}%")
        if audio_norm > 0.85:
            self.audio_bar.configure(progress_color="#EF4444")  # Red peak
        elif audio_norm > 0.6:
            self.audio_bar.configure(progress_color="#F59E0B")  # Yellow
        else:
            self.audio_bar.configure(progress_color="#10B981")  # Green


class ScannerControllerApp(ctk.CTk):
    """Main Native Desktop Application Window for Scanner Controller."""
    def __init__(self):
        super().__init__()

        self.title("Uniden Scanner Mutual Exclusion Controller — Pro Console")
        self.geometry("1120x860")
        self.minsize(980, 750)
        self.configure(fg_color="#0F172A")

        # Initialize App Configuration & Background Engine
        self.config = load_config()
        self.engine = BackgroundEngine(self.config)
        self.engine.start()

        self.last_log_count = 0
        self._build_ui()

        # Telemetry Refresh Loop (100ms interval for smooth VU meters and real-time audio)
        self.after(100, self._telemetry_refresh_loop)

        # Handle window close cleanly
        self.protocol("WM_DELETE_WINDOW", self._on_close)

    def _build_ui(self):
        # 1. Top Global Header Frame
        header = ctk.CTkFrame(self, fg_color="#1E293B", corner_radius=0, height=70)
        header.pack(fill="x", side="top")

        # Brand Title
        brand_frame = ctk.CTkFrame(header, fg_color="transparent")
        brand_frame.pack(side="left", padx=20, pady=12)

        title = ctk.CTkLabel(
            brand_frame,
            text="SCANSCRIBE CONTROLLER",
            font=ctk.CTkFont(family="Segoe UI", size=18, weight="bold"),
            text_color="#38BDF8"
        )
        title.pack(anchor="w")

        subtitle = ctk.CTkLabel(
            brand_frame,
            text="Uniden Dual-Radio Dynamic Mutual Exclusion Engine",
            font=ctk.CTkFont(family="Segoe UI", size=11),
            text_color="#94A3B8"
        )
        subtitle.pack(anchor="w")

        # Global Sync Status Banner Pill
        self.sync_status_badge = ctk.CTkLabel(
            header,
            text="● MUTUAL EXCLUSION ACTIVE",
            font=ctk.CTkFont(family="Segoe UI", size=12, weight="bold"),
            fg_color="#065F46",
            text_color="#6EE7B7",
            corner_radius=8,
            padx=14,
            pady=6
        )
        self.sync_status_badge.pack(side="left", padx=16)

        # Right-side Header Controls: Priority Mode, Sync Toggle, Open Web UI
        right_header = ctk.CTkFrame(header, fg_color="transparent")
        right_header.pack(side="right", padx=20, pady=12)

        # Priority Mode Combo
        ctk.CTkLabel(right_header, text="Priority:", font=ctk.CTkFont(size=12), text_color="#94A3B8").pack(side="left", padx=(0, 6))
        self.combo_priority = ctk.CTkComboBox(
            right_header,
            values=["EQUAL_PEERS", "PRIMARY_SECONDARY", "DEPARTMENT_PRIORITY"],
            font=ctk.CTkFont(size=11),
            width=165,
            command=self._on_priority_changed
        )
        self.combo_priority.set(self.config.sync.priority_mode.value)
        self.combo_priority.pack(side="left", padx=(0, 14))

        # Sync Enable Switch
        self.switch_sync = ctk.CTkSwitch(
            right_header,
            text="Sync Enabled",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#F8FAFC",
            command=self._on_sync_toggled
        )
        self.switch_sync.select()
        self.switch_sync.pack(side="left", padx=(0, 12))

        # Record Enable Switch (Master Call Recording Toggle)
        self.switch_record = ctk.CTkSwitch(
            right_header,
            text="🔴 Record Calls",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#F8FAFC",
            progress_color="#DC2626",
            command=self._on_record_toggled
        )
        if self.config.feeder.enable_feeder:
            self.switch_record.select()
        else:
            self.switch_record.deselect()
        self.switch_record.pack(side="left", padx=(0, 14))

        # Open Web UI Button
        btn_web = ctk.CTkButton(
            right_header,
            text="🌐 Web Dashboard",
            font=ctk.CTkFont(size=11, weight="bold"),
            fg_color="#334155",
            hover_color="#475569",
            width=120,
            height=30,
            command=self._open_web_ui
        )
        btn_web.pack(side="left")

        # 2. Main Content Container
        main_content = ctk.CTkFrame(self, fg_color="transparent")
        main_content.pack(fill="both", expand=True, padx=20, pady=(16, 20))

        # Dual Scanner Display Cards (Locked equal columns and row)
        cards_grid = ctk.CTkFrame(main_content, fg_color="transparent")
        cards_grid.pack(fill="x", pady=(0, 16))

        cards_grid.grid_columnconfigure(0, weight=1, uniform="radiocards")
        cards_grid.grid_columnconfigure(1, weight=1, uniform="radiocards")
        cards_grid.grid_rowconfigure(0, weight=1)

        self.card_a = ScannerCardFrame(
            cards_grid,
            scanner_id="scanner_a",
            title="Radio A",
            initial_model=self.config.scanner_a.model.value,
            on_skip=self.engine.send_skip,
            on_avoid=self.engine.send_avoid,
            on_model_change=self._on_model_changed_from_card
        )
        self.card_a.grid(row=0, column=0, sticky="nsew", padx=(0, 10))

        self.card_b = ScannerCardFrame(
            cards_grid,
            scanner_id="scanner_b",
            title="Radio B",
            initial_model=self.config.scanner_b.model.value,
            on_skip=self.engine.send_skip,
            on_avoid=self.engine.send_avoid,
            on_model_change=self._on_model_changed_from_card
        )
        self.card_b.grid(row=0, column=1, sticky="nsew", padx=(10, 0))

        # 3. Bottom Tabs: Setup & Discovery / Feeder / Event Log
        self.tabview = ctk.CTkTabview(
            main_content,
            fg_color="#1E293B",
            segmented_button_selected_color="#0284C7",
            segmented_button_selected_hover_color="#0369A1",
            segmented_button_unselected_color="#334155",
            corner_radius=10
        )
        self.tabview.pack(fill="both", expand=True)

        self.tab_ports = self.tabview.add("🔌 Hardware & COM Ports")
        self.tab_feeder = self.tabview.add("📡 ScanScribe Feeder")
        self.tab_log = self.tabview.add("📜 Mutual Exclusion Log")

        self._build_tab_ports()
        self._build_tab_feeder()
        self._build_tab_log()

    def _build_tab_ports(self):
        """Hardware ports configuration and auto-detection."""
        scroll_ports = ctk.CTkScrollableFrame(self.tab_ports, fg_color="transparent")
        scroll_ports.pack(fill="both", expand=True)
        frame = scroll_ports

        # Row 1: Action bar with Auto-Detect Button
        act_bar = ctk.CTkFrame(frame, fg_color="transparent")
        act_bar.pack(fill="x", padx=16, pady=(12, 16))

        btn_detect = ctk.CTkButton(
            act_bar,
            text="🔍 Auto-Detect Connected Radios",
            font=ctk.CTkFont(size=13, weight="bold"),
            fg_color="#0284C7",
            hover_color="#0369A1",
            height=34,
            command=self._run_port_detection
        )
        btn_detect.pack(side="left")

        self.lbl_detect_result = ctk.CTkLabel(
            act_bar,
            text="Click 'Auto-Detect' to scan Windows COM ports for Uniden scanners.",
            font=ctk.CTkFont(size=12),
            text_color="#94A3B8"
        )
        self.lbl_detect_result.pack(side="left", padx=16)

        # Settings Grid (Scanner A, Scanner B, Audio Device)
        grid = ctk.CTkFrame(frame, fg_color="#0F172A", corner_radius=8, border_width=1, border_color="#334155")
        grid.pack(fill="x", padx=16, pady=4)
        grid.grid_columnconfigure((0, 1, 2, 3), weight=1)

        # Audio Devices Map
        self.audio_devices_map: Dict[str, Optional[int]] = {
            "Default OS Input": None,
            "Disabled / Mute": -1
        }
        for dev in list_audio_input_devices():
            label = f"[{dev['index']}] {dev['name']} ({dev['channels']} ch)"
            self.audio_devices_map[label] = dev['index']

        audio_options = list(self.audio_devices_map.keys())

        # Determine initial selection for Scanner A
        initial_audio_a = "Disabled / Mute"
        if self.config.scanner_a.audio_enabled:
            if self.config.scanner_a.audio_device_index is None:
                initial_audio_a = "Default OS Input"
            else:
                for k, v in self.audio_devices_map.items():
                    if v == self.config.scanner_a.audio_device_index:
                        initial_audio_a = k
                        break

        # Determine initial selection for Scanner B
        initial_audio_b = "Disabled / Mute"
        if self.config.scanner_b.audio_enabled:
            if self.config.scanner_b.audio_device_index is None:
                initial_audio_b = "Default OS Input"
            else:
                for k, v in self.audio_devices_map.items():
                    if v == self.config.scanner_b.audio_device_index:
                        initial_audio_b = k
                        break

        # --- Scanner A Section ---
        ctk.CTkLabel(grid, text="Scanner A Model:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=0, column=0, padx=12, pady=(10, 4), sticky="w")
        self.combo_model_a = ctk.CTkComboBox(grid, values=POPULAR_SCANNER_MODELS, command=lambda m: self._on_model_changed_from_card("scanner_a", m))
        self.combo_model_a.set(self.config.scanner_a.model.value)
        self.combo_model_a.grid(row=0, column=1, padx=12, pady=(10, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Scanner A Port:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=0, column=2, padx=12, pady=(10, 4), sticky="w")
        self.combo_port_a = ctk.CTkComboBox(grid, values=["COM1", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "NONE"])
        self.combo_port_a.set(self.config.scanner_a.port)
        self.combo_port_a.grid(row=0, column=3, padx=12, pady=(10, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Baud Rate:", text_color="#94A3B8").grid(row=1, column=0, padx=12, pady=(4, 4), sticky="w")
        self.combo_baud_a = ctk.CTkComboBox(grid, values=["115200", "57600", "38400", "19200", "9600"])
        self.combo_baud_a.set(str(self.config.scanner_a.baud_rate))
        self.combo_baud_a.grid(row=1, column=1, padx=12, pady=(4, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Channel Demux:", text_color="#94A3B8").grid(row=1, column=2, padx=12, pady=(4, 4), sticky="w")
        self.combo_chan_a = ctk.CTkComboBox(grid, values=["MONO", "LEFT", "RIGHT"])
        self.combo_chan_a.set(self.config.scanner_a.audio_channel.value if hasattr(self.config.scanner_a.audio_channel, "value") else str(self.config.scanner_a.audio_channel))
        self.combo_chan_a.grid(row=1, column=3, padx=12, pady=(4, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Scanner A Audio Dev:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=2, column=0, padx=12, pady=(4, 12), sticky="w")
        self.combo_audio_a = ctk.CTkComboBox(grid, values=audio_options)
        self.combo_audio_a.set(initial_audio_a)
        self.combo_audio_a.grid(row=2, column=1, columnspan=3, padx=12, pady=(4, 12), sticky="ew")

        # --- Scanner B Section ---
        ctk.CTkLabel(grid, text="Scanner B Model:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=3, column=0, padx=12, pady=(10, 4), sticky="w")
        self.combo_model_b = ctk.CTkComboBox(grid, values=POPULAR_SCANNER_MODELS, command=lambda m: self._on_model_changed_from_card("scanner_b", m))
        self.combo_model_b.set(self.config.scanner_b.model.value)
        self.combo_model_b.grid(row=3, column=1, padx=12, pady=(10, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Scanner B Port:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=3, column=2, padx=12, pady=(10, 4), sticky="w")
        self.combo_port_b = ctk.CTkComboBox(grid, values=["NONE", "COM1", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8"])
        self.combo_port_b.set(self.config.scanner_b.port)
        self.combo_port_b.grid(row=3, column=3, padx=12, pady=(10, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Baud Rate:", text_color="#94A3B8").grid(row=4, column=0, padx=12, pady=(4, 4), sticky="w")
        self.combo_baud_b = ctk.CTkComboBox(grid, values=["115200", "57600", "38400", "19200", "9600"])
        self.combo_baud_b.set(str(self.config.scanner_b.baud_rate))
        self.combo_baud_b.grid(row=4, column=1, padx=12, pady=(4, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Channel Demux:", text_color="#94A3B8").grid(row=4, column=2, padx=12, pady=(4, 4), sticky="w")
        self.combo_chan_b = ctk.CTkComboBox(grid, values=["MONO", "LEFT", "RIGHT"])
        self.combo_chan_b.set(self.config.scanner_b.audio_channel.value if hasattr(self.config.scanner_b.audio_channel, "value") else str(self.config.scanner_b.audio_channel))
        self.combo_chan_b.grid(row=4, column=3, padx=12, pady=(4, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Scanner B Audio Dev:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=5, column=0, padx=12, pady=(4, 12), sticky="w")
        self.combo_audio_b = ctk.CTkComboBox(grid, values=audio_options)
        self.combo_audio_b.set(initial_audio_b)
        self.combo_audio_b.grid(row=5, column=1, columnspan=3, padx=12, pady=(4, 12), sticky="ew")

        # --- Playback Loopback Monitor Section ---
        self.audio_output_devices_map: Dict[str, Optional[int]] = {
            "Default OS Output": None,
            "Disabled / No Monitoring": -1
        }
        for dev in list_audio_output_devices():
            label = f"[{dev['index']}] {dev['name']} ({dev['channels']} ch)"
            self.audio_output_devices_map[label] = dev['index']

        output_options = list(self.audio_output_devices_map.keys())

        initial_audio_out = "Default OS Output"
        if not self.config.audio_loopback.enabled:
            initial_audio_out = "Disabled / No Monitoring"
        elif self.config.audio_loopback.output_device_index is not None:
            for k, v in self.audio_output_devices_map.items():
                if v == self.config.audio_loopback.output_device_index:
                    initial_audio_out = k
                    break

        ctk.CTkLabel(grid, text="🔊 Playback Output Dev:", font=ctk.CTkFont(weight="bold"), text_color="#38BDF8").grid(row=6, column=0, padx=12, pady=(10, 4), sticky="w")
        self.combo_audio_out = ctk.CTkComboBox(grid, values=output_options)
        self.combo_audio_out.set(initial_audio_out)
        self.combo_audio_out.grid(row=6, column=1, padx=12, pady=(10, 4), sticky="ew")

        ctk.CTkLabel(grid, text="Monitor Pan Mode:", text_color="#94A3B8").grid(row=6, column=2, padx=12, pady=(10, 4), sticky="w")
        self.combo_pan_mode = ctk.CTkComboBox(grid, values=["STEREO_SPLIT (A=Left, B=Right)", "CENTER (Both Centered)"])
        self.combo_pan_mode.set("STEREO_SPLIT (A=Left, B=Right)" if self.config.audio_loopback.pan_mode == "STEREO_SPLIT" else "CENTER (Both Centered)")
        self.combo_pan_mode.grid(row=6, column=3, padx=12, pady=(10, 4), sticky="ew")

        # Volume slider & Live Speaker Monitor Switch
        ctk.CTkLabel(grid, text="Playback Volume:", font=ctk.CTkFont(weight="bold"), text_color="#E2E8F0").grid(row=7, column=0, padx=12, pady=(4, 14), sticky="w")
        vol_frame = ctk.CTkFrame(grid, fg_color="transparent")
        vol_frame.grid(row=7, column=1, padx=12, pady=(4, 14), sticky="ew")

        init_vol_pct = int(self.config.audio_loopback.volume * 100)
        self.lbl_volume = ctk.CTkLabel(vol_frame, text=f"{init_vol_pct}%", width=40, font=ctk.CTkFont(size=11), text_color="#38BDF8")
        self.slider_volume = ctk.CTkSlider(vol_frame, from_=0, to=100, number_of_steps=100, command=self._on_volume_slider_changed)
        self.slider_volume.set(init_vol_pct)
        self.slider_volume.pack(side="left", fill="x", expand=True, padx=(0, 6))
        self.lbl_volume.pack(side="right")

        self.switch_loopback_enable = ctk.CTkSwitch(
            grid,
            text="Enable Live Audio Monitoring",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#F8FAFC"
        )
        if self.config.audio_loopback.enabled:
            self.switch_loopback_enable.select()
        else:
            self.switch_loopback_enable.deselect()
        self.switch_loopback_enable.grid(row=7, column=2, columnspan=2, padx=12, pady=(4, 14), sticky="w")

        # Save Button
        btn_save = ctk.CTkButton(
            frame,
            text="💾 Save & Apply Port & Audio Settings",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#059669",
            hover_color="#047857",
            height=34,
            command=self._apply_hardware_settings
        )
        btn_save.pack(anchor="e", padx=16, pady=14)

    def _build_tab_feeder(self):
        """ScanScribe Feeder and Directory Drop integration."""
        scroll_feeder = ctk.CTkScrollableFrame(self.tab_feeder, fg_color="transparent")
        scroll_feeder.pack(fill="both", expand=True)
        frame = scroll_feeder

        # Master recording row
        row_rec = ctk.CTkFrame(frame, fg_color="transparent")
        row_rec.pack(fill="x", padx=16, pady=(12, 6))
        self.switch_feeder_rec = ctk.CTkSwitch(
            row_rec,
            text="Enable ScanScribe Call Transmission Recording & Ingestion",
            font=ctk.CTkFont(size=13, weight="bold"),
            text_color="#F8FAFC",
            progress_color="#DC2626",
            command=self._on_feeder_rec_toggled
        )
        if self.config.feeder.enable_feeder:
            self.switch_feeder_rec.select()
        else:
            self.switch_feeder_rec.deselect()
        self.switch_feeder_rec.pack(side="left")

        # Mode row
        row_mode = ctk.CTkFrame(frame, fg_color="transparent")
        row_mode.pack(fill="x", padx=16, pady=(10, 6))
        ctk.CTkLabel(row_mode, text="Feeder Ingestion Mode:", font=ctk.CTkFont(weight="bold"), width=160, anchor="w").pack(side="left")
        self.combo_feeder_mode = ctk.CTkComboBox(row_mode, values=["DIRECTORY_DROP", "HTTP_WEBHOOK", "LIVE_STREAM"], width=200)
        self.combo_feeder_mode.set(self.config.feeder.feeder_mode.value)
        self.combo_feeder_mode.pack(side="left")

        # JSON Sidecar switch row
        row_json = ctk.CTkFrame(frame, fg_color="transparent")
        row_json.pack(fill="x", padx=16, pady=(4, 6))
        self.switch_sidecar_json = ctk.CTkSwitch(
            row_json,
            text="Generate JSON Metadata Sidecars (*.json) alongside recordings",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#94A3B8"
        )
        if getattr(self.config.feeder, "enable_sidecar_json", True):
            self.switch_sidecar_json.select()
        else:
            self.switch_sidecar_json.deselect()
        self.switch_sidecar_json.pack(side="left")

        self.switch_sidecar_json.pack(side="left")

        # --- Scanner A Filename & TIT2 Patterns Card ---
        fmt_card_a = ctk.CTkFrame(frame, fg_color="#0F172A", corner_radius=8, border_width=1, border_color="#0284C7")
        fmt_card_a.pack(fill="x", padx=16, pady=6)

        row_a_title = ctk.CTkFrame(fmt_card_a, fg_color="transparent")
        row_a_title.pack(fill="x", padx=12, pady=(8, 2))
        self.lbl_card_a_title = ctk.CTkLabel(
            row_a_title,
            text=f"📡 Scanner A ({self.config.scanner_a.model.value}) — Filename Template & TIT2 Title Tag",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#38BDF8"
        )
        self.lbl_card_a_title.pack(side="left")

        # Scanner A Filename Row
        row_tmpl_a = ctk.CTkFrame(fmt_card_a, fg_color="transparent")
        row_tmpl_a.pack(fill="x", padx=12, pady=3)
        ctk.CTkLabel(row_tmpl_a, text="Filename Template:", font=ctk.CTkFont(weight="bold"), width=140, anchor="w").pack(side="left")
        self.entry_filename_template_a = ctk.CTkEntry(row_tmpl_a)
        tmpl_a_val = getattr(self.config.scanner_a, "filename_template", None) or getattr(self.config.feeder, "filename_template", "%DT - %S - %C") or "%DT - %S - %C"
        self.entry_filename_template_a.insert(0, tmpl_a_val)
        self.entry_filename_template_a.pack(side="left", fill="x", expand=True, padx=(0, 8))
        self.entry_filename_template_a.bind("<KeyRelease>", lambda e: self._update_filename_preview("scanner_a"))

        btn_pa1 = ctk.CTkButton(row_tmpl_a, text="Default (%DT - %S - %C)", width=145, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_filename_preset("scanner_a", "%DT - %S - %C"))
        btn_pa1.pack(side="left", padx=2)
        btn_pa2 = ctk.CTkButton(row_tmpl_a, text="Full (%DT_%ST_%S_%C_%TG)", width=150, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_filename_preset("scanner_a", "%DT_%ST_%S_%C_%TG"))
        btn_pa2.pack(side="left", padx=2)
        btn_pa3 = ctk.CTkButton(row_tmpl_a, text="%TT %D %C", width=95, height=24, fg_color="#0284C7", hover_color="#0369A1", command=lambda: self._set_filename_preset("scanner_a", "%TT %D %C"))
        btn_pa3.pack(side="left", padx=2)

        # Scanner A Filename Preview
        row_prev_a = ctk.CTkFrame(fmt_card_a, fg_color="transparent")
        row_prev_a.pack(fill="x", padx=12, pady=(1, 4))
        self.lbl_filename_preview_a = ctk.CTkLabel(
            row_prev_a,
            text="Scanner A Preview: ...",
            font=ctk.CTkFont(size=11, family="Consolas", weight="bold"),
            text_color="#38BDF8",
            anchor="w"
        )
        self.lbl_filename_preview_a.pack(fill="x")

        # Scanner A TIT2 Row
        row_tit2_a = ctk.CTkFrame(fmt_card_a, fg_color="transparent")
        row_tit2_a.pack(fill="x", padx=12, pady=3)
        ctk.CTkLabel(row_tit2_a, text="TIT2 (Title) Tag:", font=ctk.CTkFont(weight="bold"), width=140, anchor="w").pack(side="left")
        self.entry_tit2_template_a = ctk.CTkEntry(row_tit2_a)
        tit2_a_val = getattr(self.config.scanner_a, "tit2_template", None) or getattr(self.config.feeder, "tit2_template", "%C (%TG)") or "%C (%TG)"
        self.entry_tit2_template_a.insert(0, tit2_a_val)
        self.entry_tit2_template_a.pack(side="left", fill="x", expand=True, padx=(0, 8))
        self.entry_tit2_template_a.bind("<KeyRelease>", lambda e: self._update_tit2_preview("scanner_a"))

        btn_ta1 = ctk.CTkButton(row_tit2_a, text="Default (%C (%TG))", width=145, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_tit2_preset("scanner_a", "%C (%TG)"))
        btn_ta1.pack(side="left", padx=2)
        btn_ta2 = ctk.CTkButton(row_tit2_a, text="TG/Tone (%TG - %C - %T)", width=150, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_tit2_preset("scanner_a", "%TG - %C - %T"))
        btn_ta2.pack(side="left", padx=2)
        btn_ta3 = ctk.CTkButton(row_tit2_a, text="%TG %G %C", width=95, height=24, fg_color="#0284C7", hover_color="#0369A1", command=lambda: self._set_tit2_preset("scanner_a", "%TG %G %C"))
        btn_ta3.pack(side="left", padx=2)

        # Scanner A TIT2 Preview
        row_tit2_prev_a = ctk.CTkFrame(fmt_card_a, fg_color="transparent")
        row_tit2_prev_a.pack(fill="x", padx=12, pady=(1, 8))
        self.lbl_tit2_preview_a = ctk.CTkLabel(
            row_tit2_prev_a,
            text="Scanner A TIT2 Preview: ...",
            font=ctk.CTkFont(size=11, family="Consolas", weight="bold"),
            text_color="#A78BFA",
            anchor="w"
        )
        self.lbl_tit2_preview_a.pack(fill="x")

        # --- Scanner B Filename & TIT2 Patterns Card ---
        fmt_card_b = ctk.CTkFrame(frame, fg_color="#0F172A", corner_radius=8, border_width=1, border_color="#7C3AED")
        fmt_card_b.pack(fill="x", padx=16, pady=6)

        row_b_title = ctk.CTkFrame(fmt_card_b, fg_color="transparent")
        row_b_title.pack(fill="x", padx=12, pady=(8, 2))
        self.lbl_card_b_title = ctk.CTkLabel(
            row_b_title,
            text=f"📡 Scanner B ({self.config.scanner_b.model.value}) — Filename Template & TIT2 Title Tag",
            font=ctk.CTkFont(size=12, weight="bold"),
            text_color="#C084FC"
        )
        self.lbl_card_b_title.pack(side="left")

        # Scanner B Filename Row
        row_tmpl_b = ctk.CTkFrame(fmt_card_b, fg_color="transparent")
        row_tmpl_b.pack(fill="x", padx=12, pady=3)
        ctk.CTkLabel(row_tmpl_b, text="Filename Template:", font=ctk.CTkFont(weight="bold"), width=140, anchor="w").pack(side="left")
        self.entry_filename_template_b = ctk.CTkEntry(row_tmpl_b)
        tmpl_b_val = getattr(self.config.scanner_b, "filename_template", None) or "%DT - %S - %C (%TG)"
        self.entry_filename_template_b.insert(0, tmpl_b_val)
        self.entry_filename_template_b.pack(side="left", fill="x", expand=True, padx=(0, 8))
        self.entry_filename_template_b.bind("<KeyRelease>", lambda e: self._update_filename_preview("scanner_b"))

        btn_pb1 = ctk.CTkButton(row_tmpl_b, text="Default (%DT - %S - %C)", width=145, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_filename_preset("scanner_b", "%DT - %S - %C"))
        btn_pb1.pack(side="left", padx=2)
        btn_pb2 = ctk.CTkButton(row_tmpl_b, text="Full (%DT_%ST_%S_%C_%TG)", width=150, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_filename_preset("scanner_b", "%DT_%ST_%S_%C_%TG"))
        btn_pb2.pack(side="left", padx=2)
        btn_pb3 = ctk.CTkButton(row_tmpl_b, text="%TT %D %C", width=95, height=24, fg_color="#7C3AED", hover_color="#6D28D9", command=lambda: self._set_filename_preset("scanner_b", "%TT %D %C"))
        btn_pb3.pack(side="left", padx=2)

        # Scanner B Filename Preview
        row_prev_b = ctk.CTkFrame(fmt_card_b, fg_color="transparent")
        row_prev_b.pack(fill="x", padx=12, pady=(1, 4))
        self.lbl_filename_preview_b = ctk.CTkLabel(
            row_prev_b,
            text="Scanner B Preview: ...",
            font=ctk.CTkFont(size=11, family="Consolas", weight="bold"),
            text_color="#C084FC",
            anchor="w"
        )
        self.lbl_filename_preview_b.pack(fill="x")

        # Scanner B TIT2 Row
        row_tit2_b = ctk.CTkFrame(fmt_card_b, fg_color="transparent")
        row_tit2_b.pack(fill="x", padx=12, pady=3)
        ctk.CTkLabel(row_tit2_b, text="TIT2 (Title) Tag:", font=ctk.CTkFont(weight="bold"), width=140, anchor="w").pack(side="left")
        self.entry_tit2_template_b = ctk.CTkEntry(row_tit2_b)
        tit2_b_val = getattr(self.config.scanner_b, "tit2_template", None) or "%C (%TG)"
        self.entry_tit2_template_b.insert(0, tit2_b_val)
        self.entry_tit2_template_b.pack(side="left", fill="x", expand=True, padx=(0, 8))
        self.entry_tit2_template_b.bind("<KeyRelease>", lambda e: self._update_tit2_preview("scanner_b"))

        btn_tb1 = ctk.CTkButton(row_tit2_b, text="Default (%C (%TG))", width=145, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_tit2_preset("scanner_b", "%C (%TG)"))
        btn_tb1.pack(side="left", padx=2)
        btn_tb2 = ctk.CTkButton(row_tit2_b, text="TG/Tone (%TG - %C - %T)", width=150, height=24, fg_color="#334155", hover_color="#475569", command=lambda: self._set_tit2_preset("scanner_b", "%TG - %C - %T"))
        btn_tb2.pack(side="left", padx=2)
        btn_tb3 = ctk.CTkButton(row_tit2_b, text="%TG %G %C", width=95, height=24, fg_color="#7C3AED", hover_color="#6D28D9", command=lambda: self._set_tit2_preset("scanner_b", "%TG %G %C"))
        btn_tb3.pack(side="left", padx=2)

        # Scanner B TIT2 Preview
        row_tit2_prev_b = ctk.CTkFrame(fmt_card_b, fg_color="transparent")
        row_tit2_prev_b.pack(fill="x", padx=12, pady=(1, 8))
        self.lbl_tit2_preview_b = ctk.CTkLabel(
            row_tit2_prev_b,
            text="Scanner B TIT2 Preview: ...",
            font=ctk.CTkFont(size=11, family="Consolas", weight="bold"),
            text_color="#E9D5FF",
            anchor="w"
        )
        self.lbl_tit2_preview_b.pack(fill="x")

        self._update_filename_preview("scanner_a")
        self._update_filename_preview("scanner_b")
        self._update_tit2_preview("scanner_a")
        self._update_tit2_preview("scanner_b")

        # ProScan Specifiers Reference Card
        ref_card = ctk.CTkFrame(frame, fg_color="#1E293B", corner_radius=6)
        ref_card.pack(fill="x", padx=16, pady=(4, 10))

        spec_help = (
            "ProScan Custom Formatters (General.cs:33805 — 100% Bit-Exact Match):\n"
            "  %ST  : Scanner Type (BCD436HP) | %C   : Channel Name             | %U1  : Unit ID Before Lookup\n"
            "  %DT  : Date / Time             | %FT  : Freq / Talk Group ID     | %U2  : Unit ID After Lookup\n"
            "  %D   : Date (MM/dd/yy)         | %F   : Frequency (+ DMR slot)   | %U3  : Unit ID #\n"
            "  %TT  : Time (HH:mm:ss)         | %T   : Tone / CTCSS / DCS       | %MD  : Modulation (NFM)\n"
            "  %H   : Hour (24h)              | %TG  : Talk Group ID            | %R   : RSSI Signal Level\n"
            "  %P   : Port (bare digits)      | %TA  : Tone-Out A               | %TB  : Tone-Out B\n"
            "  %DD  : Day (2-digit)           | %AC  : Alert Color              | %ST1 : Service Type\n"
            "  %M   : Month (Number, 09)      | %STC : Service Type Color       | %SC  : Server & Client Status\n"
            "  %MM  : Month (Name, Sep)       | %AF1 : App Folder (Full)        | %AF2 : App Folder (Partial)\n"
            "  %Y   : Year (2-digit, 26)      | %YY  : Year (4-digit, 2026)     | %FA  : Favorite Name\n"
            "  %S   : System Name             | %SI  : Site Name                | %DE  : Department Name"
        )
        ctk.CTkLabel(
            ref_card,
            text=spec_help,
            font=ctk.CTkFont(size=11, family="Consolas"),
            text_color="#94A3B8",
            justify="left",
            anchor="w"
        ).pack(padx=10, pady=8, fill="x")

        # Inbox Dir row
        row_inbox = ctk.CTkFrame(frame, fg_color="transparent")
        row_inbox.pack(fill="x", padx=16, pady=8)
        ctk.CTkLabel(row_inbox, text="Inbox Directory:", font=ctk.CTkFont(weight="bold"), width=160, anchor="w").pack(side="left")
        self.entry_inbox = ctk.CTkEntry(row_inbox)
        self.entry_inbox.insert(0, self.config.feeder.inbox_directory)
        self.entry_inbox.pack(side="left", fill="x", expand=True, padx=(0, 8))
        btn_browse = ctk.CTkButton(row_inbox, text="Browse...", width=90, command=self._browse_inbox_dir)
        btn_browse.pack(side="right")

        # Webhook row
        row_hook = ctk.CTkFrame(frame, fg_color="transparent")
        row_hook.pack(fill="x", padx=16, pady=8)
        ctk.CTkLabel(row_hook, text="Webhook URL:", font=ctk.CTkFont(weight="bold"), width=160, anchor="w").pack(side="left")
        self.entry_webhook = ctk.CTkEntry(row_hook)
        self.entry_webhook.insert(0, self.config.feeder.webhook_url)
        self.entry_webhook.pack(side="left", fill="x", expand=True)

        # Actions row
        act_row = ctk.CTkFrame(frame, fg_color="transparent")
        act_row.pack(fill="x", padx=16, pady=(16, 12))

        btn_test_a = ctk.CTkButton(
            act_row,
            text="🚀 Test Scanner A Feed",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#0284C7",
            hover_color="#0369A1",
            command=lambda: self._dispatch_test_feeder("scanner_a")
        )
        btn_test_a.pack(side="left", padx=(0, 8))

        btn_test_b = ctk.CTkButton(
            act_row,
            text="🚀 Test Scanner B Feed",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#7C3AED",
            hover_color="#6D28D9",
            command=lambda: self._dispatch_test_feeder("scanner_b")
        )
        btn_test_b.pack(side="left")

        btn_save_feeder = ctk.CTkButton(
            act_row,
            text="💾 Save Feeder & Scanner Configs",
            font=ctk.CTkFont(size=12, weight="bold"),
            fg_color="#059669",
            hover_color="#047857",
            command=self._save_feeder_config
        )
        btn_save_feeder.pack(side="right")

        self._update_filename_preview()


    def _build_tab_log(self):
        """Mutual exclusion events scrollable log."""
        frame = self.tab_log

        tb_frame = ctk.CTkFrame(frame, fg_color="transparent")
        tb_frame.pack(fill="x", padx=16, pady=(10, 6))

        ctk.CTkLabel(tb_frame, text="Live Lockout & Mutual Exclusion Audit Stream", font=ctk.CTkFont(weight="bold"), text_color="#94A3B8").pack(side="left")

        btn_clear = ctk.CTkButton(
            tb_frame,
            text="Clear Log",
            width=80,
            height=26,
            fg_color="#334155",
            hover_color="#475569",
            command=self._clear_log
        )
        btn_clear.pack(side="right")

        self.log_text = ctk.CTkTextbox(
            frame,
            fg_color="#090E17",
            text_color="#38BDF8",
            font=ctk.CTkFont(family="Consolas", size=12),
            corner_radius=8,
            border_width=1,
            border_color="#1E3A8A"
        )
        self.log_text.pack(fill="both", expand=True, padx=16, pady=(0, 12))

    def _telemetry_refresh_loop(self):
        """Pulls telemetry from SyncManager state and updates UI elements."""
        try:
            state = self.engine.sync_manager.get_full_state()
            scanner_a = state.get("scanner_a", {})
            scanner_b = state.get("scanner_b", {})
            audio_levels = state.get("audio_levels", {})

            # Update Scanner Display Cards
            rec_enabled = self.config.feeder.enable_feeder
            lvl_a = audio_levels.get("scanner_a", 0)
            lvl_b = audio_levels.get("scanner_b", 0)
            self.card_a.update_telemetry(scanner_a, lvl_a, rec_enabled)
            self.card_b.update_telemetry(scanner_b, lvl_b, rec_enabled)

            # Update Mutual Exclusion Status Banner
            sync_enabled = self.config.sync.enable_sync
            active_exclusions = state.get("active_exclusions", {})
            if not sync_enabled:
                self.sync_status_badge.configure(text="MUTUAL EXCLUSION DISABLED", fg_color="#334155", text_color="#94A3B8")
            elif active_exclusions:
                keys = ", ".join(active_exclusions.keys())
                self.sync_status_badge.configure(text=f"🔒 LOCKOUT ACTIVE: {keys}", fg_color="#7F1D1D", text_color="#FCA5A5")
            elif scanner_a.get("receiving") or scanner_b.get("receiving"):
                self.sync_status_badge.configure(text="● TRANSMISSION ACTIVE (MUTUAL SYNC ON)", fg_color="#065F46", text_color="#6EE7B7")
            else:
                self.sync_status_badge.configure(text="● SCANNING & READY (MUTUAL SYNC ON)", fg_color="#1E3A8A", text_color="#BAE6FD")

            # Update Event Log
            event_log = state.get("event_log", [])
            if len(event_log) > self.last_log_count:
                new_entries = event_log[self.last_log_count:]
                for ev in new_entries:
                    msg = ev.get("message", "")
                    self.log_text.insert("end", msg + "\n")
                self.log_text.see("end")
                self.last_log_count = len(event_log)

        except Exception as e:
            logger.error(f"Error in telemetry loop: {e}")

        self.after(100, self._telemetry_refresh_loop)

    def _on_sync_toggled(self):
        enabled = bool(self.switch_sync.get())
        self.config.sync.enable_sync = enabled
        self.engine.sync_manager.config.sync.enable_sync = enabled
        save_config(self.config)

    def _on_record_toggled(self):
        enabled = bool(self.switch_record.get())
        self.config.feeder.enable_feeder = enabled
        self.engine.sync_manager.set_recording_enabled(enabled)
        save_config(self.config)
        if hasattr(self, "switch_feeder_rec"):
            if enabled:
                self.switch_feeder_rec.select()
            else:
                self.switch_feeder_rec.deselect()

    def _on_feeder_rec_toggled(self):
        enabled = bool(self.switch_feeder_rec.get())
        if enabled:
            self.switch_record.select()
        else:
            self.switch_record.deselect()
        self._on_record_toggled()

    def _on_priority_changed(self, mode_str: str):
        self.config.sync.priority_mode = PriorityMode(mode_str)
        self.engine.sync_manager.config.sync.priority_mode = PriorityMode(mode_str)
        save_config(self.config)

    def _on_volume_slider_changed(self, val):
        pct = int(float(val))
        self.lbl_volume.configure(text=f"{pct}%")
        self.config.audio_loopback.volume = pct / 100.0
        save_config(self.config)
        if hasattr(self.engine.sync_manager, "audio_recorder") and hasattr(self.engine.sync_manager.audio_recorder, "loopback_monitor"):
            self.engine.sync_manager.audio_recorder.loopback_monitor.volume = pct / 100.0

    def _open_web_ui(self):
        import webbrowser
        webbrowser.open("http://127.0.0.1:8000")

    def _run_port_detection(self):
        """Asynchronously runs Windows COM port detection and auto-selects Uniden radios."""
        self.lbl_detect_result.configure(text="Scanning serial ports and querying Uniden radios...", text_color="#FCD34D")

        def _detect():
            import asyncio
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                active_ports = []
                if self.engine.sync_manager.scanner_a and self.engine.sync_manager.scanner_a.config.connection_mode.value == "SERIAL":
                    active_ports.append(self.engine.sync_manager.scanner_a.config.port)
                if self.engine.sync_manager.scanner_b and self.engine.sync_manager.scanner_b.config.connection_mode.value == "SERIAL":
                    active_ports.append(self.engine.sync_manager.scanner_b.config.port)

                detected = loop.run_until_complete(detect_serial_ports(active_ports))
                self.after(0, lambda: self._apply_detected_ports(detected))
            except Exception as e:
                self.after(0, lambda: self.lbl_detect_result.configure(text=f"Scan error: {e}", text_color="#EF4444"))
            finally:
                loop.close()

        threading.Thread(target=_detect, daemon=True).start()

    def _apply_detected_ports(self, detected):
        port_names = [p.port for p in detected] + ["NONE"]
        self.combo_port_a.configure(values=port_names)
        self.combo_port_b.configure(values=port_names)

        found_msgs = []
        for p in detected:
            label = f"{p.port} ({p.model or 'Available'})"
            found_msgs.append(label)
            # Auto-assign if Scanner A is currently unset or default
            if p.model and any(prefix in p.model for prefix in ["BCD", "SDS", "BCT", "BC"]):
                if self.combo_port_a.get() in ["COM3", "NONE"]:
                    self.combo_port_a.set(p.port)
                    self.combo_baud_a.set(str(p.baud_rate))
                    if p.model in POPULAR_SCANNER_MODELS:
                        self.combo_model_a.set(p.model)
                        self.card_a.combo_model.set(p.model)
                        self.config.scanner_a.model = ScannerModel(p.model)

        msg = "Detected: " + (", ".join(found_msgs) if found_msgs else "No serial devices responding")
        self.lbl_detect_result.configure(text=msg, text_color="#34D399")

    def _on_model_changed_from_card(self, scanner_id: str, new_model_str: str):
        try:
            new_model = ScannerModel(new_model_str)
            allowed_bauds = get_allowed_baud_rates(new_model)
            if scanner_id == "scanner_a":
                self.config.scanner_a.model = new_model
                if self.config.scanner_a.baud_rate not in allowed_bauds:
                    self.config.scanner_a.baud_rate = allowed_bauds[0]
                    if hasattr(self, "combo_baud_a"):
                        self.combo_baud_a.set(str(allowed_bauds[0]))
                if hasattr(self, "combo_baud_a"):
                    self.combo_baud_a.configure(values=[str(b) for b in allowed_bauds])
                if hasattr(self, "combo_model_a"):
                    self.combo_model_a.set(new_model.value)
                if hasattr(self, "card_a"):
                    self.card_a.combo_model.set(new_model.value)
            else:
                self.config.scanner_b.model = new_model
                if self.config.scanner_b.baud_rate not in allowed_bauds:
                    self.config.scanner_b.baud_rate = allowed_bauds[0]
                    if hasattr(self, "combo_baud_b"):
                        self.combo_baud_b.set(str(allowed_bauds[0]))
                if hasattr(self, "combo_baud_b"):
                    self.combo_baud_b.configure(values=[str(b) for b in allowed_bauds])
                if hasattr(self, "combo_model_b"):
                    self.combo_model_b.set(new_model.value)
                if hasattr(self, "card_b"):
                    self.card_b.combo_model.set(new_model.value)

            if scanner_id == "scanner_a" and hasattr(self, "lbl_card_a_title"):
                self.lbl_card_a_title.configure(text=f"📡 Scanner A ({new_model.value}) — Filename Template & TIT2 Title Tag")
            elif scanner_id == "scanner_b" and hasattr(self, "lbl_card_b_title"):
                self.lbl_card_b_title.configure(text=f"📡 Scanner B ({new_model.value}) — Filename Template & TIT2 Title Tag")
            self._update_filename_preview(scanner_id)
            self._update_tit2_preview(scanner_id)

            self.engine.update_config(self.config)
            save_config(self.config)
            msg = f"[{time.strftime('%H:%M:%S')}] {scanner_id.upper()} model switched to {new_model.value}"
            if hasattr(self, "log_text"):
                self.log_text.insert("end", msg + "\n")
                self.log_text.see("end")
        except Exception as e:
            logger.error(f"Error changing model for {scanner_id}: {e}")

    def _apply_hardware_settings(self):
        self.config.scanner_a.model = ScannerModel(self.combo_model_a.get())
        self.config.scanner_a.port = self.combo_port_a.get()
        self.config.scanner_a.baud_rate = int(self.combo_baud_a.get())
        self.config.scanner_a.enabled = (self.config.scanner_a.port != "NONE")

        self.config.scanner_b.model = ScannerModel(self.combo_model_b.get())
        self.config.scanner_b.port = self.combo_port_b.get()
        self.config.scanner_b.baud_rate = int(self.combo_baud_b.get())
        self.config.scanner_b.enabled = (self.config.scanner_b.port != "NONE")

        self.card_a.combo_model.set(self.config.scanner_a.model.value)
        self.card_b.combo_model.set(self.config.scanner_b.model.value)

        # Audio Settings for Scanner A
        sel_audio_a = self.combo_audio_a.get()
        if sel_audio_a == "Disabled / Mute" or not self.config.scanner_a.enabled:
            self.config.scanner_a.audio_enabled = False
            self.config.scanner_a.audio_device_index = None
        else:
            self.config.scanner_a.audio_enabled = True
            self.config.scanner_a.audio_device_index = self.audio_devices_map.get(sel_audio_a)

        ch_a = self.combo_chan_a.get().split()[0]
        self.config.scanner_a.audio_channel = AudioChannelMode(ch_a)

        # Audio Settings for Scanner B
        sel_audio_b = self.combo_audio_b.get()
        if sel_audio_b == "Disabled / Mute" or not self.config.scanner_b.enabled:
            self.config.scanner_b.audio_enabled = False
            self.config.scanner_b.audio_device_index = None
        else:
            self.config.scanner_b.audio_enabled = True
            self.config.scanner_b.audio_device_index = self.audio_devices_map.get(sel_audio_b)

        ch_b = self.combo_chan_b.get().split()[0]
        self.config.scanner_b.audio_channel = AudioChannelMode(ch_b)

        self.config.feeder.audio_device_index = self.config.scanner_a.audio_device_index

        # Playback Loopback Monitor Settings
        sel_out = self.combo_audio_out.get()
        if sel_out == "Disabled / No Monitoring":
            self.config.audio_loopback.enabled = False
            self.config.audio_loopback.output_device_index = None
        else:
            self.config.audio_loopback.enabled = bool(self.switch_loopback_enable.get())
            self.config.audio_loopback.output_device_index = self.audio_output_devices_map.get(sel_out)

        self.config.audio_loopback.volume = float(self.slider_volume.get()) / 100.0
        self.config.audio_loopback.pan_mode = "STEREO_SPLIT" if "STEREO" in self.combo_pan_mode.get() else "CENTER"

        self.engine.update_config(self.config)
        save_config(self.config)

        out_name = sel_out if self.config.audio_loopback.enabled else "Disabled"
        messagebox.showinfo(
            "Hardware & Audio Applied",
            f"Configuration Applied:\n"
            f"• Scanner A: {self.config.scanner_a.model.value} on {self.config.scanner_a.port} | Audio: {sel_audio_a} ({ch_a})\n"
            f"• Scanner B: {self.config.scanner_b.model.value} on {self.config.scanner_b.port} | Audio: {sel_audio_b} ({ch_b})\n"
            f"• Playback Output: {out_name} (Vol: {int(self.config.audio_loopback.volume * 100)}%, {self.config.audio_loopback.pan_mode})"
        )

    def _browse_inbox_dir(self):
        dir_path = filedialog.askdirectory(initialdir=self.entry_inbox.get(), title="Select ScanScribe Audio Inbox Directory")
        if dir_path:
            self.entry_inbox.delete(0, "end")
            self.entry_inbox.insert(0, dir_path)

    def _set_filename_preset(self, scanner_id: str, preset: str):
        if scanner_id == "scanner_a" and hasattr(self, "entry_filename_template_a"):
            self.entry_filename_template_a.delete(0, "end")
            self.entry_filename_template_a.insert(0, preset)
            self._update_filename_preview("scanner_a")
        elif scanner_id == "scanner_b" and hasattr(self, "entry_filename_template_b"):
            self.entry_filename_template_b.delete(0, "end")
            self.entry_filename_template_b.insert(0, preset)
            self._update_filename_preview("scanner_b")

    def _update_filename_preview(self, scanner_id: Optional[str] = None, event=None):
        import datetime
        from metadata.proscan_metadata import format_template, sanitize_filename, ProScanMetadata
        ext = self.config.feeder.audio_format.lower()
        now = datetime.datetime.now()

        # Update Scanner A preview
        if scanner_id in ("scanner_a", None) and hasattr(self, "lbl_filename_preview_a"):
            tmpl_a = self.entry_filename_template_a.get().strip() if hasattr(self, "entry_filename_template_a") else ""
            if not tmpl_a:
                tmpl_a = getattr(self.config.scanner_a, "filename_template", None) or "%DT - %S - %C"
            model_a = getattr(self.config.scanner_a.model, "value", str(self.config.scanner_a.model))
            sample_meta_a = ProScanMetadata(
                scanner=model_a,
                system_name="Metropolitan P25 Trunk",
                department_name="Fire & Rescue",
                channel_name="Dispatch North",
                frequency="851.2500",
                tgid="10401",
                tone="156.7",
                modulation="NFM",
                rssi="5"
            )
            formatted_a = format_template(tmpl_a, sample_meta_a, now)
            safe_stem_a = sanitize_filename(formatted_a)
            self.lbl_filename_preview_a.configure(text=f"Generated Preview: {safe_stem_a}.{ext}", text_color="#38BDF8")

        # Update Scanner B preview
        if scanner_id in ("scanner_b", None) and hasattr(self, "lbl_filename_preview_b"):
            tmpl_b = self.entry_filename_template_b.get().strip() if hasattr(self, "entry_filename_template_b") else ""
            if not tmpl_b:
                tmpl_b = getattr(self.config.scanner_b, "filename_template", None) or "%DT - %S - %C (%TG)"
            model_b = getattr(self.config.scanner_b.model, "value", str(self.config.scanner_b.model))
            sample_meta_b = ProScanMetadata(
                scanner=model_b,
                system_name="Statewide Highway P25",
                department_name="Highway Patrol",
                channel_name="Troop C Main",
                frequency="773.80625",
                tgid="20104",
                tone="",
                modulation="NFM",
                rssi="4"
            )
            formatted_b = format_template(tmpl_b, sample_meta_b, now)
            safe_stem_b = sanitize_filename(formatted_b)
            self.lbl_filename_preview_b.configure(text=f"Generated Preview: {safe_stem_b}.{ext}", text_color="#C084FC")

    def _set_tit2_preset(self, scanner_id: str, preset: str):
        if scanner_id == "scanner_a" and hasattr(self, "entry_tit2_template_a"):
            self.entry_tit2_template_a.delete(0, "end")
            self.entry_tit2_template_a.insert(0, preset)
            self._update_tit2_preview("scanner_a")
        elif scanner_id == "scanner_b" and hasattr(self, "entry_tit2_template_b"):
            self.entry_tit2_template_b.delete(0, "end")
            self.entry_tit2_template_b.insert(0, preset)
            self._update_tit2_preview("scanner_b")

    def _update_tit2_preview(self, scanner_id: Optional[str] = None, event=None):
        import datetime
        from metadata.proscan_metadata import format_template, ProScanMetadata
        now = datetime.datetime.now()

        # Update Scanner A TIT2 preview
        if scanner_id in ("scanner_a", None) and hasattr(self, "lbl_tit2_preview_a"):
            tmpl_a = self.entry_tit2_template_a.get().strip() if hasattr(self, "entry_tit2_template_a") else ""
            if not tmpl_a:
                tmpl_a = getattr(self.config.scanner_a, "tit2_template", None) or "%C (%TG)"
            model_a = getattr(self.config.scanner_a.model, "value", str(self.config.scanner_a.model))
            sample_meta_a = ProScanMetadata(
                scanner=model_a,
                system_name="Metropolitan P25 Trunk",
                department_name="Fire & Rescue",
                channel_name="Dispatch North",
                frequency="851.2500",
                tgid="10401",
                tone="156.7",
                modulation="NFM",
                rssi="5"
            )
            formatted_a = format_template(tmpl_a, sample_meta_a, now).strip()[:253]
            self.lbl_tit2_preview_a.configure(text=f"Generated TIT2 Title Tag: \"{formatted_a}\"", text_color="#A78BFA")

        # Update Scanner B TIT2 preview
        if scanner_id in ("scanner_b", None) and hasattr(self, "lbl_tit2_preview_b"):
            tmpl_b = self.entry_tit2_template_b.get().strip() if hasattr(self, "entry_tit2_template_b") else ""
            if not tmpl_b:
                tmpl_b = getattr(self.config.scanner_b, "tit2_template", None) or "%C (%TG)"
            model_b = getattr(self.config.scanner_b.model, "value", str(self.config.scanner_b.model))
            sample_meta_b = ProScanMetadata(
                scanner=model_b,
                system_name="Statewide Highway P25",
                department_name="Highway Patrol",
                channel_name="Troop C Main",
                frequency="773.80625",
                tgid="20104",
                tone="",
                modulation="NFM",
                rssi="4"
            )
            formatted_b = format_template(tmpl_b, sample_meta_b, now).strip()[:253]
            self.lbl_tit2_preview_b.configure(text=f"Generated TIT2 Title Tag: \"{formatted_b}\"", text_color="#E9D5FF")

    def _save_feeder_config(self):
        self.config.feeder.feeder_mode = FeederMode(self.combo_feeder_mode.get())
        inbox_val = self.entry_inbox.get().strip("\"' \t\r\n")
        self.config.feeder.inbox_directory = inbox_val

        tmpl_a = (self.entry_filename_template_a.get().strip() if hasattr(self, "entry_filename_template_a") else "") or "%DT - %S - %C"
        tit2_a = (self.entry_tit2_template_a.get().strip() if hasattr(self, "entry_tit2_template_a") else "") or "%C (%TG)"
        self.config.scanner_a.filename_template = tmpl_a
        self.config.scanner_a.tit2_template = tit2_a

        tmpl_b = (self.entry_filename_template_b.get().strip() if hasattr(self, "entry_filename_template_b") else "") or "%DT - %S - %C (%TG)"
        tit2_b = (self.entry_tit2_template_b.get().strip() if hasattr(self, "entry_tit2_template_b") else "") or "%C (%TG)"
        self.config.scanner_b.filename_template = tmpl_b
        self.config.scanner_b.tit2_template = tit2_b

        # Global fallbacks on feeder
        self.config.feeder.filename_template = tmpl_a
        self.config.feeder.tit2_template = tit2_a

        self.config.feeder.webhook_url = self.entry_webhook.get().strip()
        if hasattr(self, "switch_sidecar_json"):
            self.config.feeder.enable_sidecar_json = bool(self.switch_sidecar_json.get())

        self.engine.sync_manager.feeder.update_config(self.config.feeder)
        self.engine.sync_manager.feeder.update_scanner_configs({"scanner_a": self.config.scanner_a, "scanner_b": self.config.scanner_b})
        save_config(self.config)
        resolved_inbox = self.engine.sync_manager.feeder._resolve_inbox_directory()
        messagebox.showinfo(
            "Feeder & Scanner Templates Saved",
            f"ScanScribe Feeder settings updated successfully.\n\n"
            f"Scanner A ({self.config.scanner_a.model.value}):\n"
            f"  • Filename: {tmpl_a}\n"
            f"  • TIT2 Tag: {tit2_a}\n\n"
            f"Scanner B ({self.config.scanner_b.model.value}):\n"
            f"  • Filename: {tmpl_b}\n"
            f"  • TIT2 Tag: {tit2_b}\n\n"
            f"Active Ingestion Directory:\n{resolved_inbox}"
        )

    def _dispatch_test_feeder(self, scanner_id: str = "scanner_a"):
        sc_cfg = self.config.scanner_a if scanner_id == "scanner_a" else self.config.scanner_b
        payload = CallTransmissionPayload(
            scanner_id=scanner_id,
            scanner_model=sc_cfg.model.value,
            system_name="Metropolitan P25 Trunk" if scanner_id == "scanner_a" else "Statewide Highway P25",
            department_name="Fire & Rescue" if scanner_id == "scanner_a" else "State Police Dispatch",
            channel_name="Dispatch North" if scanner_id == "scanner_a" else "Troop C Main",
            tgid="10401" if scanner_id == "scanner_a" else "20104",
            frequency=851.2500 if scanner_id == "scanner_a" else 773.80625,
            rssi=5,
            duration_seconds=4.2,
            timestamp=time.time(),
            filename_template=getattr(sc_cfg, "filename_template", None),
            tit2_template=getattr(sc_cfg, "tit2_template", None)
        )
        res = self.engine.sync_manager.feeder.dispatch_call(payload)
        status_val = res.get('status', 'unknown')
        self.log_text.insert("end", f"[{time.strftime('%H:%M:%S')}] TEST DISPATCH [{scanner_id.upper()}]: mode={res.get('mode')} status={status_val}\n")
        self.log_text.see("end")
        file_or_url = res.get('audio_file') or res.get('response') or ''
        messagebox.showinfo("Test Dispatch", f"Feeder Test Result for {scanner_id.upper()}:\nStatus: {status_val}\nMode: {res.get('mode')}\nFile/URL: {file_or_url}")

    def _clear_log(self):
        self.log_text.delete("1.0", "end")
        self.engine.sync_manager.event_log.clear()
        self.last_log_count = 0

    def _on_close(self):
        save_config(self.config)
        self.engine.stop()
        self.destroy()


def run_gui():
    import multiprocessing
    multiprocessing.freeze_support()
    print("=" * 70)
    print("  UNIDEN SCANNER MUTUAL EXCLUSION CONTROLLER — DEBUG CONSOLE")
    print("=" * 70)
    print("  Live serial telemetry, mutual exclusion, and audio debug output.")
    print("  Close this console or the GUI window to exit.")
    print("-" * 70)
    app = ScannerControllerApp()
    app.mainloop()


if __name__ == "__main__":
    run_gui()
