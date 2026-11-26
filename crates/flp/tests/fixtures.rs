// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hand-built byte fixtures for the container, the event walk, the values and every malformed
//! case in `docs/spec/flp-format.md`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use suhina_flp::{
    Event, FileKind, FlpError, MAX_EVENTS, Payload, TextEncoding, Value, ValueKind, ids, kind_of, read_bytes, read_file,
};

/// A whole file: the header with these fields, then `data` as the data chunk.
fn file_with(kind: u16, channels: u16, ppq: u16, data: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(b"FLhd");
    f.extend_from_slice(&6u32.to_le_bytes());
    f.extend_from_slice(&kind.to_le_bytes());
    f.extend_from_slice(&channels.to_le_bytes());
    f.extend_from_slice(&ppq.to_le_bytes());
    f.extend_from_slice(b"FLdt");
    f.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    f.extend_from_slice(data);
    f
}

fn file(data: &[u8]) -> Vec<u8> {
    file_with(0, 1, 96, data)
}

/// A text event: the id, a one-byte length, the bytes.
fn text_event(id: u8, bytes: &[u8]) -> Vec<u8> {
    let mut e = vec![id, u8::try_from(bytes.len()).unwrap()];
    e.extend_from_slice(bytes);
    e
}

fn ascii(s: &str) -> Vec<u8> {
    let mut b = s.as_bytes().to_vec();
    b.push(0);
    b
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

fn version(v: &str) -> Vec<u8> {
    text_event(ids::VERSION, &ascii(v))
}

/// The eleven bytes every corpus file (FL Studio 26.1) opens with.
const REAL_OPENING: [u8; 11] = [0xa9, 0x0f, 0x00, 0x00, 0x00, 0x1c, 0x01, 0xac, 0x01, 0x01, 0x00];

// ---- the container ----

#[test]
fn reads_the_header_and_an_empty_data_chunk() {
    let flp = read_bytes(&file_with(0, 3, 96, &[])).unwrap();
    assert_eq!(flp.header.kind, FileKind::Project);
    assert_eq!((flp.header.channels, flp.header.ppq), (3, 96));
    assert_eq!(flp.data_len, 0);
    assert!(flp.events.is_empty());
}

#[test]
fn keeps_an_unknown_file_kind_as_a_number() {
    let flp = read_bytes(&file_with(0x10, 0, 960, &[])).unwrap();
    assert_eq!(flp.header.kind, FileKind::Other(0x10));
}

#[test]
fn rejects_a_bad_header_magic() {
    let mut f = file(&[]);
    f[0..4].copy_from_slice(b"RIFF");
    assert!(matches!(read_bytes(&f), Err(FlpError::BadMagic { offset: 0, expected }) if &expected == b"FLhd"));
}

#[test]
fn rejects_a_bad_data_magic() {
    let mut f = file(&[]);
    f[14..18].copy_from_slice(b"FLdx");
    assert!(matches!(read_bytes(&f), Err(FlpError::BadMagic { offset: 14, expected }) if &expected == b"FLdt"));
}

#[test]
fn rejects_a_truncated_header_at_every_length() {
    let f = file(&[]);
    for len in 0..22 {
        match read_bytes(&f[..len]) {
            Err(FlpError::TruncatedHeader { offset }) => assert_eq!(offset, len),
            other => panic!("{len} bytes: {other:?}"),
        }
    }
}

#[test]
fn rejects_a_header_length_other_than_6() {
    let mut f = file(&[]);
    f[4..8].copy_from_slice(&8u32.to_le_bytes());
    assert!(matches!(read_bytes(&f), Err(FlpError::UnsupportedHeader { offset: 4, len: 8 })));
}

#[test]
fn rejects_data_that_does_not_fill_the_chunk() {
    let mut f = file(&[0x00, 0x01, 0x00, 0x01]);
    f.truncate(f.len() - 1);
    assert!(matches!(read_bytes(&f), Err(FlpError::SizeMismatch { expected: 26, actual: 25 })));
}

#[test]
fn rejects_bytes_after_the_data_chunk() {
    let mut f = file(&[0x00, 0x01]);
    f.push(0x00);
    assert!(matches!(read_bytes(&f), Err(FlpError::SizeMismatch { expected: 24, actual: 25 })));
}

#[test]
fn reads_a_file_from_disk() {
    let dir = std::env::temp_dir().join(format!("suhina-flp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("one.flp");
    std::fs::write(&path, file(&[0x00, 0x01])).unwrap();
    let flp = read_file(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(flp.events.len(), 1);
    assert!(matches!(read_file(&dir.join("missing.flp")), Err(FlpError::Io(_))));
}

// ---- the event walk ----

#[test]
fn walks_the_four_id_ranges() {
    let mut data = vec![
        0x05, 0x7f, // id 5, one byte
        0x40, 0x34, 0x12, // id 64, two bytes
        0x80, 0x78, 0x56, 0x34, 0x12, // id 128, four bytes
    ];
    data.extend(text_event(0xd0, &[1, 2, 3])); // id 208, a length and three bytes
    let flp = read_bytes(&file(&data)).unwrap();
    let got: Vec<(u8, usize, Payload)> = flp.events.into_iter().map(|e| (e.id, e.offset, e.payload)).collect();
    assert_eq!(
        got,
        vec![
            (5, 22, Payload::Byte(0x7f)),
            (64, 24, Payload::Word(0x1234)),
            (128, 27, Payload::Dword(0x1234_5678)),
            (208, 32, Payload::Data(vec![1, 2, 3])),
        ]
    );
}

#[test]
fn walks_the_range_edges() {
    let data = [63, 1, 127, 1, 0, 191, 1, 0, 0, 0, 255, 0];
    let flp = read_bytes(&file(&data)).unwrap();
    let sizes: Vec<(u8, usize)> = flp.events.iter().map(|e| (e.id, e.payload.len())).collect();
    assert_eq!(sizes, vec![(63, 1), (127, 2), (191, 4), (255, 0)]);
}

#[test]
fn reads_a_multi_byte_length() {
    // 300 = 0b10_0101100: low group 0x2c with the continuation bit, then 0x02.
    let mut data = vec![0xd5, 0xac, 0x02];
    data.extend(std::iter::repeat_n(7u8, 300));
    data.extend([0x00, 0x01]);
    let flp = read_bytes(&file(&data)).unwrap();
    assert_eq!(flp.events[0].payload.len(), 300);
    assert_eq!((flp.events[1].id, flp.events[1].offset), (0, 22 + 3 + 300));
}

#[test]
fn id_172_has_a_one_byte_value() {
    let mut data = REAL_OPENING.to_vec();
    data.extend(version("26.1.0.5530"));
    let flp = read_bytes(&file(&data)).unwrap();
    let got: Vec<(u8, usize, Payload)> =
        flp.events.iter().take(4).map(|e| (e.id, e.offset, e.payload.clone())).collect();
    assert_eq!(
        got,
        vec![
            (169, 22, Payload::Dword(15)),
            (28, 27, Payload::Byte(1)),
            (172, 29, Payload::Byte(1)),
            (1, 31, Payload::Byte(0)),
        ]
    );
    assert_eq!(flp.version(), Some("26.1.0.5530"));
    assert_eq!(flp.events.len(), 5);
}

#[test]
fn keeps_unknown_ids_without_an_error() {
    let mut data = vec![0x3e, 9, 0x7e, 1, 2, 0xbe, 1, 2, 3, 4];
    data.extend(text_event(0xfe, &[0xff; 4]));
    let flp = read_bytes(&file(&data)).unwrap();
    let values: Vec<Value> = flp.events.iter().map(|e| flp.value(e).unwrap()).collect();
    assert_eq!(
        values,
        vec![Value::Unsigned(9), Value::Unsigned(0x0201), Value::Unsigned(0x0403_0201), Value::Binary(vec![0xff; 4]),]
    );
}

#[test]
fn rejects_a_truncated_fixed_value() {
    for (data, id) in [(vec![0x00], 0u8), (vec![0x00, 0x01, 0x40, 0x01], 64), (vec![0x80, 1, 2, 3], 128)] {
        let offset = 22 + data.len()
            - if id == 0 {
                1
            } else if id == 64 {
                2
            } else {
                4
            };
        match read_bytes(&file(&data)) {
            Err(FlpError::TruncatedEvent { offset: o, id: i }) => assert_eq!((o, i), (offset, id)),
            other => panic!("{data:?}: {other:?}"),
        }
    }
}

#[test]
fn rejects_a_text_value_longer_than_the_chunk() {
    let data = [0x00, 0x01, 0xc4, 0x05, b'a', b'b'];
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::TruncatedEvent { offset: 24, id: 0xc4 })));
}

#[test]
fn rejects_a_length_that_never_terminates() {
    let data = [0xc4, 0x80, 0x80];
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::UnterminatedLength { offset: 23 })));
    let data = [0xc4];
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::UnterminatedLength { offset: 23 })));
}

