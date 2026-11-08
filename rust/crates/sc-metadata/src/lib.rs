//! `sc-metadata`: `ProScan` pros chunks, ID3v2.3, RIFF LIST/INFO, and 33 % formatters.

#![deny(unsafe_code)]

pub mod clean;
pub mod formatters;
pub mod id3;
pub mod pros;
pub mod record;
pub mod riff;

pub use clean::clean_ascii_1;
pub use formatters::{
    format_streaming_metadata, format_template, sanitize_filename, sanitize_folder_path,
};
pub use id3::{build_id3v23_text_frame, encode_syncsafe, write_mp3_id3v23};
pub use pros::{build_pros_chunk, parse_pros_chunk};
pub use record::ProScanMetadata;
pub use riff::{build_riff_info_subchunk, write_wav_riff_info};

/// Returns the version of the `sc-metadata` crate.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::unreadable_literal)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::Value;

    fn get_fixtures_dir() -> std::path::PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        std::path::Path::new(manifest_dir)
            .join("..")
            .join("..")
            .join("fixtures")
    }

    #[test]
    fn test_formatters_json_100_percent_parity() {
        let fixtures_dir = get_fixtures_dir();
        let formatters_path = fixtures_dir.join("formatters.json");
        let content = std::fs::read_to_string(&formatters_path)
            .expect("Failed to read fixtures/formatters.json");
        let json: Value = serde_json::from_str(&content).expect("Failed to parse formatters.json");

        let meta_obj = &json["metadata_snapshot"];
        let meta: ProScanMetadata = serde_json::from_value(meta_obj.clone())
            .expect("Failed to deserialize snapshot metadata");

        // Fixed test date: 2026-09-23 15:30:00 (matches Python fixture generator)
        let fixed_now = chrono::Local
            .from_local_datetime(
                &chrono::NaiveDateTime::parse_from_str("2026-09-23 15:30:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap(),
            )
            .single()
            .unwrap();

        // 1. Check all template test cases
        let cases = json["template_cases"]
            .as_array()
            .expect("template_cases array");
        for case in cases {
            let tmpl = case["template"].as_str().unwrap();
            let exp_a = case["engine_a_expected"].as_str().unwrap();
            let exp_b = case["engine_b_expected"].as_str().unwrap();

            let act_a = format_template(tmpl, &meta, Some(fixed_now));
            let act_b = format_streaming_metadata(tmpl, &meta, 42);

            assert_eq!(
                act_a, exp_a,
                "Engine A template '{tmpl}' mismatch: got '{act_a}', expected '{exp_a}'"
            );
            assert_eq!(
                act_b, exp_b,
                "Engine B template '{tmpl}' mismatch: got '{act_b}', expected '{exp_b}'"
            );
        }

        // 2. Check sanitizer test cases
        let sanitizers = json["sanitizer_cases"]
            .as_array()
            .expect("sanitizer_cases array");
        for item in sanitizers {
            let raw = item["raw"].as_str().unwrap();
            let exp_clean = item["clean_ascii"].as_str().unwrap();
            let exp_fn = item["sanitized_filename"].as_str().unwrap();
            let exp_dir = item["sanitized_folder"].as_str().unwrap();

            assert_eq!(clean_ascii_1(raw, 253), exp_clean);
            assert_eq!(sanitize_filename(raw), exp_fn);
            assert_eq!(sanitize_folder_path(raw), exp_dir);
        }
    }

    #[test]
    fn test_all_23_binary_metadata_fixtures_byte_exact() {
        let fixtures_dir = get_fixtures_dir().join("metadata");
        let cases_path = fixtures_dir.join("metadata_cases.json");
        let content =
            std::fs::read_to_string(&cases_path).expect("Failed to read metadata_cases.json");
        let cases_map: std::collections::HashMap<String, ProScanMetadata> =
            serde_json::from_str(&content).expect("Failed to parse metadata_cases.json");

        // Fixed test date matching Python generator
        let fixed_now = chrono::Local
            .from_local_datetime(
                &chrono::NaiveDateTime::parse_from_str("2026-09-23 15:30:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap(),
            )
            .single()
            .unwrap();

        // Extract dummy_mp3 from empty_templates_id3.bin (which returns dummy_mp3 untouched)
        let empty_id3_path = fixtures_dir.join("empty_templates_id3.bin");
        let dummy_mp3 =
            std::fs::read(&empty_id3_path).expect("Failed to read empty_templates_id3.bin");

        // Extract dummy_pcm from typical_conventional_riff.bin (header is 44 bytes, PCM is 32000 bytes)
        let conv_riff_path = fixtures_dir.join("typical_conventional_riff.bin");
        let conv_riff_bytes =
            std::fs::read(&conv_riff_path).expect("Failed to read conv riff fixture");
        let dummy_pcm = &conv_riff_bytes[44..44 + 32000];

        for (name, meta) in &cases_map {
            // 1. Check pros chunk
            let pros_file = fixtures_dir.join(format!("{name}_pros.bin"));
            let expected_pros = std::fs::read(&pros_file).expect("pros fixture missing");
            let actual_pros = build_pros_chunk(meta);
            assert_eq!(
                actual_pros, expected_pros,
                "PROS chunk mismatch for '{name}'"
            );

            // Verify parse_pros_chunk roundtrip
            let (parsed, read_len) =
                parse_pros_chunk(&expected_pros).expect("Failed to parse pros chunk");
            assert_eq!(read_len, expected_pros.len());
            let rebuilt = build_pros_chunk(&parsed);
            assert_eq!(rebuilt, expected_pros, "PROS rebuilt mismatch for '{name}'");

            // 2. Check MP3 ID3v2.3 tag
            let id3_file = fixtures_dir.join(format!("{name}_id3.bin"));
            let expected_id3 = std::fs::read(&id3_file).expect("id3 fixture missing");
            let actual_id3 = write_mp3_id3v23(&dummy_mp3, meta, "%C", "%S", "%G", Some(fixed_now));
            assert_eq!(
                actual_id3, expected_id3,
                "MP3 ID3 tag mismatch for '{name}'"
            );

            // 3. Check WAV RIFF INFO container
            let riff_file = fixtures_dir.join(format!("{name}_riff.bin"));
            let expected_riff = std::fs::read(&riff_file).expect("riff fixture missing");
            let actual_riff =
                write_wav_riff_info(dummy_pcm, meta, "%C", "%S", 16000, 1, 16, Some(fixed_now));
            assert_eq!(
                actual_riff, expected_riff,
                "WAV RIFF container mismatch for '{name}'"
            );
        }

        // Test edge case: empty templates MP3
        let typical_conv = &cases_map["typical_conventional"];
        let actual_empty_id3 =
            write_mp3_id3v23(&dummy_mp3, typical_conv, "", "", "", Some(fixed_now));
        assert_eq!(actual_empty_id3, dummy_mp3);

        // Test edge case: empty templates WAV
        let empty_riff_path = fixtures_dir.join("empty_templates_riff.bin");
        let expected_empty_riff = std::fs::read(&empty_riff_path).unwrap();
        let actual_empty_riff = write_wav_riff_info(
            dummy_pcm,
            typical_conv,
            "",
            "",
            16000,
            1,
            16,
            Some(fixed_now),
        );
        assert_eq!(actual_empty_riff, expected_empty_riff);
    }

    #[test]
    fn test_parse_real_proscan_sample_files() {
        let samples_dir = get_fixtures_dir().join("proscan_samples");
        if !samples_dir.exists() {
            return;
        }

        let tagged_files = [
            "00-17-15 09-24-2026 Active Call.wav",
            "20260923_192125_scanner_a_155.7975.mp3",
            "7-19-17 PM 09-24-26 EMS 1.mp3",
        ];

        for fname in tagged_files {
            let path = samples_dir.join(fname);
            if path.exists() {
                let bytes = std::fs::read(&path).expect("Failed to read sample file");
                let parsed_res = parse_pros_chunk(&bytes);
                assert!(
                    parsed_res.is_some(),
                    "Sample file '{fname}' must contain a readable 'pros' chunk"
                );
                let (meta, _) = parsed_res.unwrap();
                assert!(
                    !meta.scanner.is_empty(),
                    "Scanner model must not be empty in '{fname}'"
                );
            }
        }

        let untagged_files = [
            "07242580022PMStFrancoisCntyCeStFrancoisStFSDLaw1.wav",
            "158599113115PM030626Dispatch1Metro.wav",
        ];

        for fname in untagged_files {
            let path = samples_dir.join(fname);
            if path.exists() {
                let bytes = std::fs::read(&path).expect("Failed to read sample file");
                let parsed_res = parse_pros_chunk(&bytes);
                assert!(
                    parsed_res.is_none(),
                    "Untagged file '{fname}' should return None"
                );
            }
        }
    }
}
