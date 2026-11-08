#!/usr/bin/env python3
"""
tools/gen_fixtures.py
Generates byte-exact golden test fixtures from the Python reference implementation
into rust/fixtures/ to freeze baseline behavior for the Rust migration.
"""

import os
import sys
import json
import shutil
import datetime
import dataclasses
import struct
import numpy as np

# Ensure repository root is in sys.path
REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, REPO_ROOT)

from metadata.proscan_metadata import (
    ProScanMetadata,
    build_pros_chunk,
    write_mp3_id3v23,
    write_wav_riff_info,
    format_template,
    format_streaming_metadata,
    sanitize_filename,
    sanitize_folder_path,
    clean_ascii_1
)
from engine.sync_manager import resample_pcm_int16

FIXTURES_DIR = os.path.join(REPO_ROOT, "rust", "fixtures")

def generate_metadata_fixtures():
    meta_dir = os.path.join(FIXTURES_DIR, "metadata")
    os.makedirs(meta_dir, exist_ok=True)
    fixed_dt = datetime.datetime(2026, 9, 23, 15, 30, 0)
    dummy_mp3 = b"\xff\xfb\x90\x64\x00\x00\x00\x00" * 32  # minimal valid-ish MP3 frame sequence
    dummy_pcm = (np.sin(np.linspace(0, 100 * np.pi, 16000)) * 10000).astype(np.int16).tobytes()

    matrix = {
        "typical_conventional": ProScanMetadata(
            scanner="BCD996P2",
            ending_date="20260924003530",
            favorite_name="Public Safety Favorites",
            system_name="St. Francois County",
            site_name="North Tower",
            department_name="Sheriff Dispatch",
            channel_name="Law 1 Dispatch",
            frequency="155.7975",
            modulation="NFM",
            tone="DCS023",
            tgid="",
            uid="",
            uid_number="",
            ftoa="155.7975",
            ftob="155.7975",
            rssi="4",
            alert_color="Blue",
            service_type="Law Dispatch",
            service_color="Red",
            displayed_system_type="Conventional",
            digital_status="Analog",
            dmr_slot="",
            scanner_mode="Hold",
            info_sc_state="Hold",
            comm_port="COM5"
        ),
        "typical_trunk": ProScanMetadata(
            scanner="BCD436HP",
            ending_date="20260924011500",
            favorite_name="Statewide P25",
            system_name="MOSWIN",
            site_name="St. Francois Site",
            department_name="Fire Dispatch",
            channel_name="County Fire Main",
            frequency="853.5000",
            modulation="NFM",
            tone="",
            tgid="10401",
            uid="704101",
            uid_number="101",
            ftoa="",
            ftob="",
            rssi="5",
            alert_color="Red",
            service_type="Fire Dispatch",
            service_color="Red",
            displayed_system_type="P25 Standard",
            digital_status="P25 Phase 1",
            dmr_slot="1",
            scanner_mode="Scan",
            info_sc_state="Scan",
            comm_port="COM6"
        ),
        "empty_all": ProScanMetadata(
            scanner="",
            ending_date="",
            favorite_name="",
            system_name="",
            site_name="",
            department_name="",
            channel_name="",
            frequency="",
            modulation="",
            tone="",
            tgid="",
            uid="",
            uid_number="",
            ftoa="",
            ftob="",
            rssi="",
            alert_color="",
            service_type="",
            service_color="",
            displayed_system_type="",
            digital_status="",
            dmr_slot="",
            scanner_mode=""
        ),
        "max_len_253": ProScanMetadata(
            scanner="A" * 253,
            favorite_name="F" * 253,
            system_name="S" * 253,
            site_name="T" * 253,
            department_name="D" * 253,
            channel_name="C" * 253,
            frequency="123.4567",
            modulation="WFM",
            tone="T" * 24,
            tgid="9" * 16,
            uid="U" * 64,
            uid_number="N" * 16,
            service_type="V" * 253
        ),
        "non_ascii": ProScanMetadata(
            scanner="BCD996P2",
            favorite_name="Café & naïve—favorites…",
            system_name="Metro «Trunk» ©2026",
            department_name="Füßgänger & Rescue",
            channel_name="Main—Channel™",
            frequency="851.2500",
            tone="DCS023"
        ),
        "odd_payload_pad": ProScanMetadata(
            scanner="A",  # 1 char to manipulate payload parity
            favorite_name="B",
            system_name="C",
            department_name="D",
            channel_name="E",
            frequency="155.0",
            modulation="NFM"
        ),
        "even_payload_nopad": ProScanMetadata(
            scanner="AA",
            favorite_name="BB",
            system_name="CC",
            department_name="DD",
            channel_name="EE",
            frequency="155.00",
            modulation="NFM"
        )
    }

    for name, meta in matrix.items():
        # 1. pros chunk
        pros = build_pros_chunk(meta)
        with open(os.path.join(meta_dir, f"{name}_pros.bin"), "wb") as f:
            f.write(pros)

        # 2. mp3 id3v23 tag
        mp3_out = write_mp3_id3v23(dummy_mp3, meta, title_template="%C", artist_template="%S", band_template="%G", now=fixed_dt)
        with open(os.path.join(meta_dir, f"{name}_id3.bin"), "wb") as f:
            f.write(mp3_out)

        # 3. wav riff info chunk
        wav_out = write_wav_riff_info(dummy_pcm, meta, title_template="%C", artist_template="%S", sample_rate=16000, now=fixed_dt)
        with open(os.path.join(meta_dir, f"{name}_riff.bin"), "wb") as f:
            f.write(wav_out)

    # Edge Case: All templates empty in MP3 -> No ID3 tag written at all
    empty_templates_mp3 = write_mp3_id3v23(dummy_mp3, matrix["typical_conventional"], title_template="", artist_template="", band_template="", now=fixed_dt)
    with open(os.path.join(meta_dir, "empty_templates_id3.bin"), "wb") as f:
        f.write(empty_templates_mp3)

    # Edge Case: All templates empty in WAV -> Full RIFF and pros chunk written unconditionally
    empty_templates_wav = write_wav_riff_info(dummy_pcm, matrix["typical_conventional"], title_template="", artist_template="", sample_rate=16000, now=fixed_dt)
    with open(os.path.join(meta_dir, "empty_templates_riff.bin"), "wb") as f:
        f.write(empty_templates_wav)

    cases_dict = {name: dataclasses.asdict(meta) for name, meta in matrix.items()}
    with open(os.path.join(meta_dir, "metadata_cases.json"), "w", encoding="utf-8") as f:
        json.dump(cases_dict, f, indent=2)

    print(f"[OK] Generated {len(matrix) * 3 + 2} metadata binary fixtures and metadata_cases.json in {meta_dir}")