#[test]
fn rejects_a_length_that_is_too_long_or_too_large() {
    let six_bytes = [0xc4, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00];
    assert!(matches!(read_bytes(&file(&six_bytes)), Err(FlpError::BadLength { offset: 23 })));
    // Five bytes, but 2^32: one past what 32 bits hold.
    let over_32_bits = [0xc4, 0x80, 0x80, 0x80, 0x80, 0x10];
    assert!(matches!(read_bytes(&file(&over_32_bits)), Err(FlpError::BadLength { offset: 23 })));
}

#[test]
fn a_huge_declared_length_allocates_nothing() {
    // u32::MAX bytes declared, none present: refused before any allocation of that size.
    let data = [0xc4, 0xff, 0xff, 0xff, 0xff, 0x0f];
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::TruncatedEvent { offset: 22, id: 0xc4 })));
}

#[test]
fn caps_the_number_of_events() {
    let data = vec![0u8; (MAX_EVENTS + 1) * 2];
    assert!(matches!(
        read_bytes(&file(&data)),
        Err(FlpError::TooManyEvents { offset }) if offset == 22 + MAX_EVENTS * 2
    ));
}

// ---- values ----

fn event(id: u8, payload: Payload) -> Event {
    Event { id, offset: 100, payload }
}

#[test]
fn decodes_every_value_kind() {
    let cases = [
        (event(0, Payload::Byte(0)), ValueKind::Bool, Value::Bool(false)),
        (event(0, Payload::Byte(2)), ValueKind::Bool, Value::Bool(true)),
        (event(1, Payload::Byte(0xff)), ValueKind::Unsigned, Value::Unsigned(255)),
        (event(1, Payload::Byte(0xff)), ValueKind::Signed, Value::Signed(-1)),
        (event(64, Payload::Word(0xfffe)), ValueKind::Signed, Value::Signed(-2)),
        (event(128, Payload::Dword(0xffff_fffd)), ValueKind::Signed, Value::Signed(-3)),
        (event(128, Payload::Dword(1.5f32.to_bits())), ValueKind::F32, Value::F32(1.5)),
        (event(128, Payload::Dword(0x0002_0001)), ValueKind::TwoU16, Value::TwoU16(1, 2)),
        (event(128, Payload::Dword(0xff33_2211)), ValueKind::Color, Value::Color(0x11, 0x22, 0x33)),
        (event(208, Payload::Data(vec![1, 2])), ValueKind::Binary, Value::Binary(vec![1, 2])),
        (event(1, Payload::Byte(7)), ValueKind::Binary, Value::Binary(vec![7])),
        (event(199, Payload::Data(ascii("12.0"))), ValueKind::AsciiString, Value::String("12.0".into())),
    ];
    for (e, kind, want) in cases {
        assert_eq!(e.decode(kind).unwrap(), want, "{e:?} as {kind:?}");
    }
}

