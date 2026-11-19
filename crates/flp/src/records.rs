// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 The Suhina contributors

//! Adapted from Lemon Studio os-flp src/project.rs and project/legacy.rs at
//! 011a03f23d9db7f10f07cd7d165cc978a9dda263 (<https://github.com/ItsAlphaNeon/Lemon-Studio>).
//! Changes: checked reads, explicit errors and Suhina model values; no writer.
//! Layouts and scope: docs/spec/flp-model-mapping.md.

use crate::{Event, FlpError, Payload};

pub(crate) fn error(offset: usize, reason: impl Into<String>) -> FlpError {
    FlpError::Import { offset, reason: reason.into() }
}

pub(crate) fn number(e: &Event) -> Result<u32, FlpError> {
    match e.payload {
        Payload::Byte(v) => Ok(u32::from(v)),
        Payload::Word(v) => Ok(u32::from(v)),
        Payload::Dword(v) => Ok(v),
        _ => Err(error(e.offset, "expected an integer event")),
    }
}

pub(crate) fn data(e: &Event) -> Result<&[u8], FlpError> {
    match &e.payload {
        Payload::Data(v) => Ok(v),
        _ => Err(error(e.offset, "expected a binary event")),
    }
}

pub(crate) fn word(d: &[u8], at: usize, offset: usize) -> Result<u16, FlpError> {
    let b = d
        .get(at..at.saturating_add(2))
        .and_then(|s| <[u8; 2]>::try_from(s).ok())
        .ok_or_else(|| error(offset, "truncated record"))?;
    Ok(u16::from_le_bytes(b))
}

pub(crate) fn dword(d: &[u8], at: usize, offset: usize) -> Result<u32, FlpError> {
    let b = d
        .get(at..at.saturating_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .ok_or_else(|| error(offset, "truncated record"))?;
    Ok(u32::from_le_bytes(b))
}

pub(crate) struct PatternNote {
    pub channel: u16,
    pub note: suhina_model::Note,
    pub expression: bool,
}

/// One 24-byte note record; `None` for a step-sequencer note (its length all ones: no note-off)
/// or a zero-length one, which stay in the retained source without playing.
pub(crate) fn note(r: &[u8], ppq: f64, offset: usize) -> Result<Option<PatternNote>, FlpError> {
    let key = word(r, 12, offset)?;
    if key > 127 {
        return Err(error(offset, "note key exceeds 127"));
    }
    let velocity = *r.get(21).ok_or_else(|| error(offset, "truncated note"))?;
    if velocity > 128 {
        return Err(error(offset, "note velocity exceeds 128"));
    }
    let duration = dword(r, 8, offset)?;
    // A step note has no note-off: its length is all ones in this representation.
    if duration == 0 || duration == u32::MAX {
        return Ok(None);
    }
    let mut note = suhina_model::Note::new(
        f64::from(dword(r, 0, offset)?) / ppq,
        f64::from(duration) / ppq,
        key as u8,
        f32::from(velocity) / 128.0,
    );
    note.muted = r.get(19).is_some_and(|b| b & 0x20 != 0);
    Ok(Some(PatternNote {
        channel: word(r, 6, offset)?,
        note,
        expression: word(r, 4, offset)? & 8 != 0
            || r.get(16).is_some_and(|v| *v != 120)
            || r.get(20).is_some_and(|v| *v != 64),
    }))
}

pub(crate) struct Placement {
    pub row: u16,
    pub source: u16,
    pub start: f64,
    pub length: f64,
    pub offset: f64,
    /// Pattern placements without a trim window play the whole pattern.
    pub untrimmed_pattern: bool,
    pub enabled: bool,
    pub fades: [(f64, f64); 2],
    pub gain: f64,
}

pub(crate) fn placements(e: &Event, ppq: f64, old_rows: bool) -> Result<Vec<Placement>, FlpError> {
    let d = data(e)?;
    if d.is_empty() {
        return Ok(Vec::new());
    }
    let size = [88, 80, 60, 32]
        .into_iter()
        .find(|&n| d.len().is_multiple_of(n) && d.chunks_exact(n).all(|r| r.get(4..6) == Some(&[0, 0x50])))
        .ok_or_else(|| error(e.offset, "unsupported or truncated playlist record layout"))?;
    if d.len() / size > 1_000_000 {
        return Err(error(e.offset, "too many playlist placements"));
    }
    d.chunks_exact(size)
        .map(|r| {
            let row = word(r, 12, e.offset)?;
            let row = (if old_rows { 198u16 } else { 499u16 })
                .checked_sub(row)
                .ok_or_else(|| error(e.offset, "invalid playlist row"))?;
            let source = word(r, 6, e.offset)?;
            let trim = dword(r, 24, e.offset)?;
            let length = f64::from(dword(r, 8, e.offset)?) / ppq;
            if length <= 0. {
                return Err(error(e.offset, "playlist clip has zero length"));
            }
            Ok(Placement {
                row,
                source,
                start: f64::from(dword(r, 0, e.offset)?) / ppq,
                length,
                untrimmed_pattern: source >= 0x5000 && trim == u32::MAX,
                offset: if source >= 0x5000 {
                    if trim != u32::MAX { f64::from(trim) / ppq } else { 0. }
                } else {
                    let value = f32::from_bits(trim);
                    if !value.is_finite() && trim != u32::MAX {
                        return Err(error(e.offset, "non-finite audio/automation offset"));
                    }
                    if trim == u32::MAX { 0. } else { f64::from(value.max(0.)) }
                },
                enabled: r.get(19).is_none_or(|b| b & 0x20 == 0),
                fades: if source < 0x5000 && size >= 60 {
                    [stored_fade(r, 36, e.offset)?, stored_fade(r, 44, e.offset)?]
                } else {
                    [(0., 0.); 2]
                },
                gain: if source < 0x5000 && size >= 60 {
                    let gain = f32::from_bits(dword(r, 52, e.offset)?);
                    if gain.is_finite() && gain > 0. { f64::from(gain) } else { 1. }
                } else {
                    1.
                },
            })
        })
        .collect()
}

fn stored_fade(record: &[u8], at: usize, offset: usize) -> Result<(f64, f64), FlpError> {
    let duration = f64::from(f32::from_bits(dword(record, at, offset)?));
    let tension = f64::from(f32::from_bits(dword(record, at + 4, offset)?));
    Ok((duration, tension))
}