def generate_formatter_fixtures():
    fixed_dt = datetime.datetime(2026, 9, 23, 15, 30, 0)
    meta = ProScanMetadata(
        scanner="BCD996P2",
        ending_date="20260924003530",
        favorite_name="Public Safety Favorites",
        system_name="Metropolitan P25 Trunk",
        site_name="Metro Tower 1",
        department_name="Fire & Rescue",
        channel_name="Dispatch North",
        frequency="851.2500",
        modulation="NFM",
        tone="DCS023",
        tgid="10401",
        uid="704101",
        uid_number="101",
        ftoa="851.2500",
        ftob="851.2500",
        rssi="4",
        alert_color="Blue",
        service_type="Fire Dispatch",
        service_color="Red",
        displayed_system_type="P25 Standard",
        digital_status="P25 Phase 1",
        dmr_slot="1",
        scanner_mode="Hold",
        info_sc_state="Hold",
        comm_port="COM5",
        program_path="C:\\ProScan\\ProScan.exe",
        program_filename="ProScan.exe"
    )

    templates = [
        # Canonical 37 Engine A specifiers
        ("%FT", "Frequency / TGID hybrid"),
        ("%TG", "TalkGroup"),
        ("%L2", "System Name line 2"),
        ("%L1", "Channel Name line 1"),
        ("%ST1", "Service Type"),
        ("%STC", "Service Color"),
        ("%AC", "Alert Color"),
        ("%SC", "Scanner State"),
        ("%ST", "Scanner Model Name"),
        ("%DD", "Day 2-digit"),
        ("%DT", "Date & Time formatted"),
        ("%TT", "Time formatted"),
        ("%H", "Hour 24-hr"),
        ("%MM", "Month Name 3-char"),
        ("%YY", "Year 4-digit"),
        ("%AF1", "Program Path"),
        ("%AF2", "Program Filename"),
        ("%FA", "Favorite Name"),
        ("%SI", "Site Name"),
        ("%S", "System Name"),
        ("%G", "Department / Group Name"),
        ("%B", "Department / Group Name (alias B)"),
        ("%DE", "Department Name (alias DE)"),
        ("%D", "Full Date"),
        ("%M", "Month 2-digit"),
        ("%Y", "Year 2-digit"),
        ("%C", "Channel Name"),
        ("%F", "Frequency with DMR slot"),
        ("%T", "Tone"),
        ("%MD", "Modulation"),
        ("%R", "RSSI"),
        ("%s", "Space character"),
        ("%U1", "UID"),
        ("%U2", "UID Number"),
        ("%U3", "UID Number (alias U3)"),
        ("%TA", "FTOA"),
        ("%TB", "FTOB"),
        ("%P", "Comm Port"),
        # The Three Critical Counterintuitive Inversions
        ("%MM-%M-%YY-%Y", "Three Inversions 1"),
        ("%D %DD", "Three Inversions 2"),
        # Combinations & Filename Schemes
        ("%YY%M%DD_%H%TT_%C_%F", "Standard recording template"),
        ("[%S] [%G] - %C (%TG) - %F %MD (%T)", "Detailed title template"),
        ("%S/%G/%C", "Subfolder path template"),
        # Streaming metadata (Engine B)
        ("%C - %S - %G", "Streaming standard"),
        ("%CL Listeners: %C on %F", "Streaming with listener count")
    ]

    cases = []
    for tmpl, desc in templates:
        exp_engine_a = format_template(tmpl, meta, now=fixed_dt)
        exp_engine_b = format_streaming_metadata(tmpl, meta, listener_count=42)
        cases.append({
            "template": tmpl,
            "description": desc,
            "engine_a_expected": exp_engine_a,
            "engine_b_expected": exp_engine_b
        })

    # Filename & Path Sanitization Cases
    sanitize_cases = [
        ("Fire / Rescue: Tac 1 *Main*? <North>", "Fire  Rescue Tac 1 Main North"),
        ("COM1: Special \"Aux\" | Output", "COM1 Special Aux  Output"),
        ("Normal_Channel_Name-155.7975", "Normal_Channel_Name-155.7975"),
        ("Folder/Sub:Folder\\Path", "Folder/Sub:Folder\\Path")
    ]

    sanitizer_data = []
    for raw, _ in sanitize_cases:
        sanitizer_data.append({
            "raw": raw,
            "clean_ascii": clean_ascii_1(raw, 253),
            "sanitized_filename": sanitize_filename(raw),
            "sanitized_folder": sanitize_folder_path(raw)
        })

    output = {
        "metadata_snapshot": dataclasses.asdict(meta),
        "fixed_now": fixed_dt.isoformat(),
        "template_cases": cases,
        "sanitizer_cases": sanitizer_data
    }

    out_file = os.path.join(FIXTURES_DIR, "formatters.json")
    with open(out_file, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2)

    print(f"[OK] Generated formatters matrix with {len(cases)} templates in {out_file}")