#[test]
fn rejects_a_value_of_the_wrong_size() {
    let cases = [
        (event(64, Payload::Word(1)), ValueKind::Bool, 1),
        (event(1, Payload::Byte(1)), ValueKind::F32, 4),
        (event(64, Payload::Word(1)), ValueKind::TwoU16, 4),
        (event(1, Payload::Byte(1)), ValueKind::Color, 4),
        (event(208, Payload::Data(vec![1; 4])), ValueKind::Unsigned, 4),
    ];
    for (e, kind, expected) in cases {
        match e.decode(kind) {
            Err(FlpError::WrongSize { offset: 100, id, expected: x, .. }) => assert_eq!((id, x), (e.id, expected)),
            other => panic!("{e:?} as {kind:?}: {other:?}"),
        }
    }
}

#[test]
fn knows_the_kinds_the_spec_names() {
    assert_eq!(kind_of(ids::CHANNEL_ENABLED), ValueKind::Bool);
    assert_eq!(kind_of(ids::COLOR), ValueKind::Color);
    assert_eq!(kind_of(ids::TEMPO), ValueKind::Unsigned);
    assert_eq!(kind_of(ids::VERSION), ValueKind::AsciiString);
    for id in [ids::NAME, ids::URL, ids::LICENSEE, ids::AUTHOR] {
        assert_eq!(kind_of(id), ValueKind::String);
    }
    assert_eq!(kind_of(198), ValueKind::Binary);
    assert_eq!(kind_of(208), ValueKind::Binary);
}

