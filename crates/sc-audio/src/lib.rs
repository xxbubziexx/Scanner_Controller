//! Real-time audio capture, `PreRollRingBuffer`, `ProScan` VOX, resampling, loopback mixer,
//! and WAV/MP3 encoders for `Scanner_Controller`.
//!
//! Ported from [`AUDIO-IO-PIPELINE.md`](../../../docs/AUDIO-IO-PIPELINE.md),
//! [`VOX-ALGORITHM.md`](../../../docs/VOX-ALGORITHM.md), [`audio/recorder.py`](../../../../audio/recorder.py),
//! and [`feeder/scanscribe_feeder.py`](../../../../feeder/scanscribe_feeder.py).

pub mod devices;
pub mod encoder;
pub mod error;
pub mod loopback;
pub mod preroll;
pub mod resample;
pub mod stream;
pub mod vox;

pub use devices::{
    list_audio_input_devices, list_audio_input_devices_cached, list_audio_output_devices,
    list_audio_output_devices_cached, resolve_input_device, resolve_output_device,
    resolved_device_name, AudioDeviceInfo,
};
pub use encoder::{
    encode_audio_for_dispatch, encode_mp3_raw_lame, encode_wav_with_metadata, is_valid_mp3_payload,
};
pub use error::AudioError;
pub use loopback::{
    fill_rt_output_callback, limit_short_1, scale_out_volume, AudioRingBuffer, LoopbackMixer,
    PanMode, RtLoopbackShared,
};
pub use preroll::PreRollRingBuffer;
pub use resample::{
    bytes_to_i16_samples, i16_samples_to_bytes, resample_f32_linear, resample_pcm_int16,
    resample_pcm_rubato,
};
pub use stream::{
    is_generic_channel, is_generic_system, is_valid_metadata_status, push_rt_input_callback,
    should_update_session_status, ActiveCallSession, CompletedCall, DispatchCallback,
    ScannerAudioRecorder, SingleScannerAudioStream,
};
pub use vox::{
    compute_peak_equiv_rms, rms_to_vox_meter, ProScanVoxEngine, VoxEvent, VoxGate,
    DEFAULT_PROSCAN_VOX_HANG_MS, DEFAULT_PROSCAN_VOX_THRESHOLD,
};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use sc_core::{AudioChannelMode, ScannerStatus, ALLOWED_SAMPLE_RATES};
    use sc_metadata::{parse_pros_chunk, ProScanMetadata};
    use serde::Deserialize;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicU8};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    fn audio_fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("audio")
    }

    #[test]
    fn test_preroll_boundary_exact_sample_accuracy() {
        let sample_rate = 44_100usize;
        let exact_capacity = sample_rate * 2; // 88,200 samples = 176,400 bytes
        let mut preroll = PreRollRingBuffer::new(exact_capacity);

        // 1. Partial fill: feed 1.0s of sequential audio
        let seq_1s: Vec<i16> = (0..44_100).map(|i| (i % 25_000) as i16).collect();
        preroll.write(&seq_1s);
        let partial_bytes = preroll.get_preroll_bytes();
        assert_eq!(partial_bytes.len(), 44_100 * 2);
        let partial_samples = bytes_to_i16_samples(&partial_bytes).unwrap();
        assert_eq!(partial_samples, seq_1s);

        // 2. Overfill & wrap: write another 100,000 sequential samples (total 144,100 samples)
        let seq_overflow: Vec<i16> = (44_100..144_100).map(|i| (i % 25_000) as i16).collect();
        preroll.write(&seq_overflow);

        let full_bytes = preroll.get_preroll_bytes();
        assert_eq!(full_bytes.len(), exact_capacity * 2);
        assert_eq!(full_bytes.len(), 176_400);

        let full_samples = bytes_to_i16_samples(&full_bytes).unwrap();
        assert_eq!(full_samples.len(), exact_capacity);
        let expected: Vec<i16> = ((144_100 - exact_capacity)..144_100)
            .map(|i| (i % 25_000) as i16)
            .collect();
        assert_eq!(full_samples, expected);
    }

    #[derive(Debug, Deserialize)]
    struct VoxExpectedFixture {
        sample_rate: u32,
        total_samples: usize,
        voice_start_sample: usize,
        voice_end_sample: usize,
        expected_gate_open_sample: usize,
        hang_time_ms: u64,
        expected_gate_close_sample: usize,
    }

    #[test]
    fn test_vox_fixture_sample_exact_parity() {
        let dir = audio_fixtures_dir();
        let pcm_bytes = std::fs::read(dir.join("vox_test_16k.pcm")).unwrap();
        let expected_json = std::fs::read_to_string(dir.join("vox_expected.json")).unwrap();
        let expected: VoxExpectedFixture = serde_json::from_str(&expected_json).unwrap();

        let samples = bytes_to_i16_samples(&pcm_bytes).unwrap();
        assert_eq!(samples.len(), expected.total_samples);
        assert_eq!(expected.voice_start_sample, 8000);
        assert_eq!(expected.voice_end_sample, 24000);

        let mut gate =
            VoxGate::with_default_voice_threshold(expected.sample_rate, expected.hang_time_ms);
        let events = gate.process_samples(&samples);

        assert_eq!(
            events,
            vec![
                VoxEvent::GateOpen {
                    sample_index: expected.expected_gate_open_sample
                },
                VoxEvent::GateClose {
                    sample_index: expected.expected_gate_close_sample
                },
            ]
        );
    }

    #[test]
    fn test_proscan_vox_engine_arithmetic_and_hang() {
        // Silence -> 0 on 0..90 scale (-90 dBFS)
        assert_eq!(rms_to_vox_meter(0), 0);
        // Full scale 32767 -> 90 on 0..90 scale (0 dBFS)
        assert_eq!(rms_to_vox_meter(32767), 90);
        // -60 dBFS (~33 linear) -> 30 on 0..90 scale
        assert_eq!(rms_to_vox_meter(33), 30);

        let mut engine = ProScanVoxEngine::new(
            10,
            DEFAULT_PROSCAN_VOX_THRESHOLD,
            DEFAULT_PROSCAN_VOX_HANG_MS,
        );
        let silence = vec![0_i16; 1600];
        let loud = vec![12_000_i16; 1600];

        let (m0, latched0) = engine.process_mono_buffer(&silence, 100);
        assert_eq!(m0, 0);
        assert!(!latched0);

        // 0-buffer attack: latches immediately on first loud buffer
        let (m1, latched1) = engine.process_mono_buffer(&loud, 100);
        assert!(m1 >= DEFAULT_PROSCAN_VOX_THRESHOLD);
        assert!(latched1);
        assert!(engine.is_threshold_triggered());

        // Drop to silence: first 2 buffers flush the 3-buffer (round(10*0.15)+1) smoothing window,
        // and the next 19 buffers (1900ms) remain inside the 2000ms hang time
        for _ in 0..21 {
            let (_, latched) = engine.process_mono_buffer(&silence, 100);
            assert!(latched);
        }
        // At 2000ms after the smoothing window drops below threshold, unlatches
        let (_, latched_end) = engine.process_mono_buffer(&silence, 100);
        assert!(!latched_end);
    }

    #[test]
    fn test_resample_fixtures_parity_and_rubato() {
        let res_dir = audio_fixtures_dir().join("resample");
        let pairs = [
            (44100u32, 16000u32),
            (16000, 8000),
            (8000, 16000),
            (22050, 16000),
            (44100, 22050),
        ];

        for (in_rate, out_rate) in pairs {
            let in_bytes =
                std::fs::read(res_dir.join(format!("in_{in_rate}_to_{out_rate}.pcm"))).unwrap();
            let expected_bytes =
                std::fs::read(res_dir.join(format!("out_{in_rate}_to_{out_rate}.pcm"))).unwrap();

            // 1. Linear resampler parity against Python's resample_pcm_int16 fixture
            let actual_bytes = resample_pcm_int16(&in_bytes, in_rate, out_rate).unwrap();
            assert_eq!(
                actual_bytes.len(),
                expected_bytes.len(),
                "byte length mismatch for {in_rate}->{out_rate}"
            );

            let actual_samples = bytes_to_i16_samples(&actual_bytes).unwrap();
            let expected_samples = bytes_to_i16_samples(&expected_bytes).unwrap();
            for (idx, (&a, &e)) in actual_samples
                .iter()
                .zip(expected_samples.iter())
                .enumerate()
            {
                let diff = (i32::from(a) - i32::from(e)).abs();
                assert!(
                    diff <= 2,
                    "sample {idx} mismatch for {in_rate}->{out_rate}: actual={a}, expected={e}"
                );
            }

            // 2. Rubato band-limited FFT resampler length and signal preservation
            let rubato_bytes = resample_pcm_rubato(&in_bytes, in_rate, out_rate).unwrap();
            let rubato_samples = bytes_to_i16_samples(&rubato_bytes).unwrap();
            let sample_count_diff =
                (rubato_samples.len() as isize - expected_samples.len() as isize).abs();
            assert!(
                sample_count_diff <= 2,
                "rubato length mismatch for {in_rate}->{out_rate}"
            );
            let rms = compute_peak_equiv_rms(&rubato_samples);
            assert!(
                (12_000..=17_500).contains(&rms),
                "rubato peak-equiv RMS {rms} out of expected 15000 range for {in_rate}->{out_rate}"
            );
        }
    }

    #[test]
    fn test_underrun_recovery_no_lockout() {
        let mut ring = AudioRingBuffer::new(96_000, 2, 2048);
        assert!(ring.is_buffering());

        // Prime with 2048 stereo frames of 0.5
        let prime_data = vec![[0.5_f32, 0.5_f32]; 2048];
        ring.write_stereo_frames(&prime_data);
        assert!(!ring.is_buffering());
        assert_eq!(ring.available(), 2048);

        // Read block 1 (1024 frames)
        let mut out_block1 = vec![[0.0_f32; 2]; 1024];
        ring.read_stereo_frames(&mut out_block1, PanMode::StereoSplit);
        assert!(out_block1
            .iter()
            .all(|f| (f[0] - 0.5).abs() < 1e-6 && (f[1] - 0.5).abs() < 1e-6));
        assert_eq!(ring.available(), 1024);

        // Read block 2 (drains buffer completely)
        let mut out_block2 = vec![[0.0_f32; 2]; 1024];
        ring.read_stereo_frames(&mut out_block2, PanMode::StereoSplit);
        assert!(out_block2
            .iter()
            .all(|f| (f[0] - 0.5).abs() < 1e-6 && (f[1] - 0.5).abs() < 1e-6));
        assert_eq!(ring.available(), 0);

        // Starvation / Underrun test: consumer reads when available is 0
        let mut out_underrun = vec![[-99.0_f32; 2]; 1024];
        ring.read_stereo_frames(&mut out_underrun, PanMode::StereoSplit);
        assert!(out_underrun.iter().all(|f| f[0] == 0.0 && f[1] == 0.0));
        // Critical assertion: must NOT re-lock into buffering
        assert!(!ring.is_buffering());

        // Partial underrun: write 512 frames, read 1024
        let partial_data = vec![[0.75_f32, 0.75_f32]; 512];
        ring.write_stereo_frames(&partial_data);
        assert_eq!(ring.available(), 512);

        let mut out_partial = vec![[0.0_f32; 2]; 1024];
        ring.read_stereo_frames(&mut out_partial, PanMode::StereoSplit);
        assert!(out_partial[..512]
            .iter()
            .all(|f| (f[0] - 0.75).abs() < 1e-6));
        assert!(out_partial[512..].iter().all(|f| f[0] == 0.0));
        assert_eq!(ring.available(), 0);

        // Immediate resumption: next block plays immediately without lockout
        let resume_data = vec![[0.9_f32, 0.9_f32]; 1024];
        ring.write_stereo_frames(&resume_data);
        let mut out_resume = vec![[0.0_f32; 2]; 1024];
        ring.read_stereo_frames(&mut out_resume, PanMode::StereoSplit);
        assert!(out_resume.iter().all(|f| (f[0] - 0.9).abs() < 1e-6));
    }

    #[test]
    fn test_smooth_declicking_gain_ramp_and_proscan_volume_curve() {
        // Verify ProScan ScaleOutVolume & LimitShort1 (AUDIO-IO-PIPELINE.md §6)
        assert_eq!(scale_out_volume(0), 0.0);
        assert!((scale_out_volume(30) - 1.0).abs() < 1e-9);
        assert!((scale_out_volume(20) - 0.316_227_766).abs() < 1e-6);
        assert!((scale_out_volume(40) - 3.162_277_66).abs() < 1e-6);
        assert_eq!(limit_short_1(20_000.0, 2.0), 32767);
        assert_eq!(limit_short_1(-20_000.0, 2.0), -32768);

        let mut mixer = LoopbackMixer::new(true, 1.0, PanMode::StereoSplit, 44_100);
        let ramp_samples = mixer.ramp_samples();
        assert_eq!(ramp_samples, 661);

        // Prime with 2048 steady samples to complete initial pre-buffering
        mixer.push_audio("scanner_a", &vec![1.0_f32; 2048], 44_100);
        let mut drain = vec![[0.0_f32; 2]; 2048];
        mixer
            .ring_buffer
            .read_stereo_frames(&mut drain, PanMode::StereoSplit);
        assert_eq!(mixer.ring_buffer.available(), 0);

        // Trigger mutual exclusion mute on Scanner A
        mixer.mute_scanner("scanner_a", true);
        assert_eq!(mixer.target_gain("scanner_a"), 0.0);

        let input_samples = vec![1.0_f32; 1024];
        mixer.push_audio("scanner_a", &input_samples, 44_100);

        let mut ramp_out = vec![[0.0_f32; 2]; 1024];
        mixer
            .ring_buffer
            .read_stereo_frames(&mut ramp_out, PanMode::StereoSplit);

        let left_ch: Vec<f32> = ramp_out.iter().map(|f| f[0]).collect();
        assert!((left_ch[0] - 1.0).abs() < 0.01);
        for w in left_ch[..ramp_samples].windows(2) {
            assert!(w[1] <= w[0]);
        }
        assert_eq!(left_ch[ramp_samples], 0.0);
        assert_eq!(*left_ch.last().unwrap(), 0.0);

        // Subsequent block while muted is dropped without touching ring buffer
        let avail_before = mixer.ring_buffer.available();
        mixer.push_audio("scanner_a", &input_samples, 44_100);
        assert_eq!(mixer.ring_buffer.available(), avail_before);

        // Unmute ramp
        mixer.mute_scanner("scanner_a", false);
        assert_eq!(mixer.target_gain("scanner_a"), 1.0);
        mixer.push_audio("scanner_a", &input_samples, 44_100);

        let mut unmute_out = vec![[0.0_f32; 2]; 1024];
        mixer
            .ring_buffer
            .read_stereo_frames(&mut unmute_out, PanMode::StereoSplit);
        let left_unmute: Vec<f32> = unmute_out.iter().map(|f| f[0]).collect();
        assert!(left_unmute[0].abs() < 0.01);
        for w in left_unmute[..ramp_samples].windows(2) {
            assert!(w[1] >= w[0]);
        }
        assert_eq!(left_unmute[ramp_samples], 1.0);
        assert_eq!(*left_unmute.last().unwrap(), 1.0);
    }

    #[test]
    fn test_rt_callbacks_zero_lock_and_spsc_flow() {
        let (mut prod_in, mut cons_in) = rtrb::RingBuffer::<f32>::new(1024);
        let level = AtomicU8::new(0);
        let overflow = AtomicU64::new(0);

        // Interleaved stereo: Left = 0.8, Right = 0.2
        let stereo_in = [0.8_f32, 0.2_f32, 0.8_f32, 0.2_f32];
        push_rt_input_callback(
            &stereo_in,
            2,
            AudioChannelMode::Left,
            &mut prod_in,
            &level,
            &overflow,
        );
        assert_eq!(cons_in.slots(), 2);
        assert!((cons_in.pop().unwrap() - 0.8).abs() < 1e-6);
        assert!((cons_in.pop().unwrap() - 0.8).abs() < 1e-6);
        assert!(level.load(std::sync::atomic::Ordering::Relaxed) > 0);
        assert_eq!(overflow.load(std::sync::atomic::Ordering::Relaxed), 0);

        // Test lock-free RT output callback filler
        let (mut prod_a, mut cons_a) = rtrb::RingBuffer::<f32>::new(128);
        let (mut prod_b, mut cons_b) = rtrb::RingBuffer::<f32>::new(128);
        let shared = RtLoopbackShared::new(PanMode::StereoSplit, 2);
        for _ in 0..4 {
            prod_a.push(0.6).unwrap();
            prod_b.push(0.4).unwrap();
        }
        let mut out_interleaved = [0.0_f32; 8];
        fill_rt_output_callback(&mut out_interleaved, 2, &mut cons_a, &mut cons_b, &shared);
        assert!((out_interleaved[0] - 0.6).abs() < 1e-6);
        assert!((out_interleaved[1] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn test_wav_and_mp3_encoders_parse_with_sc_metadata() {
        let meta = ProScanMetadata {
            scanner: "BCD436HP".to_string(),
            system_name: "Metro P25".to_string(),
            department_name: "Fire Dispatch".to_string(),
            channel_name: "Fire Ops 1".to_string(),
            tgid: "10401".to_string(),
            frequency: "851.2500".to_string(),
            ..ProScanMetadata::default()
        };

        // Raw negative 16-bit PCM starting with 0xFFFF must NEVER be misidentified as MP3
        let neg_pcm = vec![0xFF_u8; 512];
        assert!(!is_valid_mp3_payload(&neg_pcm));

        for rate in ALLOWED_SAMPLE_RATES {
            // 0.25s of 440 Hz tone
            let n_samples = (rate / 4) as usize;
            let samples: Vec<i16> = (0..n_samples)
                .map(|i| {
                    let t = (i as f64) / f64::from(rate);
                    ((2.0 * std::f64::consts::PI * 440.0 * t).sin() * 14_000.0) as i16
                })
                .collect();
            let pcm_bytes = i16_samples_to_bytes(&samples);

            // 1. WAV encode & parse back pros chunk
            let (wav_bytes, wav_ext) =
                encode_audio_for_dispatch(Some(&pcm_bytes), "WAV", rate, &meta, "%C (%TG)", "%S")
                    .unwrap();
            assert_eq!(wav_ext, "wav");
            assert!(wav_bytes.starts_with(b"RIFF"));
            assert!(!is_valid_mp3_payload(&wav_bytes));
            let (parsed_wav_meta, _) = parse_pros_chunk(&wav_bytes).unwrap();
            assert_eq!(parsed_wav_meta.system_name, "Metro P25");
            assert_eq!(parsed_wav_meta.channel_name, "Fire Ops 1");
            assert_eq!(parsed_wav_meta.tgid, "10401");

            // 2. MP3 encode via LAME & parse back pros chunk
            let (mp3_bytes, mp3_ext) =
                encode_audio_for_dispatch(Some(&pcm_bytes), "MP3", rate, &meta, "%C (%TG)", "%S")
                    .unwrap();
            assert_eq!(mp3_ext, "mp3");
            assert!(mp3_bytes.starts_with(b"ID3"));
            assert!(is_valid_mp3_payload(&mp3_bytes));
            let (parsed_mp3_meta, _) = parse_pros_chunk(&mp3_bytes).unwrap();
            assert_eq!(parsed_mp3_meta.system_name, "Metro P25");
            assert_eq!(parsed_mp3_meta.channel_name, "Fire Ops 1");
            assert_eq!(parsed_mp3_meta.tgid, "10401");
        }
    }

    #[test]
    fn test_per_scanner_session_lifecycle_and_abort() {
        let dispatched: Arc<Mutex<Vec<CompletedCall>>> = Arc::new(Mutex::new(Vec::new()));
        let disp_clone = Arc::clone(&dispatched);
        let cb: DispatchCallback = Arc::new(move |call| {
            disp_clone.lock().unwrap().push(call);
        });

        let mut recorder = ScannerAudioRecorder::new(
            16_000,
            0.05,
            0.0,
            Some(cb),
            None,
            None,
            false,
            0.8,
            "STEREO_SPLIT",
        );
        recorder.configure_scanner("scanner_a", None, None, AudioChannelMode::Mono, true);
        recorder.configure_scanner("scanner_b", None, None, AudioChannelMode::Right, true);

        let mut status_a = ScannerStatus::new("scanner_a", "Scanner A", "BCD436HP");
        status_a.channel_name = "Dispatch 1".to_string();
        status_a.frequency = Some(461.525);

        let mut status_b = ScannerStatus::new("scanner_b", "Scanner B", "BCD996P2");
        status_b.channel_name = "Tac 2".to_string();
        status_b.frequency = Some(154.280);

        // 1. Squelch open on Scanner A -> feed audio -> squelch close -> hang time expire -> dispatch
        recorder.on_squelch_open("scanner_a", &status_a);
        assert!(recorder.streams["scanner_a"].has_active_session());
        let dummy_pcm = vec![0x10_u8; 8000];
        recorder.streams["scanner_a"].feed_pcm_bytes(&dummy_pcm);
        thread::sleep(Duration::from_millis(20));
        recorder.on_squelch_close("scanner_a", &status_a);
        assert!(!recorder.streams["scanner_a"].has_active_session());
        assert!(recorder.streams["scanner_a"].has_pending_release());

        thread::sleep(Duration::from_millis(65));
        recorder.check_hang_time_expirations();

        assert_eq!(dispatched.lock().unwrap().len(), 1);
        assert_eq!(dispatched.lock().unwrap()[0].scanner_id, "scanner_a");
        assert_eq!(
            dispatched.lock().unwrap()[0].status.channel_name,
            "Dispatch 1"
        );

        // 2. Squelch open on Scanner B -> feed audio -> abort_session (mutual exclusion) -> no dispatch
        recorder.on_squelch_open("scanner_b", &status_b);
        assert!(recorder.streams["scanner_b"].has_active_session());
        recorder.streams["scanner_b"].feed_pcm_bytes(&dummy_pcm);
        recorder.abort_session("scanner_b");
        assert!(!recorder.streams["scanner_b"].has_active_session());
        assert!(!recorder.streams["scanner_b"].has_pending_release());

        thread::sleep(Duration::from_millis(65));
        recorder.check_hang_time_expirations();
        assert_eq!(dispatched.lock().unwrap().len(), 1);
    }
}
