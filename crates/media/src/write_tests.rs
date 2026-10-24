// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The writer's tests: the depths and the dither are deterministic (the same export is the
//! same bytes), the compressed formats round-trip through symphonia's decoder, and the
//! refusals are said as the spec quotes them (`docs/spec/export.md` › Tests).

use std::path::{Path, PathBuf};

use super::{AudioFileSpec, AudioFormat, BitDepth, Dither, Mp3Rate, WriteMode, write_audio_file};
use crate::decode_file;

const SR: u32 = 48_000;

/// A 440 Hz sine at 0.5, one second, interleaved stereo.
fn sine() -> Vec<f32> {
    let frames = SR as usize;
    (0..frames)
        .flat_map(|frame| {
            let sample = (std::f64::consts::TAU * 440.0 * frame as f64 / f64::from(SR)).sin() * 0.5;
            [sample as f32, sample as f32]
        })
        .collect()
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-write-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn temp_file(name: &str) -> PathBuf {
    temp_dir().join(format!("{name}-{}.{}", std::process::id(), name))
}

fn cleanup(paths: &[&Path]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

/// The dithered, quantized samples the writer makes of `samples` at the depth, in the same
/// order the writer makes them.
fn expected_pcm(samples: &[f32], depth: BitDepth, seed: u64) -> Vec<i32> {
    let bits = match depth {
        BitDepth::Pcm16 => 16,
        _ => 24,
    };
    let mut noise = Dither::new(seed, bits);
    samples.iter().map(|s| noise.quantize(*s, true)).collect()
}

#[test]
fn a_16_bit_wav_is_bit_true_dither_of_the_float_source_and_the_same_bytes_every_run() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Wav,
        depth: BitDepth::Pcm16,
        sample_rate: SR,
        dither: true,
        seed: 7,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let (first, second) = (temp_file("w16a"), temp_file("w16b"));
    write_audio_file(&first, &samples, &spec, WriteMode::Replace).unwrap();
    write_audio_file(&second, &samples, &spec, WriteMode::Replace).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
    // Read back with hound at the depth's range and compare with the dither the seed makes.
    let mut reader = hound::WavReader::open(&first).unwrap();
    assert_eq!(reader.spec().sample_rate, SR);
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_format, hound::SampleFormat::Int);
    assert_eq!(reader.spec().bits_per_sample, 16);
    let written: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
    assert_eq!(written, expected_pcm(&samples, BitDepth::Pcm16, 7));
    cleanup(&[&first, &second]);
}

#[test]
fn a_24_bit_wav_is_bit_true_dither_of_the_float_source() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Wav,
        depth: BitDepth::Pcm24,
        sample_rate: SR,
        dither: true,
        seed: 11,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let path = temp_file("w24");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().bits_per_sample, 24);
    let written: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
    assert_eq!(written, expected_pcm(&samples, BitDepth::Pcm24, 11));
    cleanup(&[&path]);
}

#[test]
fn an_undithered_24_bit_wav_quantizes_round_to_nearest() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Wav,
        depth: BitDepth::Pcm24,
        sample_rate: SR,
        dither: false,
        seed: 3,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let path = temp_file("w24q");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    let written: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
    let expected: Vec<i32> = samples
        .iter()
        .map(|s| {
            (f64::from(*s) * f64::from(1u32 << 23)).round().clamp(-f64::from(1u32 << 23), f64::from((1u32 << 23) - 1))
                as i32
        })
        .collect();
    assert_eq!(written, expected);
    cleanup(&[&path]);
}

#[test]
fn a_32_bit_float_wav_round_trips_exactly() {
    let samples = sine();
    let spec = AudioFileSpec::float_wav(SR);
    let path = temp_file("w32f");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
    let written: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    assert_eq!(written, samples);
    cleanup(&[&path]);
}

#[test]
fn a_flac_export_decodes_to_the_same_length_rate_and_samples() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Flac,
        depth: BitDepth::Pcm24,
        sample_rate: SR,
        dither: true,
        seed: 5,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let path = temp_file("flac");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let decoded = decode_file(&path).unwrap();
    assert_eq!(decoded.sample_rate, SR);
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.samples.len(), samples.len());
    // FLAC is lossless at the depth: the decoded float is the quantized sample.
    for (decoded, quantized) in decoded.samples.iter().zip(expected_pcm(&samples, BitDepth::Pcm24, 5)) {
        let expected = f64::from(quantized) / f64::from(1u32 << 23);
        assert!((f64::from(*decoded) - expected).abs() < 1e-6, "{decoded} against {expected}");
    }
    cleanup(&[&path]);
}

