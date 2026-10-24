// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The encoder's tests: encode the materials the export renders and decode the files back
//! with Symphonia, checking length (the encoder's delay and padding ride along) and sample
//! rate, and the settings' rejection paths.

use std::path::{Path, PathBuf};

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{Mp3Encoder, Mp3Error, Mp3Rate, Mp3Settings, Mp3Tags, assemble_mp3_file};

const RATE_44100: u32 = 44_100;

fn temp_file(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("suhina-mp3-{name}.mp3"));
    path
}

fn settings(rate: Mp3Rate) -> Mp3Settings {
    Mp3Settings { rate, sample_rate: RATE_44100, channels: 2, tags: Mp3Tags::default() }
}

/// Decodes a file's samples, its sample rate and channel count.
fn decode(path: &Path) -> (u32, u16, Vec<f32>) {
    let file = std::fs::File::open(path).unwrap();
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .unwrap();
    let track = format.default_track(TrackType::Audio).unwrap();
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).unwrap().clone();
    let sample_rate = params.sample_rate.unwrap();
    let channels = u16::try_from(params.channels.as_ref().map(|c| c.count()).unwrap_or(1)).unwrap();
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).unwrap();
    let mut samples: Vec<f32> = Vec::new();
    let mut packet_samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => continue,
            Err(e) => panic!("read error: {e}"),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                packet_samples.clear();
                decoded.copy_to_vec_interleaved(&mut packet_samples);
                samples.extend_from_slice(&packet_samples);
            }
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => continue,
            Err(e) => panic!("decode error: {e}"),
        }
    }
    (sample_rate, channels, samples)
}

/// Interleaves two channels.
fn interleave(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter().zip(right).flat_map(|(l, r)| [*l, *r]).collect()
}

/// Encodes both channels in blocks, closes the file and writes it to a test's own path.
fn encode_all(mut encoder: Mp3Encoder, left: &[f32], right: &[f32], name: &str) -> PathBuf {
    let mut stream: Vec<u8> = Vec::new();
    for (l, r) in left.chunks(4800).zip(right.chunks(4800)) {
        encoder.encode(l, r, &mut stream).unwrap();
    }
    let tag = encoder.finish(&mut stream).unwrap();
    let file = assemble_mp3_file(&stream, &tag).unwrap();
    let path = temp_file(name);
    std::fs::write(&path, &file).unwrap();
    path
}

fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt()
}

#[test]
fn silence_encodes_and_decodes() {
    let left = vec![0.0f32; RATE_44100 as usize];
    let right = left.clone();
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap(), &left, &right, "silence");
    let (rate, channels, samples) = decode(&path);
    assert_eq!(rate, RATE_44100);
    assert_eq!(channels, 2);
    // The encoder's delay and padding ride along; the length is within a few frames.
    let frames = samples.len() / 2;
    assert!((frames as i64 - RATE_44100 as i64).abs() < 8_000, "decoded {frames} frames for 44100 samples");
    assert!(rms(&samples) < 0.01, "silence decoded to {}", rms(&samples));
}

#[test]
fn sine_encodes_at_the_right_level() {
    // A 220 Hz sine at -6 dBFS, one second: the decode's RMS is within a dB of the source's.
    let left: Vec<f32> = (0..RATE_44100 as usize)
        .map(|i| 0.5 * (core::f32::consts::TAU * 220.0 * i as f32 / RATE_44100 as f32).sin())
        .collect();
    let right = left.clone();
    let source_rms = rms(&interleave(&left, &right));
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap(), &left, &right, "sine");
    let (_, _, samples) = decode(&path);
    let decoded_rms = rms(&samples);
    let db = 20.0 * (decoded_rms / source_rms).log10();
    assert!(db.abs() < 1.5, "sine decoded at {db:.2} dB of the source");
}

#[test]
fn full_scale_clipping_survives() {
    let left = vec![1.0f32; RATE_44100 as usize / 2];
    let right = vec![-1.0f32; RATE_44100 as usize / 2];
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap(), &left, &right, "clip");
    let (_, _, samples) = decode(&path);
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    // MP3's coding floor keeps the peak near full scale, not exactly at it.
    assert!(peak > 0.85, "full scale decoded at {peak}");
    assert!(peak <= 1.5, "full scale decoded at {peak}");
}

#[test]
fn single_sample_encodes() {
    let left = vec![0.5f32];
    let right = vec![-0.5f32];
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap(), &left, &right, "single");
    let (rate, _, samples) = decode(&path);
    assert_eq!(rate, RATE_44100);
    // A single sample is all encoder delay and padding, and still a whole file.
    assert!(!samples.is_empty());
}