def generate_driver_fixtures():
    import asyncio
    from unittest.mock import AsyncMock, patch
    from config import ScannerConfig, ScannerModel
    from drivers.bcd996p2 import BCD996P2Driver
    from drivers.bcd436hp import BCD436HPDriver

    drv_dir = os.path.join(FIXTURES_DIR, "drivers")
    os.makedirs(drv_dir, exist_ok=True)

    raw_cases = {
        "dma_conventional": (
            "scanner_b",
            ScannerModel.BCD996P2,
            BCD996P2Driver,
            (
                "MDL,BCD996P2\r"
                "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
                "GLG,155.7975,NFM,,0,St. Francois County,Sheriff Dispatch,Law 1 Dispatch,1,,,,,0\r"
                "VOL,12\r"
                "SQL,2\r"
                "PWR,4,15579750\r"
            ),
        ),
        "dma_trunk": (
            "scanner_b",
            ScannerModel.BCD996P2,
            BCD996P2Driver,
            (
                "MDL,BCD996P2\r"
                "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
                "GLG,ID:10401,NFM,,0,MOSWIN,St. Francois County,County Fire Main,1,,,,,0\r"
                "VOL,12\r"
                "SQL,2\r"
                "PWR,5,85350000\r"
            ),
        ),
        "dma_scanning": (
            "scanner_b",
            ScannerModel.BCD996P2,
            BCD996P2Driver,
            (
                "MDL,BCD996P2\r"
                "STS,00000000000000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
                "GLG,,,,0,Scanning...,,,0,,,,,0\r"
                "VOL,12\r"
                "SQL,2\r"
                "PWR,0,0\r"
            ),
        ),
        "hp_pipelined": (
            "scanner_a",
            ScannerModel.BCD436HP,
            BCD436HPDriver,
            (
                "MDL,BCD436HP\r"
                "STS,00110110110000, F0:---------9 17:29    ,, S0:--2------- Sep23    ,\r"
                "GLG,155.7975,NFM,,0,St. Francois County,Sheriff,Law 1,0,1,0,0,0,0\r"
                "VOL,10\r"
                "SQL,2\r"
                "PWR,4,155.7975\r"
                "GSI,<XML>,<ScannerInfo>"
                "<Department Name=\"Sheriff Dispatch\"/>"
                "<ConvFrequency Name=\"Law 1 Dispatch\" Freq=\"155.7975\" TGID=\"\"/>"
                "<Property P25Status=\"None\" Mute=\"Unmute\" RecSlot=\"\" U_Id=\"\" Rssi=\"4\"/>"
                "</ScannerInfo>\r"
            ),
        ),
        "hp_encrypted": (
            "scanner_a",
            ScannerModel.SDS100,
            BCD436HPDriver,
            (
                "MDL,SDS100\r"
                "GLG,853.5000,NFM,,0,MOSWIN,State Highway Patrol,Troop C Tac,0,0,0,0,0,0\r"
                "PWR,5,853.5000\r"
                "GSI,<XML>,<ScannerInfo>"
                "<Department Name=\"Troop C Tactical\"/>"
                "<ConvFrequency Name=\"Troop C Enc Tac\" Freq=\"853.5000\" TGID=\"51022\"/>"
                "<Property P25Status=\"Enc\" Mute=\"Mute\" RecSlot=\"/1\" U_Id=\"720101\" Rssi=\"5\"/>"
                "</ScannerInfo>\r"
            ),
        ),
    }

    for name, (scanner_id, model_enum, driver_cls, raw_resp) in raw_cases.items():
        cfg = ScannerConfig(id=scanner_id, name=name, model=model_enum, port="COM5")
        drv = driver_cls(cfg)
        drv.status.connected = True
        with patch.object(drv, "_send_command", new_callable=AsyncMock) as mock_cmd:
            mock_cmd.return_value = raw_resp
            status = asyncio.run(drv.poll())

        expected_dict = {
            "scanner_id": status.scanner_id,
            "model": status.model,
            "connected": status.connected,
            "receiving": status.receiving,
            "system_name": status.system_name,
            "dept_name": status.dept_name,
            "channel_name": status.channel_name,
            "frequency": status.frequency,
            "tgid": status.tgid,
            "modulation": status.modulation,
            "rssi": status.rssi,
            "status_line": status.status_line,
        }

        with open(os.path.join(drv_dir, f"{name}.txt"), "w", encoding="utf-8", newline="") as f:
            f.write(raw_resp)
        with open(os.path.join(drv_dir, f"{name}_expected.json"), "w", encoding="utf-8") as f:
            json.dump(expected_dict, f, indent=2)

    print(f"[OK] Generated {len(raw_cases)} driver transcripts in {drv_dir}")