#[test]
fn tempo_is_in_thousandths() {
    let mut data = version("26.1.0.5530");
    data.extend([ids::TEMPO, 0xe0, 0x22, 0x02, 0x00]); // 140000
    let flp = read_bytes(&file(&data)).unwrap();
    let tempo = flp.first(ids::TEMPO).unwrap();
    assert_eq!(flp.value(tempo).unwrap(), Value::Unsigned(140_000));
}

// ---- strings ----

#[test]
fn strings_are_utf16_from_11_5_on() {
    for v in ["11.5", "11.5.0.1", "12.0", "20.8.4.2576", "26.1.0.5530"] {
        let mut data = version(v);
        data.extend(text_event(ids::TITLE, &utf16("Päivä 1")));
        let flp = read_bytes(&file(&data)).unwrap();
        assert_eq!(flp.encoding().unwrap(), TextEncoding::Utf16Le, "{v}");
        assert_eq!(flp.value(flp.first(ids::TITLE).unwrap()).unwrap(), Value::String("Päivä 1".into()));
    }
}

#[test]
fn strings_are_ascii_before_11_5() {
    for v in ["9.0", "11.4.9", "11"] {
        let mut data = version(v);
        data.extend(text_event(ids::TITLE, &ascii("Song")));
        let flp = read_bytes(&file(&data)).unwrap();
        assert_eq!(flp.encoding().unwrap(), TextEncoding::Ascii, "{v}");
        assert_eq!(flp.value(flp.first(ids::TITLE).unwrap()).unwrap(), Value::String("Song".into()));
    }
}

#[test]
fn the_version_is_ascii_even_in_a_utf16_file() {
    let mut data = text_event(ids::TITLE, &utf16("x"));
    data.extend(version("21.0.3"));
    let flp = read_bytes(&file(&data)).unwrap();
    assert_eq!(flp.version(), Some("21.0.3"));
    let v = flp.first(ids::VERSION).unwrap();
    assert_eq!(flp.value(v).unwrap(), Value::String("21.0.3".into()));
}