#[test]
fn mono_encodes_and_decodes_as_mono() {
    let left: Vec<f32> = (0..RATE_44100 as usize)
        .map(|i| 0.4 * (core::f32::consts::TAU * 440.0 * i as f32 / RATE_44100 as f32).sin())
        .collect();
    // One channel: the right slice is along for the length check and ignored.
    let mut stream: Vec<u8> = Vec::new();
    let mut encoder = Mp3Encoder::new(&Mp3Settings {
        rate: Mp3Rate::Cbr(192),
        sample_rate: RATE_44100,
        channels: 1,
        tags: Mp3Tags { title: "mono".into() },
    })
    .unwrap();
    encoder.encode(&left, &left, &mut stream).unwrap();
    let tag = encoder.finish(&mut stream).unwrap();
    let file = assemble_mp3_file(&stream, &tag).unwrap();
    let path = temp_file("mono");
    std::fs::write(&path, &file).unwrap();
    let (rate, channels, samples) = decode(&path);
    assert_eq!(rate, RATE_44100);
    assert_eq!(channels, 1);
    assert!(rms(&samples) > 0.2, "mono sine decoded to {}", rms(&samples));
}

#[test]
fn vbr_writes_a_whole_file() {
    let left = vec![0.1f32; RATE_44100 as usize];
    let right = vec![0.1f32; RATE_44100 as usize];
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Vbr(2))).unwrap(), &left, &right, "vbr");
    let (_, _, samples) = decode(&path);
    let frames = samples.len() / 2;
    assert!((frames as i64 - RATE_44100 as i64).abs() < 8_000, "VBR decoded {frames} frames");
}

#[test]
fn long_render_encodes() {
    // Three minutes of quiet noise: many frames, the reservoir and the flush.
    let n = RATE_44100 as usize * 180;
    let mut state = 7u64;
    let mut noise = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u32 << 24) as f32 * 0.01 - 0.005
    };
    let left: Vec<f32> = (0..n).map(|_| noise()).collect();
    let right: Vec<f32> = (0..n).map(|_| noise()).collect();
    let path = encode_all(Mp3Encoder::new(&settings(Mp3Rate::Cbr(128))).unwrap(), &left, &right, "long");
    let (rate, _, samples) = decode(&path);
    assert_eq!(rate, RATE_44100);
    let frames = samples.len() / 2;
    assert!((frames as i64 - n as i64).abs() < 8_000, "3 minutes decoded to {frames} frames");
}

#[test]
fn invalid_settings_are_errors() {
    // A sample rate LAME does not encode.
    let bad = Mp3Settings { rate: Mp3Rate::Cbr(320), sample_rate: 96_000, channels: 2, tags: Mp3Tags::default() };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
    // Zero channels, and five.
    let bad = Mp3Settings { rate: Mp3Rate::Cbr(320), sample_rate: RATE_44100, channels: 0, tags: Mp3Tags::default() };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
    let bad = Mp3Settings { rate: Mp3Rate::Cbr(320), sample_rate: RATE_44100, channels: 5, tags: Mp3Tags::default() };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
    // A bitrate past LAME's range, and a VBR quality past 9.
    let bad = Mp3Settings { rate: Mp3Rate::Cbr(321), sample_rate: RATE_44100, channels: 2, tags: Mp3Tags::default() };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
    let bad = Mp3Settings { rate: Mp3Rate::Vbr(10), sample_rate: RATE_44100, channels: 2, tags: Mp3Tags::default() };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
    // A title with a NUL.
    let bad = Mp3Settings {
        rate: Mp3Rate::Cbr(320),
        sample_rate: RATE_44100,
        channels: 2,
        tags: Mp3Tags { title: "a\0b".into() },
    };
    assert!(matches!(Mp3Encoder::new(&bad), Err(Mp3Error::InvalidSettings(_))));
}

#[test]
fn mismatched_blocks_are_errors() {
    let mut encoder = Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap();
    let mut out = Vec::new();
    let error = encoder.encode(&[0.0; 10], &[0.0; 12], &mut out).unwrap_err();
    assert!(matches!(error, Mp3Error::InvalidSettings(_)));
}