def generate_audio_fixtures():
    aud_dir = os.path.join(FIXTURES_DIR, "audio")
    os.makedirs(aud_dir, exist_ok=True)

    # 1. Resampling fixtures (PCM in -> resampled PCM out)
    res_dir = os.path.join(aud_dir, "resample")
    os.makedirs(res_dir, exist_ok=True)

    rates = [(44100, 16000), (16000, 8000), (8000, 16000), (22050, 16000), (44100, 22050)]
    for in_rate, out_rate in rates:
        t = np.linspace(0, 0.5, int(in_rate * 0.5), endpoint=False)
        # Sine wave tone burst at 440 Hz
        pcm_in = (np.sin(2 * np.pi * 440 * t) * 15000).astype(np.int16).tobytes()
        pcm_out = resample_pcm_int16(pcm_in, in_rate, out_rate)

        with open(os.path.join(res_dir, f"in_{in_rate}_to_{out_rate}.pcm"), "wb") as f:
            f.write(pcm_in)
        with open(os.path.join(res_dir, f"out_{in_rate}_to_{out_rate}.pcm"), "wb") as f:
            f.write(pcm_out)

    # 2. VOX test audio sequence
    # 0.5s silence, 1.0s active voice (800 Hz tone), 0.5s silence, 0.5s background noise
    sr = 16000
    silence = np.zeros(int(sr * 0.5), dtype=np.int16)
    t_voice = np.linspace(0, 1.0, int(sr * 1.0), endpoint=False)
    voice = (np.sin(2 * np.pi * 800 * t_voice) * 12000).astype(np.int16)
    noise = (np.random.normal(0, 500, int(sr * 0.5))).astype(np.int16)
    full_pcm = np.concatenate([silence, voice, silence, noise])

    with open(os.path.join(aud_dir, "vox_test_16k.pcm"), "wb") as f:
        f.write(full_pcm.tobytes())

    vox_expected = {
        "sample_rate": sr,
        "total_samples": len(full_pcm),
        "voice_start_sample": int(sr * 0.5),   # 8000
        "voice_end_sample": int(sr * 1.5),     # 24000
        "expected_gate_open_sample": 8000,
        "hang_time_ms": 300,
        "expected_gate_close_sample": 24000 + int(sr * 0.3)  # 28800
    }
    with open(os.path.join(aud_dir, "vox_expected.json"), "w", encoding="utf-8") as f:
        json.dump(vox_expected, f, indent=2)

    print(f"[OK] Generated audio & VOX fixtures in {aud_dir}")


