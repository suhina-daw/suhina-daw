// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The standard VST2 bank (an `.fxb` file's contents): the form the model keeps for a VST2
//! plug-in's state, the form a VST3 version of the plug-in reads when it stands in for the
//! VST2 one (ADR 0006, decision 8), and the form the VST2 host saves and loads (ADR 0022,
//! `docs/spec/plugin-vst2.md` › state). The layout is the VST3 SDK's `vst2persistence.h`
//! (MIT) as the Live Set importer reads it.
//!
//! The bank is big-endian: `CcnK`, the size of the rest, `FBCh` or `FxBk`, the bank version
//! (2), the plug-in's id and version, the number of programs, the current program, 124 zero
//! bytes; then the chunk (its size, its bytes) or each program (`CcnK`, the size of the rest,
//! `FxCk`, version 1, id, plug-in version, number of parameters, the 28-byte name, the
//! parameters).
//!
//! A bank of programs (`FxBk`) holds parameter values rather than the plug-in's own chunk; not
//! every VST3 version of a plug-in reads such a bank in `setState`, so the host also sets the
//! current program's values as parameters.

/// `FBCh`: a bank that holds the plug-in's own chunk.
pub const CHUNK_BANK: u32 = u32::from_be_bytes(*b"FBCh");
/// `FxBk`: a bank of programs, each a list of parameter values.
pub const PROGRAM_BANK: u32 = u32::from_be_bytes(*b"FxBk");
/// Length of a program's name.
const NAME: usize = 28;
/// Most programs or parameters a bank may say it has.
const MAX_COUNT: usize = 1 << 16;
/// Size of a bank's header, before its programs or chunk.
const BANK_HEADER: usize = 156;
/// Offset of a program's parameter count, and of its values.
const PROGRAM_COUNT: usize = 24;
const PROGRAM_VALUES: usize = 56;

/// What a saved VST2 state says about itself besides its bytes: the kind of bank it is, the
/// plug-in it belongs to and how its programs are laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// [`CHUNK_BANK`] or [`PROGRAM_BANK`].
    pub kind: u32,
    pub id: u32,
    pub version: i32,
    pub programs: usize,
    pub current: i32,
    pub parameters: usize,
    /// Whether parameter values are stored little-endian.
    pub little_endian: bool,
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(field.try_into().ok()?))
}