/// The streamed file is the same audio flacenc's own whole-stream encode would make for the
/// same quantized mix, tested at the edge frames — a stream whose frames are in steps of the
/// encoder's block size, and one whose last frame is short. The two files' bytes differ where
/// this writer's header says its frame sizes are unknown (and where this writer's own encodes
/// do without flacenc's side-channel codings, whose subframes run past the depth the file
/// states), so the check is what either file decodes to: the samples, the length, the rate.
#[test]
fn a_flac_written_to_the_file_decodes_like_the_whole_stream_encode() {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use flacenc::source::Fill;

    let encode_to_stream = |samples: &[i32], bits: usize| -> Vec<u8> {
        let mut raw = flacenc::config::Encoder::default();
        let mut stereo = flacenc::config::StereoCoding::default();
        stereo.use_leftside = false;
        stereo.use_rightside = false;
        stereo.use_midside = false;
        raw.stereo_coding = stereo;
        let config = raw.into_verified().unwrap();
        let block_size = config.block_size;
        let mut stream = flacenc::component::Stream::new(48_000, 2, bits).unwrap();
        stream.stream_info_mut().set_block_sizes(block_size, block_size).unwrap();
        let mut framebuf = flacenc::source::FrameBuf::with_size(2, block_size).unwrap();
        let mut context = flacenc::source::Context::new(bits, 2);
        for (frame_number, chunk) in samples.chunks(block_size * 2).enumerate() {
            framebuf.fill_interleaved(chunk).unwrap();
            context.fill_interleaved(chunk).unwrap();
            let frame =
                flacenc::encode_fixed_size_frame(&config, &framebuf, frame_number, stream.stream_info()).unwrap();
            stream.add_frame(frame);
        }
        stream.stream_info_mut().set_total_samples(samples.len() / 2);
        stream.stream_info_mut().set_block_sizes(block_size, block_size).unwrap();
        stream.stream_info_mut().set_md5_digest(&context.md5_digest());
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).unwrap();
        sink.into_inner()
    };
    let decode_of_bytes = |bytes: &[u8]| -> suhina_engine::AudioData {
        let path = temp_file("whole-stream");
        std::fs::write(&path, bytes).unwrap();
        let decoded = decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        decoded
    };
    let decode_of_writer = |samples: &[f32], bits: usize| -> suhina_engine::AudioData {
        let path = temp_file("file-sunk");
        let spec = AudioFileSpec {
            format: AudioFormat::Flac,
            depth: if bits == 16 { BitDepth::Pcm16 } else { BitDepth::Pcm24 },
            sample_rate: SR,
            dither: true,
            seed: 5,
            mp3_rate: Mp3Rate::default(),
            title: String::new(),
        };
        write_audio_file(&path, samples, &spec, WriteMode::Replace).unwrap();
        let decoded = decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        decoded
    };
    let sine_of = |frames: usize| -> Vec<f32> {
        (0..frames)
            .flat_map(|frame| {
                let sample = (std::f64::consts::TAU * 300.0 * frame as f64 / f64::from(SR)).sin() * 0.6;
                [sample as f32, (sample * 3.0).clamp(-1.0, 1.0) as f32]
            })
            .collect()
    };
    // The quantized mix the writer would make, taken from its own dither's seed.
    for (samples, bits) in [
        (sine_of(4 + 4098), 24), // one over a block step, a short last frame
        (sine_of(4096), 24),     // exactly one block
        (sine_of(4096 * 3), 16), // exactly three
    ] {
        let quantized: Vec<i32> = {
            let bits_q = if bits == 16 { 16 } else { 24 };
            let mut noise = Dither::new(5, bits_q);
            samples.iter().map(|s| noise.quantize(*s, true)).collect()
        };
        let buffer_bytes = encode_to_stream(&quantized, bits);
        let buffer = decode_of_bytes(&buffer_bytes);
        let file = decode_of_writer(&samples, bits);
        assert_eq!(file.samples.len(), samples.len(), "bits {bits}: the file's own length");
        assert_eq!(buffer.samples.len(), samples.len(), "bits {bits}: the whole stream's length");
        assert_eq!(file.sample_rate, buffer.sample_rate, "bits {bits}: the rate");
        assert_eq!(file.channels, buffer.channels, "bits {bits}: the channels");
        for (index, (a, b)) in file.samples.iter().zip(buffer.samples.iter()).enumerate() {
            assert_eq!(a, b, "bits {bits}, frames {}: sample {index}", samples.len() / 2);
        }
    }
}

