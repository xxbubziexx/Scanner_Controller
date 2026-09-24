import unittest
import struct
import os
import sys
import datetime

# Add project root to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from metadata.proscan_metadata import (
    ProScanMetadata,
    build_pros_chunk,
    parse_pros_chunk,
    format_template,
    format_streaming_metadata,
    sanitize_filename,
    sanitize_folder_path,
    resolve_collision_filename,
    write_mp3_id3v23,
    write_wav_riff_info,
    clean_ascii_1
)

class TestProScanMetadata(unittest.TestCase):

    def setUp(self):
        self.sample_meta = ProScanMetadata(
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
            rssi="4",
            service_type="Fire Dispatch",
            service_color="Red",
            alert_color="Blue",
            dmr_slot="1",
            comm_port="COM5"
        )
        # Fixed test timestamp: 2026-09-23 15:30:00 (matches FORMATTERS-AND-FILENAME-SCHEME.md §9)
        self.fixed_dt = datetime.datetime(2026, 9, 23, 15, 30, 0)

    def test_pros_chunk_binary_structure(self):
        chunk_bytes = build_pros_chunk(self.sample_meta)
        
        # 1. Magic 'pros' header
        self.assertTrue(chunk_bytes.startswith(b"pros"))
        
        # 2. Length check (bytes 4..8)
        payload_len = struct.unpack("<I", chunk_bytes[4:8])[0]
        self.assertEqual(len(chunk_bytes), 8 + payload_len)
        
        # 3. Parsed roundtrip verification
        res = parse_pros_chunk(chunk_bytes)
        self.assertIsNotNone(res)
        parsed_meta, bytes_read = res
        self.assertEqual(bytes_read, len(chunk_bytes))
        self.assertEqual(parsed_meta.scanner, "BCD996P2")
        self.assertEqual(parsed_meta.system_name, "Metropolitan P25 Trunk")
        self.assertEqual(parsed_meta.channel_name, "Dispatch North")
        self.assertEqual(parsed_meta.tgid, "10401")
        self.assertEqual(parsed_meta.frequency, "851.2500")

    def test_fixed_width_padding(self):
        chunk_bytes = build_pros_chunk(self.sample_meta)
        
        # Tone padded to 24 chars: "Tone:DCS023                  \0"
        self.assertIn(b"Tone:DCS023                  \x00", chunk_bytes)
        
        # UID padded to 64 chars
        uid_expected = f"UID:{'704101'.ljust(64)}\x00".encode("ascii")
        self.assertIn(uid_expected, chunk_bytes)

        # UID# padded to 16 chars
        uid_num_expected = f"UID#:{'101'.ljust(16)}\x00".encode("ascii")
        self.assertIn(uid_num_expected, chunk_bytes)

    def test_three_counterintuitive_inversions(self):
        """
        Tests the three critical inversions from FORMATTERS-AND-FILENAME-SCHEME.md §1 & §9:
        - %MM is Month Name (Sep), %M is Month Number (09)
        - %YY is 4-digit Year (2026), %Y is 2-digit Year (26)
        - %D is Full Date (09/23/26), %DD is 2-digit Day (23)
        """
        template = "%MM-%M-%YY-%Y"
        expanded = format_template(template, self.sample_meta, now=self.fixed_dt)
        self.assertEqual(expanded, "Sep-09-2026-26")

        template_date = "%D %DD"
        expanded_date = format_template(template_date, self.sample_meta, now=self.fixed_dt, month_day_year_format="MM/dd/yy")
        self.assertEqual(expanded_date, "09/23/26 23")

    def test_chained_longest_first_ordering(self):
        """
        Tests that longest-prefix specifiers are substituted correctly without collisions:
        - %FT before %F
        - %TG before %T
        - %ST1, %STC before %ST, before %S
        - %DD, %DT before %D
        - %TT, %TA before %T
        """
        template = "%FT | %TG | %ST1 | %ST | %S | %F | %T"
        expanded = format_template(template, self.sample_meta, now=self.fixed_dt)
        # %FT -> talkgroup (10401)
        # %TG -> 10401
        # %ST1 -> Fire Dispatch
        # %ST -> BCD996P2
        # %S -> Metropolitan P25 Trunk
        # %F -> 851.2500S1
        # %T -> DCS023
        self.assertEqual(expanded, "10401 | 10401 | Fire Dispatch | BCD996P2 | Metropolitan P25 Trunk | 851.2500S1 | DCS023")

    def test_default_recording_format_and_sanitization(self):
        """
        Tests ProScan factory default %DT - %S - %C and file sanitization:
        '09/23/26 15:30:00 - County P25 - FIRE-TAC1' -> '09-23-26 15-30-00 - County P25 - FIRE-TAC1'
        """
        template = "%DT - %S - %C"
        formatted = format_template(template, self.sample_meta, now=self.fixed_dt, military_time=True)
        self.assertEqual(formatted, "09/23/26 15:30:00 - Metropolitan P25 Trunk - Dispatch North")

        sanitized = sanitize_filename(formatted)
        self.assertEqual(sanitized, "09-23-26 15-30-00 - Metropolitan P25 Trunk - Dispatch North")

    def test_engine_b_streaming_metadata(self):
        """
        Tests Engine B (Common_PS_RF.cs:784):
        - Supports %CL (listener count)
        - Date/time specifiers pass through literally (not supported in streamed metadata)
        """
        template = "%C - %TG (%CL) [%DT]"
        result = format_streaming_metadata(template, self.sample_meta, listener_count=7)
        self.assertEqual(result, "Dispatch North - 10401 (7) [%DT]")

    def test_filename_and_folder_sanitizers(self):
        # Filename sanitizer
        raw_bad = 'Dispatch/North: Fire*Rescue? <Tac> "1" | Special.'
        clean = sanitize_filename(raw_bad)
        self.assertNotIn("/", clean)
        self.assertNotIn(":", clean)
        self.assertNotIn("*", clean)
        self.assertNotIn("?", clean)
        self.assertNotIn("<", clean)
        self.assertNotIn(">", clean)
        self.assertNotIn("|", clean)
        self.assertFalse(clean.endswith("."))

        # Folder sanitizer with drive letter guard
        folder_bad = "C:\\Recordings/2026:09\\Fire:Dept/"
        clean_folder = sanitize_folder_path(folder_bad)
        self.assertTrue(clean_folder.startswith("C:\\"))
        self.assertNotIn("Fire:Dept", clean_folder)
        self.assertIn("Fire-Dept", clean_folder)
        self.assertTrue(clean_folder.endswith("\\"))

    def test_collision_filename_resolution(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            f1 = os.path.join(tmpdir, "851.2500 - Dispatch.wav")
            with open(f1, "wb") as f:
                f.write(b"data")

            # Must append ' #2' before extension, avoiding dot-in-frequency bug
            c2 = resolve_collision_filename(tmpdir, "851.2500 - Dispatch", "wav")
            self.assertEqual(c2, "851.2500 - Dispatch #2")

    def test_mp3_id3v23_tagging(self):
        fake_audio = b"\xFF\xFB\x90\x44" * 10
        tagged_mp3 = write_mp3_id3v23(fake_audio, self.sample_meta, title_template="%C", artist_template="%S", now=self.fixed_dt)
        
        self.assertTrue(tagged_mp3.startswith(b"ID3\x03\x00\x00"))
        self.assertIn(b"pros", tagged_mp3)
        self.assertIn(b"TSEE", tagged_mp3)
        self.assertIn(b"TIT2", tagged_mp3)
        self.assertIn(b"TPE1", tagged_mp3)

    def test_wav_riff_info_tagging(self):
        fake_pcm = b"\x00\x00" * 400
        tagged_wav = write_wav_riff_info(fake_pcm, self.sample_meta, now=self.fixed_dt)
        
        self.assertTrue(tagged_wav.startswith(b"RIFF"))
        self.assertIn(b"WAVE", tagged_wav)
        self.assertIn(b"LIST", tagged_wav)
        self.assertIn(b"INFO", tagged_wav)
        self.assertIn(b"IPRD", tagged_wav)
        self.assertIn(b"pros", tagged_wav)

    def test_all_33_custom_formatters_dialog_placeholders(self):
        """
        Explicitly tests all 33 placeholders shown in the ProScan Custom Formatters dialog:
        Left column:
        %ST, %DT, %D, %TT, %H, %P, %DD, %M, %MM, %Y, %YY, %SC, %AF1, %AF2, %FA, %S, %SI, %DE
        Right column:
        %C, %FT, %F, %T, %TG, %U1, %U2, %U3, %MD, %R, %TA, %TB, %AC, %ST1, %STC
        """
        meta = ProScanMetadata(
            scanner="BCD436HP",
            favorite_name="Favorites 1",
            system_name="Metropolitan P25",
            site_name="North Site",
            department_name="Fire Ops",
            channel_name="Dispatch North",
            frequency="851.2500",
            modulation="NFM",
            tone="DCS023",
            tgid="10401",
            uid="704101",
            uid_number="101",
            ftoa="156.7",
            ftob="167.9",
            rssi="5",
            alert_color="Red",
            service_type="Fire Dispatch",
            service_color="Blue",
            info_sc_state="Connected",
            comm_port="COM6",
            program_path="C:\\ProScan",
            program_filename="ProScan.exe",
            dmr_slot="1"
        )
        fixed_time = datetime.datetime(2026, 9, 23, 15, 30, 45)

        # Left column
        self.assertEqual(format_template("%ST", meta, now=fixed_time), "BCD436HP")
        self.assertEqual(format_template("%DT", meta, now=fixed_time, month_day_year_format="MM/dd/yy", military_time=True), "09/23/26 15:30:45")
        self.assertEqual(format_template("%D", meta, now=fixed_time, month_day_year_format="MM/dd/yy"), "09/23/26")
        self.assertEqual(format_template("%TT", meta, now=fixed_time, military_time=True), "15:30:45")
        self.assertEqual(format_template("%TT", meta, now=fixed_time, military_time=False), "3:30:45 PM")
        self.assertEqual(format_template("%H", meta, now=fixed_time), "15")
        self.assertEqual(format_template("%P", meta, now=fixed_time), "6")
        self.assertEqual(format_template("%DD", meta, now=fixed_time), "23")
        self.assertEqual(format_template("%M", meta, now=fixed_time), "09")
        self.assertEqual(format_template("%MM", meta, now=fixed_time), "Sep")
        self.assertEqual(format_template("%Y", meta, now=fixed_time), "26")
        self.assertEqual(format_template("%YY", meta, now=fixed_time), "2026")
        self.assertEqual(format_template("%SC", meta, now=fixed_time), "Connected")
        self.assertEqual(format_template("%AF1", meta, now=fixed_time), "C:\\ProScan")
        self.assertEqual(format_template("%AF2", meta, now=fixed_time), "ProScan.exe")
        self.assertEqual(format_template("%FA", meta, now=fixed_time), "Favorites 1")
        self.assertEqual(format_template("%S", meta, now=fixed_time), "Metropolitan P25")
        self.assertEqual(format_template("%SI", meta, now=fixed_time), "North Site")
        self.assertEqual(format_template("%DE", meta, now=fixed_time), "Fire Ops")

        # Right column
        self.assertEqual(format_template("%C", meta, now=fixed_time), "Dispatch North")
        self.assertEqual(format_template("%FT", meta, now=fixed_time), "10401")
        self.assertEqual(format_template("%F", meta, now=fixed_time), "851.2500S1")
        self.assertEqual(format_template("%T", meta, now=fixed_time), "DCS023")
        self.assertEqual(format_template("%TG", meta, now=fixed_time), "10401")
        self.assertEqual(format_template("%U1", meta, now=fixed_time), "704101")
        self.assertEqual(format_template("%U2", meta, now=fixed_time), "101")
        self.assertEqual(format_template("%U3", meta, now=fixed_time), "101")
        self.assertEqual(format_template("%MD", meta, now=fixed_time), "NFM")
        self.assertEqual(format_template("%R", meta, now=fixed_time), "5")
        self.assertEqual(format_template("%TA", meta, now=fixed_time), "156.7")
        self.assertEqual(format_template("%TB", meta, now=fixed_time), "167.9")
        self.assertEqual(format_template("%AC", meta, now=fixed_time), "Red")
        self.assertEqual(format_template("%ST1", meta, now=fixed_time), "Fire Dispatch")
        self.assertEqual(format_template("%STC", meta, now=fixed_time), "Blue")

    def test_user_exact_filename_and_tit2_examples(self):
        """
        Asserts the user's exact production examples produce 100% bit-exact strings:
        %TT %D %C -> Filename: 5-20-46 PM 09-23-26 Fire Main 1.mp3
        %TG %G %C -> TIT2 metadata: St. Francois Central Dispatch Fire Main 1
        """
        meta = ProScanMetadata(
            department_name="St. Francois Central Dispatch",
            channel_name="Fire Main 1",
            tgid=""
        )
        dt = datetime.datetime(2026, 9, 23, 17, 20, 46)

        # 1. Filename test
        stem = format_template("%TT %D %C", meta, dt, military_time=False)
        filename = sanitize_filename(stem) + ".mp3"
        self.assertEqual(filename, "5-20-46 PM 09-23-26 Fire Main 1.mp3")

        # 2. TIT2 metadata test
        tit2 = clean_ascii_1(format_template("%TG %G %C", meta, dt))
        self.assertEqual(tit2, "St. Francois Central Dispatch Fire Main 1")

if __name__ == "__main__":
    unittest.main()
