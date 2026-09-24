# -*- mode: python ; coding: utf-8 -*-
import sys
import os
from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs, collect_submodules

block_cipher = None

# Collect sounddevice portaudio DLLs and data
sounddevice_datas = collect_data_files('_sounddevice_data')
sounddevice_binaries = collect_dynamic_libs('_sounddevice_data')

# Static files
datas = [
    ('static', 'static'),
] + sounddevice_datas

binaries = sounddevice_binaries

hiddenimports = [
    # Uvicorn internals
    'uvicorn',
    'uvicorn.logging',
    'uvicorn.loops',
    'uvicorn.loops.auto',
    'uvicorn.loops.asyncio',
    'uvicorn.protocols',
    'uvicorn.protocols.http',
    'uvicorn.protocols.http.auto',
    'uvicorn.protocols.http.h11_impl',
    'uvicorn.protocols.websockets',
    'uvicorn.protocols.websockets.auto',
    'uvicorn.protocols.websockets.websockets_impl',
    'uvicorn.lifespan',
    'uvicorn.lifespan.on',
    'uvicorn.lifespan.off',

    # FastAPI & Starlette
    'fastapi',
    'fastapi.staticfiles',
    'starlette',
    'starlette.staticfiles',
    'starlette.responses',
    'starlette.websockets',

    # Serial and Hardware Detection
    'serial',
    'serial.tools',
    'serial.tools.list_ports',
    'serial.tools.list_ports_windows',

    # Audio & Numerics
    'sounddevice',
    '_sounddevice_data',
    'numpy',

    # App internal modules
    'config',
    'server',
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
    ['server.py'],
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
    name='ScannerController',
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
