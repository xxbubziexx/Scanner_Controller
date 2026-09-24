# -*- mode: python ; coding: utf-8 -*-
import sys
import os
from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs

block_cipher = None

# Collect customtkinter themes and fonts
customtkinter_datas = collect_data_files('customtkinter')

# Collect sounddevice portaudio DLLs and data
sounddevice_datas = collect_data_files('_sounddevice_data')
sounddevice_binaries = collect_dynamic_libs('_sounddevice_data')

datas = customtkinter_datas + sounddevice_datas
binaries = sounddevice_binaries

hiddenimports = [
    # CustomTkinter & Desktop UI
    'customtkinter',
    'darkdetect',
    'tkinter',
    'tkinter.filedialog',
    'tkinter.messagebox',

    # Serial and Hardware Detection
    'serial',
    'serial.tools',
    'serial.tools.list_ports',
    'serial.tools.list_ports_windows',

    # Audio & Numerics
    'sounddevice',
    '_sounddevice_data',
    'numpy',

    # Internal Engine and Modules
    'config',
    'engine.sync_manager',
    'drivers.base_driver',
    'drivers.bcd436hp',
    'drivers.bcd996p2',
    'drivers.classic_driver',
    'drivers.driver_factory',
    'discovery.serial_detector',
    'discovery.net_detector',
    'feeder.scanscribe_feeder',
    'metadata.proscan_metadata',
    'audio.recorder',
]

a = Analysis(
    ['gui.py'],
    pathex=['.'],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name='ScannerControllerGUI',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