#[test]
fn title_is_written_into_the_file() {
    let left = vec![0.0f32; RATE_44100 as usize];
    let right = left.clone();
    let mut stream: Vec<u8> = Vec::new();
    let mut encoder = Mp3Encoder::new(&Mp3Settings {
        rate: Mp3Rate::Cbr(320),
        sample_rate: RATE_44100,
        channels: 2,
        tags: Mp3Tags { title: "a project's name".into() },
    })
    .unwrap();
    encoder.encode(&left, &right, &mut stream).unwrap();
    let tag = encoder.finish(&mut stream).unwrap();
    let file = assemble_mp3_file(&stream, &tag).unwrap();
    // The header is at the start, the tag frame right after it, and the title inside the
    // header. No v1 trailer: the file does not end in "TAG".
    assert!(file.starts_with(b"ID3"));
    assert!(file.windows(utf16_title("a project's name").len()).any(|w| w == utf16_title("a project's name")));
    assert!(!file.ends_with(b"TAG"), "a v1 tag was written under v2 only");
}

/// A `TIT2` text as LAME writes it from UTF-16: encoding 1, the byte order mark, then the
/// text's UTF-16 code units, little end first (LAME writes every UTF-16 text that way).
fn utf16_title(title: &str) -> Vec<u8> {
    let mut bytes = vec![1u8, 0xFF, 0xFE];
    bytes.extend(title.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[test]
fn a_title_beyond_ascii_is_written_as_utf16() {
    let left = vec![0.0f32; RATE_44100 as usize];
    let right = left.clone();
    let mut stream: Vec<u8> = Vec::new();
    let mut encoder = Mp3Encoder::new(&Mp3Settings {
        rate: Mp3Rate::Cbr(320),
        sample_rate: RATE_44100,
        channels: 2,
        tags: Mp3Tags { title: "Café ベース".into() },
    })
    .unwrap();
    encoder.encode(&left, &right, &mut stream).unwrap();
    let tag = encoder.finish(&mut stream).unwrap();
    let file = assemble_mp3_file(&stream, &tag).unwrap();
    let title = utf16_title("Café ベース");
    assert!(file.windows(title.len()).any(|w| w == title.as_slice()), "the title is not in the file as UTF-16");
    // The UTF-8 bytes would be the Latin-1 misreading every player shows.
    assert!(!file.windows("Café".len()).any(|w| w == "Café".as_bytes()));
}

#[test]
fn samples_past_full_scale_or_not_finite_are_refused() {
    for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -1.0001] {
        let mut encoder = Mp3Encoder::new(&settings(Mp3Rate::Cbr(320))).unwrap();
        let mut out = vec![7u8];
        let mut left = vec![0.0f32; 64];
        left[10] = hostile;
        let error = encoder.encode(&left, &[0.0; 64], &mut out).unwrap_err();
        assert!(matches!(error, Mp3Error::InvalidSettings(_)), "{hostile}");
        // Nothing was written past what the caller already had.
        assert_eq!(out, [7]);
    }
}

#[test]
fn assemble_refuses_malformed_streams() {
    let tag = vec![0u8; 1044];
    // Not a stream at all.
    assert!(matches!(assemble_mp3_file(b"nope", &tag), Err(Mp3Error::Assemble(_))));
    // An ID3v2 header whose size runs past the stream.
    let mut stream = b"ID3\x03\x00\x00".to_vec();
    stream.extend_from_slice(&[0x7F, 0x7F, 0x7F, 0x7F]);
    assert!(matches!(assemble_mp3_file(&stream, &tag), Err(Mp3Error::Assemble(_))));
}

#[test]
fn stream_with_id3_and_tag_rounds_through_assemble() {
    // A stream whose first frame sits after the v2 header, spliced with a tag that is
    // recognisable: it must replace the empty frame and leave the rest pass through.
    let mut stream = vec![0u8; 10];
    stream[0..3].copy_from_slice(b"ID3");
    // 1044-byte frame: MPEG 1, Layer III, 320 kbps, 44.1 kHz, no padding.
    let mut frame = vec![0xFF, 0xFB, 0xE0, 0x44];
    frame.resize(1044, 0);
    stream.extend_from_slice(&frame);
    stream.extend_from_slice(&frame);
    let tag = vec![0xAAu8; 1044];
    let file = assemble_mp3_file(&stream, &tag).unwrap();
    assert_eq!(file.len(), stream.len());
    assert!(file.starts_with(b"ID3"));
    assert!(file[10..1054].iter().all(|&b| b == 0xAA), "the tag did not replace the empty frame");
    assert_eq!(&file[1054..1058], &[0xFF, 0xFB, 0xE0, 0x44], "the frames after the tag pass through");
}