#[test]
fn the_first_version_event_decides() {
    let mut data = version("10.0");
    data.extend(version("20.0"));
    assert_eq!(read_bytes(&file(&data)).unwrap().encoding().unwrap(), TextEncoding::Ascii);
}

#[test]
fn a_file_without_a_version_has_no_readable_strings() {
    let data = text_event(ids::TITLE, &utf16("x"));
    let flp = read_bytes(&file(&data)).unwrap();
    assert_eq!(flp.version(), None);
    assert!(matches!(flp.encoding(), Err(FlpError::NoVersion)));
    assert!(matches!(flp.value(&flp.events[0]), Err(FlpError::NoVersion)));
}

#[test]
fn rejects_a_bad_version() {
    for v in ["", "abc", "26..1", "26.1a", ".5", "99999999999.0"] {
        let data = version(v);
        assert!(matches!(read_bytes(&file(&data)), Err(FlpError::BadVersion { offset: 22 })), "{v:?}");
    }
}

#[test]
fn rejects_a_version_without_its_terminator() {
    let data = text_event(ids::VERSION, b"26.1");
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::BadString { offset: 22, .. })));
    let data = text_event(ids::VERSION, b"");
    assert!(matches!(read_bytes(&file(&data)), Err(FlpError::BadString { offset: 22, .. })));
}

#[test]
fn rejects_strings_without_a_terminator() {
    let utf16_cases = [utf16("ab")[..4].to_vec(), Vec::new(), vec![0x61]];
    for bytes in utf16_cases {
        let e = event(ids::TITLE, Payload::Data(bytes.clone()));
        assert!(matches!(e.text(TextEncoding::Utf16Le), Err(FlpError::BadString { offset: 100, .. })), "{bytes:?}");
    }
    let e = event(ids::TITLE, Payload::Data(b"ab".to_vec()));
    assert!(matches!(e.text(TextEncoding::Ascii), Err(FlpError::BadString { offset: 100, .. })));
}

#[test]
fn rejects_strings_that_are_not_valid_in_their_encoding() {
    // A lone high surrogate, then the terminator.
    let e = event(ids::TITLE, Payload::Data(vec![0x00, 0xd8, 0x00, 0x00]));
    assert!(matches!(e.text(TextEncoding::Utf16Le), Err(FlpError::BadString { offset: 100, .. })));
    let e = event(ids::TITLE, Payload::Data(vec![b'a', 0xe4, 0x00]));
    assert!(matches!(e.text(TextEncoding::Ascii), Err(FlpError::BadString { offset: 100, .. })));
    let e = event(1, Payload::Byte(0));
    assert!(matches!(e.text(TextEncoding::Ascii), Err(FlpError::BadString { offset: 100, .. })));
    assert!(matches!(
        event(ids::TITLE, Payload::Data(utf16("a"))).decode(ValueKind::String),
        Err(FlpError::BadString { .. })
    ));
}

#[test]
fn the_real_opening_then_strings_and_tempo() {
    let mut data = REAL_OPENING.to_vec();
    data.extend(version("26.1.0.5530"));
    data.extend(text_event(ids::NAME, &utf16("Sampler")));
    data.extend([ids::TEMPO, 0x00, 0xf4, 0x01, 0x00]); // 128000
    let flp = read_bytes(&file(&data)).unwrap();
    assert_eq!(flp.value(flp.first(ids::NAME).unwrap()).unwrap(), Value::String("Sampler".into()));
    assert_eq!(flp.value(flp.first(ids::TEMPO).unwrap()).unwrap(), Value::Unsigned(128_000));
    assert_eq!(flp.value(flp.first(ids::LICENSED).unwrap()).unwrap(), Value::Bool(true));
}

#[test]
fn errors_say_where() {
    let e = read_bytes(&file(&[0xc4, 0x80])).unwrap_err();
    assert_eq!(e.to_string(), "the length at offset 23 never ends");
}