def generate_feeder_fixtures():
    fdr_dir = os.path.join(FIXTURES_DIR, "feeder")
    os.makedirs(fdr_dir, exist_ok=True)

    sidecar_sample = {
        "timestamp": 1790184600.0,
        "datetime": "2026-09-23 15:30:00",
        "scanner_id": "scanner_a",
        "scanner_model": "BCD436HP",
        "system_name": "Metropolitan P25 Trunk",
        "department_name": "Fire & Rescue",
        "channel_name": "Dispatch North",
        "tgid": "10401",
        "talkgroup": "10401",
        "talkgroup_id": "10401",
        "talkgroup_num": 10401,
        "talkgroup_tag": "Dispatch North",
        "talkgroup_name": "Dispatch North",
        "talkgroup_group": "Fire & Rescue",
        "group_name": "Fire & Rescue",
        "frequency": 851.25,
        "frequency_mhz": "851.2500",
        "modulation": "NFM",
        "rssi": 4,
        "duration": 4.5,
        "duration_seconds": 4.5,
        "audio_filename": "20260923_153000_Dispatch_North.mp3"
    }

    with open(os.path.join(fdr_dir, "golden_sidecar.json"), "w", encoding="utf-8") as f:
        json.dump(sidecar_sample, f, indent=2)

    payload_sample = {
        "scanner_id": "scanner_a",
        "scanner_model": "BCD436HP",
        "system_name": "Metropolitan P25 Trunk",
        "department_name": "Fire & Rescue",
        "channel_name": "Dispatch North",
        "tgid": "10401",
        "frequency": 851.25,
        "modulation": "NFM",
        "rssi": 4,
        "timestamp": 1790184600.0,
        "duration_seconds": 4.5
    }

    with open(os.path.join(fdr_dir, "golden_webhook_payload.json"), "w", encoding="utf-8") as f:
        json.dump(payload_sample, f, indent=2)

    print(f"[OK] Generated feeder golden fixtures in {fdr_dir}")


def copy_proscan_samples():
    samples_dir = os.path.join(FIXTURES_DIR, "proscan_samples")
    os.makedirs(samples_dir, exist_ok=True)

    sample_filenames = [
        "00-17-15 09-24-2026 Active Call.wav",
        "07242580022PMStFrancoisCntyCeStFrancoisStFSDLaw1.wav",
        "158599113115PM030626Dispatch1Metro.wav",
        "20260923_192125_scanner_a_155.7975.mp3",
        "7-19-17 PM 09-24-26 EMS 1.mp3"
    ]

    copied = 0
    for fname in sample_filenames:
        src = os.path.join(REPO_ROOT, fname)
        dst = os.path.join(samples_dir, fname)
        if os.path.exists(src):
            shutil.copy2(src, dst)
            copied += 1

    print(f"[OK] Copied {copied} ProScan sample files into {samples_dir}")


def main():
    print("=== Generating Golden Test Fixtures for Rust Workspace ===")
    os.makedirs(FIXTURES_DIR, exist_ok=True)
    generate_metadata_fixtures()
    generate_formatter_fixtures()
    generate_driver_fixtures()
    generate_audio_fixtures()
    generate_feeder_fixtures()
    copy_proscan_samples()
    print("=== All Fixtures Generated Successfully ===")

if __name__ == "__main__":
    main()