/// A mix whose frames end exactly on the encoder's block still carries its true sample count,
/// and the same render writes the same bytes.
#[test]
fn a_flac_whole_in_encoder_blocks_is_deterministic_and_true_in_length() {
    let frames = 4096;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let sample = (std::f64::consts::TAU * 440.0 * frame as f64 / f64::from(SR)).sin() * 0.5;
            [sample as f32, sample as f32]
        })
        .collect();
    let spec = AudioFileSpec {
        format: AudioFormat::Flac,
        depth: BitDepth::Pcm24,
        sample_rate: SR,
        dither: true,
        seed: 3,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let (first, second) = (temp_file("flac-whole1"), temp_file("flac-whole2"));
    write_audio_file(&first, &samples, &spec, WriteMode::Replace).unwrap();
    write_audio_file(&second, &samples, &spec, WriteMode::Replace).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap(), "the same bytes every run");
    let decoded = decode_file(&first).unwrap();
    assert_eq!(decoded.samples.len(), samples.len());
    assert_eq!(decoded.sample_rate, SR);
    assert_eq!(decoded.channels, 2);
    cleanup(&[&first, &second]);
}

/// Prim-by-prim differential: the same random call sequences through both sinks, each round
/// finished out and compared whole. The file sink is the buffer variant bit for bit, at every
/// width and alignment the writer's callers reach.
#[test]
fn the_sink_prims_agree_with_the_buffer_variant() {
    use super::FileSink;
    use flacenc::bitsink::{BitSink, ByteSink};
    // The same op sequence through both sinks; compared after every round. `FileSink` fields
    // are private, so a round is finished out into a byte vector and compared whole.
    let mut state = 0x1234_5678_9abc_def0u64;
    let rnd = |state: &mut u64, modulus: u64| -> u64 {
        *state ^= *state >> 12;
        *state ^= *state << 25;
        *state ^= *state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D) % modulus.max(1)
    };
    for round in 0..20_000 {
        let mut out = Vec::new();
        let mut file = FileSink::new(&mut out);
        let mut byte = ByteSink::new();
        let mut trace = String::new();
        for _ in 0..(rnd(&mut state, 7) + 1) {
            let draw = rnd(&mut state, 6);
            let width = [64usize, 32, 16, 8][rnd(&mut state, 4) as usize];
            let val = rnd(&mut state, 1u64 << width.min(62));
            match draw {
                0 => {
                    trace += &format!("write{width}({val}); ");
                    match width {
                        64 => {
                            file.write(val).unwrap();
                            byte.write(val).unwrap();
                        }
                        32 => {
                            file.write(val as u32).unwrap();
                            byte.write(val as u32).unwrap();
                        }
                        16 => {
                            file.write(val as u16).unwrap();
                            byte.write(val as u16).unwrap();
                        }
                        _ => {
                            file.write(val as u8).unwrap();
                            byte.write(val as u8).unwrap();
                        }
                    }
                }
                1 => {
                    trace += &format!("msbs{width}({val}, {n}); ", n = val % (width as u64 + 1));
                    match width {
                        64 => {
                            file.write_msbs(val, (val as usize) % width).unwrap();
                            byte.write_msbs(val, (val as usize) % width).unwrap();
                        }
                        32 => {
                            file.write_msbs(val as u32, (val as usize) % width).unwrap();
                            byte.write_msbs(val as u32, (val as usize) % width).unwrap();
                        }
                        16 => {
                            file.write_msbs(val as u16, (val as usize) % width).unwrap();
                            byte.write_msbs(val as u16, (val as usize) % width).unwrap();
                        }
                        _ => {
                            file.write_msbs(val as u8, (val as usize) % width).unwrap();
                            byte.write_msbs(val as u8, (val as usize) % width).unwrap();
                        }
                    }
                }
                2 => {
                    let n = (val % (width as u64 + 1)) as usize;
                    trace += &format!("lsbs{width}({val}, {n}); ");
                    match width {
                        64 => {
                            file.write_lsbs(val, n).unwrap();
                            byte.write_lsbs(val, n).unwrap();
                        }
                        32 => {
                            file.write_lsbs(val as u32, n).unwrap();
                            byte.write_lsbs(val as u32, n).unwrap();
                        }
                        16 => {
                            file.write_lsbs(val as u16, n).unwrap();
                            byte.write_lsbs(val as u16, n).unwrap();
                        }
                        _ => {
                            file.write_lsbs(val as u8, n).unwrap();
                            byte.write_lsbs(val as u8, n).unwrap();
                        }
                    }
                }
                3 => {
                    let n = (rnd(&mut state, 40)) as usize;
                    trace += &format!("zeros({n}); ");
                    file.write_zeros(n).unwrap();
                    byte.write_zeros(n).unwrap();
                }
                4 => {
                    let bytes: [u8; 4] = [val as u8, (val >> 8) as u8, (val >> 16) as u8, (val >> 24) as u8];
                    trace += &format!("bytes_aligned({:02x?}); ", &bytes[..(val as usize) % 5]);
                    file.write_bytes_aligned(&bytes[..(val as usize) % 5]).unwrap();
                    byte.write_bytes_aligned(&bytes[..(val as usize) % 5]).unwrap();
                }
                _ => {
                    trace += "align(); ";
                    file.align_to_byte().unwrap();
                    byte.align_to_byte().unwrap();
                }
            }
        }
        drop(file.finish());
        assert_eq!(out, byte.as_slice(), "round {round}: {trace}");
        assert_eq!(out.len(), byte.as_slice().len());
    }
}

