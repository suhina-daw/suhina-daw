// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn a_chunk_becomes_a_chunk_bank_and_back() {
    let preset = Preset {
        kind: CHUNK_BANK,
        id: 0x5465_7374,
        version: 3,
        programs: 0,
        current: 0,
        parameters: 9,
        little_endian: true,
    };
    let out = bank(&preset, &[1, 2, 3]).unwrap();
    assert_eq!(&out[..4], b"CcnK");
    assert_eq!(word(&out, 4) as usize, out.len() - 8);
    assert_eq!(&out[8..12], b"FBCh");
    assert_eq!((word(&out, 12), word(&out, 16), word(&out, 20), word(&out, 24)), (2, 0x5465_7374, 3, 1));
    assert!(out[32..156].iter().all(|b| *b == 0));
    assert_eq!(word(&out, 156), 3);
    assert_eq!(&out[160..], [1, 2, 3]);
    // And the chunk a host hands a plug-in is the same bytes back.
    assert_eq!(chunk(&out), Some(&[1u8, 2, 3][..]));
    assert_eq!(program(&out), None);
}

#[test]
fn programs_become_a_program_bank_and_the_current_one_reads_back() {
    let preset =
        Preset { kind: PROGRAM_BANK, id: 7, version: 1, programs: 2, current: 1, parameters: 2, little_endian: true };
    let mut buffer = Vec::new();
    for (name, values) in [("One", [0.25f32, 1.0]), ("Two", [0.5, 0.0])] {
        let mut field = [0u8; 28];
        field[..name.len()].copy_from_slice(name.as_bytes());
        buffer.extend_from_slice(&field);
        values.iter().for_each(|v| buffer.extend_from_slice(&v.to_le_bytes()));
    }
    let out = bank(&preset, &buffer).unwrap();
    assert_eq!(&out[8..12], b"FxBk");
    assert_eq!((word(&out, 24), word(&out, 28)), (2, 1));
    // The first program after the bank header: CcnK, size, FxCk, 1, id, version, count, name.
    let first = &out[156..];
    assert_eq!(&first[..4], b"CcnK");
    // The current program reads back with its number.
    assert_eq!(program(&out).map(|(current, values)| (current, values.len())), Some((1, 2)));
    let (_, values) = program(&out).unwrap();
    assert_eq!(values, vec![0.5, 0.0]);
    assert_eq!(program_values(&out), Some(vec![0.5, 0.0]));
    assert_eq!(chunk(&out), None);
}

/// A bank whose current program is past its programs falls back to the first.
#[test]
fn a_current_program_past_the_programs_reads_the_first() {
    let preset =
        Preset { kind: PROGRAM_BANK, id: 7, version: 1, programs: 1, current: 5, parameters: 1, little_endian: true };
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&[0u8; 28]);
    buffer.extend_from_slice(&0.75f32.to_le_bytes());
    let out = bank(&preset, &buffer).unwrap();
    assert_eq!(program_values(&out), Some(vec![0.75]));
}

/// An `.fxp` file: `CcnK`, the size, its kind, version 1, id 7, version 1, `count`, the name,
/// then `rest`.
fn fxp(kind: &[u8; 4], count: u32, rest: &[u8]) -> Vec<u8> {
    let mut out = b"CcnK".to_vec();
    out.extend_from_slice(&u32::try_from(4 * 5 + NAME + rest.len()).unwrap().to_be_bytes());
    out.extend_from_slice(kind);
    for value in [1, 7, 1, count] {
        out.extend_from_slice(&u32::to_be_bytes(value));
    }
    out.extend_from_slice(&[0; NAME]);
    out.extend_from_slice(rest);
    out
}

/// A single program reads as its values (`FxCk`) or its chunk (`FPCh`); a bank is not one,
/// and a program cut short is nothing (#1167).
#[test]
fn single_programs_read_as_their_values_or_their_chunk() {
    let values = fxp(b"FxCk", 2, &[0.5f32.to_be_bytes(), 0.25f32.to_be_bytes()].concat());
    assert_eq!(single_program(&values), Some(Program::Values(vec![0.5, 0.25])));
    assert_eq!(program_values(&values), Some(vec![0.5, 0.25]), "set as parameters, as a bank's program");
    let chunk = fxp(b"FPCh", 1, &[&4u32.to_be_bytes()[..], b"gain"].concat());
    assert_eq!(single_program(&chunk), Some(Program::Chunk(b"gain")));
    assert_eq!(program_values(&chunk), None);
    assert!(single_program(&chunk[..chunk.len() - 1]).is_none());
    assert!(single_program(&values[..values.len() - 1]).is_none());
    assert!(
        single_program(
            &bank(
                &Preset {
                    kind: CHUNK_BANK,
                    id: 7,
                    version: 1,
                    programs: 1,
                    current: 0,
                    parameters: 0,
                    little_endian: false
                },
                b"gain"
            )
            .unwrap()
        )
        .is_none()
    );
    assert!(program_shaped(&chunk[..20]), "a cut-short program still has its shape");
    assert!(!program_shaped(b"gain"));
}

/// A program of values that declares more than the most parameters, or more values than its
/// declared size covers, is not read, even when the bytes follow.
#[test]
fn a_program_of_values_that_lies_about_its_size_is_not_read() {
    let program = |count: u32, declared: u32, values: usize| {
        let mut out = b"CcnK".to_vec();
        out.extend_from_slice(&declared.to_be_bytes());
        out.extend_from_slice(b"FxCk");
        for word in [1, 9, 1, count] {
            out.extend_from_slice(&u32::to_be_bytes(word));
        }
        out.extend_from_slice(&[0; 28]);
        out.extend(std::iter::repeat_n(0u8, values * 4));
        out
    };
    let honest = |count: u32| 48 + 4 * count;
    assert_eq!(single_program(&program(2, honest(2), 2)), Some(Program::Values(vec![0.0, 0.0])));
    assert_eq!(program_header(&program(2, honest(2), 2)), Some((9, 2)));
    // Past the most, though all its bytes follow.
    let over = u32::try_from(MAX_COUNT).unwrap() + 1;
    assert_eq!(single_program(&program(over, honest(over), MAX_COUNT + 1)), None);
    // A declared size that holds fewer values than the count.
    assert_eq!(single_program(&program(2, honest(1), 2)), None);
}