fn push(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Writes `CcnK` and a size to fill in later; returns where the size goes.
fn open(out: &mut Vec<u8>) -> usize {
    out.extend_from_slice(b"CcnK");
    out.extend_from_slice(&[0; 4]);
    out.len()
}

/// Fills in the size written at `at`: the bytes after it.
fn close(out: &mut [u8], at: usize) -> Result<(), String> {
    let size = u32::try_from(out.len() - at).map_err(|_| "the state is too large")?;
    let slot = out.get_mut(at - 4..at).ok_or("bank layout")?;
    slot.copy_from_slice(&size.to_be_bytes());
    Ok(())
}

/// Writes the standard VST2 bank for `buffer`, as `preset` describes it: the form a project
/// saves and a VST3 replacement reads. For a chunk bank the buffer is the chunk's bytes; for
/// a program bank it is each program's 28-byte name then every parameter as a 32-bit float,
/// in the byte order [`Preset::little_endian`] names.
pub fn bank(preset: &Preset, buffer: &[u8]) -> Result<Vec<u8>, String> {
    if preset.programs > MAX_COUNT || preset.parameters > MAX_COUNT {
        return Err("the preset says it has too many programs or parameters".into());
    }
    let mut out = Vec::with_capacity(buffer.len() + 160 + preset.programs * 48);
    let bank = open(&mut out);
    push(&mut out, preset.kind);
    push(&mut out, 2);
    push(&mut out, preset.id);
    push(&mut out, preset.version as u32);
    match preset.kind {
        CHUNK_BANK => {
            push(&mut out, preset.programs.max(1) as u32);
            push(&mut out, preset.current.max(0) as u32);
            out.extend_from_slice(&[0; 124]);
            push(&mut out, u32::try_from(buffer.len()).map_err(|_| "the state is too large")?);
            out.extend_from_slice(buffer);
        }
        PROGRAM_BANK => {
            let program = NAME + 4 * preset.parameters;
            if preset.programs == 0 || buffer.len() != preset.programs * program {
                return Err(format!(
                    "the state holds {} bytes, not {} programs of {} parameters",
                    buffer.len(),
                    preset.programs,
                    preset.parameters
                ));
            }
            push(&mut out, preset.programs as u32);
            push(&mut out, preset.current.max(0) as u32);
            out.extend_from_slice(&[0; 124]);
            for stored in buffer.chunks_exact(program) {
                let (name, values) = stored.split_at(NAME);
                let at = open(&mut out);
                out.extend_from_slice(b"FxCk");
                push(&mut out, 1);
                push(&mut out, preset.id);
                push(&mut out, preset.version as u32);
                push(&mut out, preset.parameters as u32);
                out.extend_from_slice(name);
                for value in values.as_chunks::<4>().0 {
                    let bits =
                        if preset.little_endian { u32::from_le_bytes(*value) } else { u32::from_be_bytes(*value) };
                    push(&mut out, bits);
                }
                close(&mut out, at)?;
            }
        }
        other => return Err(format!("unknown kind of VST2 state {other}")),
    }
    close(&mut out, bank)?;
    Ok(out)
}

/// The plug-in's own chunk a chunk bank (`CcnK` … `FBCh`) holds, as it gave it; `None` for a
/// program bank or anything else.
pub fn chunk(bank: &[u8]) -> Option<&[u8]> {
    if bank.get(..4)? != b"CcnK" || bank.get(8..12)? != b"FBCh" {
        return None;
    }
    let size = word(bank, 156)? as usize;
    bank.get(160..160usize.checked_add(size)?)
}

/// The current program of a bank of programs (`CcnK` … `FxBk`): its number and its parameter
/// values, in parameter order, big-endian as the bank holds them; `None` for a chunk bank or
/// anything else.
pub fn program(bank: &[u8]) -> Option<(u32, Vec<f32>)> {
    if bank.get(..4)? != b"CcnK" || bank.get(8..12)? != b"FxBk" {
        return None;
    }
    let programs = word(bank, 24)? as usize;
    let current = word(bank, 28)? as usize;
    let wanted = if current < programs { current } else { 0 };
    let mut at = BANK_HEADER;
    for index in 0..programs {
        if bank.get(at..at + 4)? != b"CcnK" {
            return None;
        }
        let size = word(bank, at + 4)? as usize;
        if index == wanted {
            let program = bank.get(at..at.checked_add(8)?.checked_add(size)?)?;
            if program.get(8..12)? != b"FxCk" {
                return None;
            }
            let count = (word(program, PROGRAM_COUNT)? as usize).min(MAX_COUNT);
            let values = program.get(PROGRAM_VALUES..PROGRAM_VALUES.checked_add(count.checked_mul(4)?)?)?;
            return Some((
                wanted as u32,
                values.as_chunks::<4>().0.iter().map(|v| f32::from_bits(u32::from_be_bytes(*v))).collect(),
            ));
        }
        at = at.checked_add(8)?.checked_add(size)?;
    }
    None
}

/// The current program's parameter values, in parameter order, when `bank` is a bank of
/// programs; `None` for a chunk bank or anything else. See [`program`].
pub fn program_values(bank: &[u8]) -> Option<Vec<f32>> {
    program(bank).map(|(_, values)| values)
}

#[cfg(test)]
mod tests {
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
        let preset = Preset {
            kind: PROGRAM_BANK,
            id: 7,
            version: 1,
            programs: 2,
            current: 1,
            parameters: 2,
            little_endian: true,
        };
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
        let preset = Preset {
            kind: PROGRAM_BANK,
            id: 7,
            version: 1,
            programs: 1,
            current: 5,
            parameters: 1,
            little_endian: true,
        };
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&[0u8; 28]);
        buffer.extend_from_slice(&0.75f32.to_le_bytes());
        let out = bank(&preset, &buffer).unwrap();
        assert_eq!(program_values(&out), Some(vec![0.75]));
    }
}