#[test]
fn an_ogg_export_decodes_to_the_same_length_rate_and_is_deterministic() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Ogg,
        depth: BitDepth::Float32,
        sample_rate: SR,
        dither: false,
        seed: 9,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let (first, second) = (temp_file("ogg1"), temp_file("ogg2"));
    write_audio_file(&first, &samples, &spec, WriteMode::Replace).unwrap();
    write_audio_file(&second, &samples, &spec, WriteMode::Replace).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
    let decoded = decode_file(&first).unwrap();
    assert_eq!(decoded.sample_rate, SR);
    assert_eq!(decoded.channels, 2);
    // Lossy: the length is the source's (within one encoder block) and the level is close.
    let rms = |values: &[f32]| {
        (values.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / values.len().max(1) as f64).sqrt()
    };
    let source_rms = rms(&samples);
    let decoded_rms = rms(&decoded.samples);
    assert!((source_rms - decoded_rms).abs() < 0.05 * source_rms, "{source_rms} against {decoded_rms}");
    cleanup(&[&first, &second]);
}

#[test]
fn an_mp3_export_decodes_to_the_same_length_rate_is_deterministic_and_carries_the_title() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Mp3,
        depth: BitDepth::Float32,
        sample_rate: SR,
        dither: false,
        seed: 9,
        mp3_rate: Mp3Rate::Cbr(320),
        title: "a project".into(),
    };
    let (first, second) = (temp_file("mp3a"), temp_file("mp3b"));
    write_audio_file(&first, &samples, &spec, WriteMode::Replace).unwrap();
    write_audio_file(&second, &samples, &spec, WriteMode::Replace).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
    // The header and the tag frame sit before the audio; the decoder skips them.
    let bytes = std::fs::read(&first).unwrap();
    assert!(bytes.starts_with(b"ID3"), "the ID3v2 header is not at the start");
    assert!(bytes.windows(4).any(|w| w == b"Info" || w == b"Xing"), "no LAME tag in the file");
    let decoded = decode_file(&first).unwrap();
    assert_eq!(decoded.sample_rate, SR);
    assert_eq!(decoded.channels, 2);
    // Lossy: the length is the source's (within one encoder block) and the level is close.
    let rms = |values: &[f32]| {
        (values.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / values.len().max(1) as f64).sqrt()
    };
    let source_rms = rms(&samples);
    let decoded_rms = rms(&decoded.samples);
    assert!((source_rms - decoded_rms).abs() < 0.05 * source_rms, "{source_rms} against {decoded_rms}");
    cleanup(&[&first, &second]);
}

#[test]
fn hostile_values_reach_an_mp3_as_full_scale_and_silence() {
    // The encoder refuses what is not finite or past full scale; the writer brings it there
    // first, so a hostile mix still writes a whole file.
    let mut samples = sine();
    for (i, hostile) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0, -2.0].into_iter().enumerate() {
        samples[i * 100] = hostile;
    }
    let spec = AudioFileSpec {
        format: AudioFormat::Mp3,
        depth: BitDepth::Float32,
        sample_rate: SR,
        dither: false,
        seed: 0,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let path = temp_file("mp3hostile");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let decoded = decode_file(&path).unwrap();
    assert!(decoded.samples.iter().all(|s| s.is_finite()), "a hostile value reached the file");
    cleanup(&[&path]);
}

#[test]
fn an_mp3_export_refuses_a_rate_its_encoder_does_not_write() {
    let samples = sine();
    let spec = AudioFileSpec {
        format: AudioFormat::Mp3,
        depth: BitDepth::Float32,
        sample_rate: 96_000,
        dither: false,
        seed: 0,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let error = write_audio_file(&temp_file("mp3rate"), &samples, &spec, WriteMode::Replace).unwrap_err();
    assert!(error.contains("96 000") || error.contains("96000"), "{error}");
}

#[test]
fn the_names_and_the_extensions_agree() {
    assert_eq!(AudioFormat::from_extension("wav"), Some(AudioFormat::Wav));
    assert_eq!(AudioFormat::from_extension("FLAC"), Some(AudioFormat::Flac));
    assert_eq!(AudioFormat::from_extension("ogg"), Some(AudioFormat::Ogg));
    assert_eq!(AudioFormat::from_extension("mp3"), Some(AudioFormat::Mp3));
    assert_eq!(AudioFormat::from_extension(AudioFormat::Ogg.name()), Some(AudioFormat::Ogg));
    assert_eq!(AudioFormat::from_extension(AudioFormat::Mp3.name()), Some(AudioFormat::Mp3));
    assert_eq!(Mp3Rate::from_name("320"), Some(Mp3Rate::Cbr(320)));
    assert_eq!(Mp3Rate::from_name("v2"), Some(Mp3Rate::Vbr(2)));
    assert_eq!(Mp3Rate::from_name("9"), None);
    assert_eq!(BitDepth::from_name("32f"), Some(BitDepth::Float32));
    assert_eq!(BitDepth::from_name("24"), Some(BitDepth::Pcm24));
    assert_eq!(BitDepth::from_name("16"), Some(BitDepth::Pcm16));
    assert_eq!(BitDepth::from_name("8"), None);
    assert_eq!(BitDepth::from_name(BitDepth::Pcm24.name()), Some(BitDepth::Pcm24));
}

#[test]
fn hostile_values_cannot_reach_the_file() {
    let samples = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0, -2.0, 0.0];
    let spec = AudioFileSpec {
        format: AudioFormat::Wav,
        depth: BitDepth::Pcm16,
        sample_rate: SR,
        dither: false,
        seed: 1,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let path = temp_file("hostile");
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    let written: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
    // NaN becomes 0; the infinities and the over-range values clamp at full scale.
    assert_eq!(&written[..3], &[0, 32_767, -32_768]);
    assert_eq!(&written[3..], &[32_767, -32_768, 0]);
    cleanup(&[&path]);
}

#[test]
fn refusals_are_said_as_they_are() {
    let samples = sine();
    // A rate the engine does not take.
    let spec = AudioFileSpec {
        format: AudioFormat::Wav,
        depth: BitDepth::Float32,
        sample_rate: 400,
        dither: false,
        seed: 0,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let error = write_audio_file(&temp_file("rate"), &samples, &spec, WriteMode::Replace).unwrap_err();
    assert!(error.contains("sample rate 400 is not supported"), "{error}");
    // A torn frame.
    let spec = AudioFileSpec::float_wav(SR);
    let error = write_audio_file(&temp_file("torn"), &[0.0, 0.1, 0.2], &spec, WriteMode::Replace).unwrap_err();
    assert!(error.contains("stereo frames"), "{error}");
    // FLAC takes no float depth.
    let spec = AudioFileSpec {
        format: AudioFormat::Flac,
        depth: BitDepth::Float32,
        sample_rate: SR,
        dither: false,
        seed: 0,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let error = write_audio_file(&temp_file("deep"), &samples, &spec, WriteMode::Replace).unwrap_err();
    assert!(error.contains("not 32f"), "{error}");
    // A folder that is not there.
    let missing = temp_dir().join(format!("no-such-{}-dir", std::process::id())).join("out.wav");
    let spec = AudioFileSpec::float_wav(SR);
    let error = write_audio_file(&missing, &samples, &spec, WriteMode::Replace).unwrap_err();
    assert!(error.contains(&missing.display().to_string()), "{error}");
}

/// The export's rules hold when the bytes are committed, not only in the check before the
/// render: a destination that appears between the two is never clobbered without `overwrite`,
/// and a symlink is never followed.
#[test]
fn an_export_holds_its_rules_when_the_destination_changes_after_the_check() {
    let dir = temp_dir().join(format!("commit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let spec = AudioFileSpec::float_wav(SR);
    let samples = sine();

    // The destination appears after an export checked it: with overwrite off, the file that
    // appeared is left alone and the export refuses.
    let appeared = dir.join("appeared.wav");
    std::fs::write(&appeared, b"quiet\n").unwrap();
    let error = write_audio_file(&appeared, &samples, &spec, WriteMode::Export { overwrite: false }).unwrap_err();
    assert!(error.contains("already exists; pass overwrite: true"), "{error}");
    assert_eq!(std::fs::read(&appeared).unwrap(), b"quiet\n", "the file that appeared is untouched");

    // With overwrite, the same file is replaced, wholly, with no stage file left behind.
    write_audio_file(&appeared, &samples, &spec, WriteMode::Export { overwrite: true }).unwrap();
    assert_eq!(decode_file(&appeared).unwrap().samples.len(), samples.len());
    assert_eq!(left_overs(&dir), Vec::<String>::new(), "no staged file is left");

    // A symlink is never written through, with overwrite on or off.
    let target = dir.join("target.wav");
    std::fs::write(&target, b"the old file\n").unwrap();
    #[cfg(unix)]
    {
        let link = dir.join("linked.wav");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = write_audio_file(&link, &samples, &spec, WriteMode::Export { overwrite: true }).unwrap_err();
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file\n", "the link's target is untouched");
        let _ = std::fs::remove_file(&link);

        // A replace writes over a symlink as itself: the one rename swaps the link out for
        // the plain file, and what the link pointed at is not written.
        let link = dir.join("linked.wav");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_audio_file(&link, &samples, &spec, WriteMode::Replace).unwrap();
        assert_eq!(decode_file(&link).unwrap().samples.len(), samples.len(), "the link is a plain file now");
        assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file\n", "the link's target is untouched");
        let _ = std::fs::remove_file(&link);
    }

    // A failed write leaves the destination it would have replaced intact: an illegal depth
    // for flac is refused inside the writer, after the stage was begun (the name is taken).
    let flac_32f = AudioFileSpec {
        format: AudioFormat::Flac,
        depth: BitDepth::Float32,
        sample_rate: SR,
        dither: false,
        seed: 0,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    let flac = dir.join("flac.wav");
    std::fs::write(&flac, b"the earlier flac\n").unwrap();
    let error = write_audio_file(&flac, &samples, &flac_32f, WriteMode::Export { overwrite: true }).unwrap_err();
    assert!(error.contains("not 32f"), "{error}");
    assert_eq!(std::fs::read(&flac).unwrap(), b"the earlier flac\n", "the earlier flac is untouched");
    assert_eq!(left_overs(&dir), Vec::<String>::new(), "no staged file is left");

    let _ = std::fs::remove_dir_all(dir);
}

/// The names in `dir`, kept in order, with the test's own files taken out: what remains is
/// what the writer left behind by accident.
fn left_overs(dir: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        // The files the test made itself; every staged file starts with a dot.
        if ["appeared.wav", "target.wav", "flac.wav", "linked.wav"].contains(&name.as_str()) {
            continue;
        }
        names.push(name);
    }
    names.sort();
    names
}

/// The writer's peak memory on a long mix, measured in stages (`memory_stats`'s resident set):
/// the mix alone, then with the encode behind it, then with the decode — via
/// `/usr/bin/time -l cargo test -p suhina-media --lib a_half_hour -- --ignored --nocapture`,
/// whose `maximum resident set size` is the whole run's peak. The mix is half an hour of noise
/// at 48 kHz; the encode takes minutes, so the test runs only when it is asked for.
#[test]
#[ignore = "a measurement, not a check: runs for as long as the encode takes"]
fn a_half_hour_flac_write_keeps_bounded_memory() {
    let rss = || memory_stats::memory_stats().map(|s| s.physical_mem).unwrap_or(0) as f64 / (1024.0 * 1024.0);
    let seconds = 1_800usize;
    let frames = seconds * SR as usize;
    let mut samples = vec![0.0f32; frames * 2];
    // Sorted noise so the frames have content of a real sort to compress.
    let mut state = 0x1234_5678_9abc_def0u64;
    for pair in samples.as_chunks_mut::<2>().0.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let left = (state >> 33) as f32 / 2_147_483_647.0 - 0.5;
        pair[0] = left.clamp(-0.9, 0.9);
        pair[1] = (left * 0.25).clamp(-0.9, 0.9);
    }
    println!("the mix alone: {:.0} MB ({} frames)", rss(), frames);
    let path = temp_file("memory-flac");
    let spec = AudioFileSpec {
        format: AudioFormat::Flac,
        depth: BitDepth::Pcm24,
        sample_rate: SR,
        dither: true,
        seed: 7,
        mp3_rate: Mp3Rate::default(),
        title: String::new(),
    };
    write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
    println!("after the encode: {:.0} MB", rss());
    let decoded = decode_file(&path).unwrap();
    println!("after the decode: {:.0} MB", rss());
    assert_eq!(decoded.samples.len(), frames * 2, "the same length it was given");
    let _ = std::fs::remove_file(&path);
}

/// The production writer over the measurement's correlated kind of noise (left say a value,
/// east a quarter of it) — the shape that once sent flacenc's side-channel codings past the
/// stated depth — grows linearly, and decodes.
#[test]
fn the_real_writer_grows_linearly_over_correlated_noise() {
    let mut state = 0x1234_5678_9abc_def0u64;
    for seconds in [1usize, 30, 120] {
        let frames = seconds * SR as usize;
        let mut samples = vec![0.0f32; frames * 2];
        for pair in samples.as_chunks_mut::<2>().0.iter_mut() {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let left = (state >> 33) as f32 / 2_147_483_647.0 - 0.5;
            pair[0] = left.clamp(-0.9, 0.9);
            pair[1] = (left * 0.25).clamp(-0.9, 0.9);
        }
        let path = temp_file("correlated-size-hunt");
        let spec = AudioFileSpec {
            format: AudioFormat::Flac,
            depth: BitDepth::Pcm24,
            sample_rate: SR,
            dither: true,
            seed: 7,
            mp3_rate: Mp3Rate::default(),
            title: String::new(),
        };
        write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        let blocks = frames / 4096;
        // The mix is correlated, so the file gives a little more than the independent one, but
        // not much more: a frame holds what the samples need, not an order more.
        let per_block = size / blocks as u64;
        assert!(per_block < 40_000, "{seconds} s runs to {size} bytes, {per_block} per block");
        decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn the_real_writer_grows_linearly_over_noise() {
    let mut state = 0x1234_5678_9abc_def0u64;
    for seconds in [1usize, 30, 120] {
        let frames = seconds * SR as usize;
        let samples: Vec<f32> = std::iter::from_fn(move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            Some(((state & 0xFFFFFF) as f32) / 8_388_608.0 - 0.5)
        })
        .take(frames * 2)
        .collect();
        let path = temp_file("size-hunt");
        let spec = AudioFileSpec {
            format: AudioFormat::Flac,
            depth: BitDepth::Pcm24,
            sample_rate: SR,
            dither: true,
            seed: 7,
            mp3_rate: Mp3Rate::default(),
            title: String::new(),
        };
        write_audio_file(&path, &samples, &spec, WriteMode::Replace).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        let blocks = frames / 4096;
        println!("{seconds} s: {blocks} blocks, file {size} bytes ({} per block)", size / blocks as u64);
        decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
