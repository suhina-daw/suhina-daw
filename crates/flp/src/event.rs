// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The event stream of the data chunk: walking it, and decoding the values of its events.

use crate::{FlpError, TextEncoding};

/// Upper bound for the events of one file, so a hostile file cannot make the list exhaust
/// memory. Projects in the corpus have a few thousand.
pub const MAX_EVENTS: usize = 4 * 1024 * 1024;
/// The most bytes a length (unsigned LEB128) may take: five carry 35 bits, enough for 32.
const MAX_LENGTH_BYTES: usize = 5;

/// Event ids with a meaning in this crate.
pub mod ids {
    pub const CHANNEL_ENABLED: u8 = 0;
    pub const LICENSED: u8 = 28;
    pub const NEW_CHANNEL: u8 = 64;
    pub const NEW_PATTERN: u8 = 65;
    pub const TEMPO_COARSE: u8 = 66;
    pub const COLOR: u8 = 128;
    pub const TEMPO: u8 = 156;
    /// Written with a one-byte value although it is in the four-byte range (the spec's exception).
    pub const ONE_BYTE_EXCEPTION: u8 = 172;
    pub const NAME: u8 = 192;
    pub const PATTERN_NAME: u8 = 193;
    pub const TITLE: u8 = 194;
    pub const COMMENT: u8 = 195;
    pub const SAMPLE_PATH: u8 = 196;
    pub const URL: u8 = 197;
    pub const VERSION: u8 = 199;
    pub const LICENSEE: u8 = 200;
    pub const DEFAULT_PLUGIN_NAME: u8 = 201;
    pub const DATA_PATH: u8 = 202;
    pub const PLUGIN_NAME: u8 = 203;
    pub const INSERT_NAME: u8 = 204;
    pub const TIME_MARKER_NAME: u8 = 205;
    pub const GENRE: u8 = 206;
    pub const AUTHOR: u8 = 207;
}

/// An event's value as it is in the file: its size comes from the id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    Byte(u8),
    Word(u16),
    Dword(u32),
    Data(Vec<u8>),
}

impl Payload {
    /// The value's size in bytes.
    pub fn len(&self) -> usize {
        match self {
            Self::Byte(_) => 1,
            Self::Word(_) => 2,
            Self::Dword(_) => 4,
            Self::Data(d) => d.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One event of the data chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: u8,
    /// Where the id byte is, from the start of the file.
    pub offset: usize,
    pub payload: Payload,
}

/// How an event's bytes are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Unsigned,
    Signed,
    F32,
    TwoU16,
    Color,
    /// A string in the file's encoding.
    String,
    /// A string that is ASCII in every version (the version event).
    AsciiString,
    Binary,
}

/// A decoded value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Unsigned(u32),
    Signed(i32),
    F32(f32),
    TwoU16(u16, u16),
    /// Red, green, blue.
    Color(u8, u8, u8),
    String(String),
    Binary(Vec<u8>),
}

/// The kind an id is read as: the ones the spec names, otherwise a plain number of the id's size
/// or, for ids 192 to 255, binary.
pub fn kind_of(id: u8) -> ValueKind {
    use ids::*;
    match id {
        CHANNEL_ENABLED | LICENSED => ValueKind::Bool,
        COLOR => ValueKind::Color,
        VERSION => ValueKind::AsciiString,
        NAME..=URL | LICENSEE..=AUTHOR => ValueKind::String,
        192..=255 => ValueKind::Binary,
        _ => ValueKind::Unsigned,
    }
}

impl Event {
    /// The value's bytes, little-endian for numbers.
    pub fn bytes(&self) -> Vec<u8> {
        match &self.payload {
            Payload::Byte(b) => vec![*b],
            Payload::Word(w) => w.to_le_bytes().to_vec(),
            Payload::Dword(d) => d.to_le_bytes().to_vec(),
            Payload::Data(d) => d.clone(),
        }
    }

    /// Reads the value as `kind`. Strings need the file's encoding: use [`Event::text`] or
    /// [`crate::Flp::value`].
    pub fn decode(&self, kind: ValueKind) -> Result<Value, FlpError> {
        let wrong = |expected: usize| FlpError::WrongSize {
            offset: self.offset,
            id: self.id,
            expected,
            actual: self.payload.len(),
        };
        Ok(match (kind, &self.payload) {
            (ValueKind::Bool, Payload::Byte(b)) => Value::Bool(*b != 0),
            (ValueKind::Bool, _) => return Err(wrong(1)),
            (ValueKind::Unsigned, Payload::Byte(b)) => Value::Unsigned(u32::from(*b)),
            (ValueKind::Unsigned, Payload::Word(w)) => Value::Unsigned(u32::from(*w)),
            (ValueKind::Unsigned, Payload::Dword(d)) => Value::Unsigned(*d),
            (ValueKind::Signed, Payload::Byte(b)) => Value::Signed(i32::from(i8::from_le_bytes([*b]))),
            (ValueKind::Signed, Payload::Word(w)) => Value::Signed(i32::from(i16::from_le_bytes(w.to_le_bytes()))),
            (ValueKind::Signed, Payload::Dword(d)) => Value::Signed(i32::from_le_bytes(d.to_le_bytes())),
            (ValueKind::Unsigned | ValueKind::Signed, Payload::Data(_)) => return Err(wrong(4)),
            (ValueKind::F32, Payload::Dword(d)) => Value::F32(f32::from_bits(*d)),
            (ValueKind::TwoU16, Payload::Dword(d)) => {
                let [a, b, c, e] = d.to_le_bytes();
                Value::TwoU16(u16::from_le_bytes([a, b]), u16::from_le_bytes([c, e]))
            }
            (ValueKind::Color, Payload::Dword(d)) => {
                let [r, g, b, _] = d.to_le_bytes();
                Value::Color(r, g, b)
            }
            (ValueKind::F32 | ValueKind::TwoU16 | ValueKind::Color, _) => return Err(wrong(4)),
            (ValueKind::String, _) => {
                return Err(FlpError::BadString { offset: self.offset, reason: "needs the file's encoding" });
            }
            (ValueKind::AsciiString, _) => Value::String(self.text(TextEncoding::Ascii)?),
            (ValueKind::Binary, Payload::Data(d)) => Value::Binary(d.clone()),
            (ValueKind::Binary, _) => Value::Binary(self.bytes()),
        })
    }

    /// Reads the value as a NUL-terminated string in `encoding`.
    pub fn text(&self, encoding: TextEncoding) -> Result<String, FlpError> {
        let bad = |reason| FlpError::BadString { offset: self.offset, reason };
        let Payload::Data(data) = &self.payload else {
            return Err(bad("is not in a text event"));
        };
        match encoding {
            TextEncoding::Ascii => {
                let Some((&0, text)) = data.split_last() else {
                    return Err(bad("has no terminator"));
                };
                if !text.is_ascii() {
                    return Err(bad("has a byte above 127"));
                }
                Ok(text.iter().map(|&b| char::from(b)).collect())
            }
            TextEncoding::Utf16Le => {
                if data.len() % 2 != 0 {
                    return Err(bad("has an odd length for UTF-16"));
                }
                let units: Vec<u16> = data.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
                let Some((&0, text)) = units.split_last() else {
                    return Err(bad("has no terminator"));
                };
                String::from_utf16(text).map_err(|_| bad("is not valid UTF-16"))
            }
        }
    }
}

/// Walks the events of a data chunk whose first byte is at `base` in the file. The walk must end
/// exactly at the end of `data`.
pub(crate) fn walk(data: &[u8], base: usize) -> Result<Vec<Event>, FlpError> {
    let mut events = Vec::new();
    let mut pos = 0;
    while let Some(&id) = data.get(pos) {
        let offset = base.saturating_add(pos);
        if events.len() >= MAX_EVENTS {
            return Err(FlpError::TooManyEvents { offset });
        }
        pos += 1;
        let truncated = || FlpError::TruncatedEvent { offset, id };
        let fixed = |len: usize| data.get(pos..pos.saturating_add(len)).ok_or_else(truncated);
        let payload = match id {
            ids::ONE_BYTE_EXCEPTION | 0..=63 => {
                let b = fixed(1)?;
                pos += 1;
                Payload::Byte(b.first().copied().unwrap_or_default())
            }
            64..=127 => {
                let b = <[u8; 2]>::try_from(fixed(2)?).map_err(|_| truncated())?;
                pos += 2;
                Payload::Word(u16::from_le_bytes(b))
            }
            128..=191 => {
                let b = <[u8; 4]>::try_from(fixed(4)?).map_err(|_| truncated())?;
                pos += 4;
                Payload::Dword(u32::from_le_bytes(b))
            }
            192..=255 => {
                let (len, after) = read_length(data, pos, base)?;
                pos = after;
                let end = pos.checked_add(len).ok_or_else(truncated)?;
                let bytes = data.get(pos..end).ok_or_else(truncated)?;
                pos = end;
                Payload::Data(bytes.to_vec())
            }
        };
        events.push(Event { id, offset, payload });
    }
    Ok(events)
}

/// Reads an unsigned LEB128 length at `pos`; returns it and the position after it.
fn read_length(data: &[u8], pos: usize, base: usize) -> Result<(usize, usize), FlpError> {
    let offset = base.saturating_add(pos);
    let mut value: u64 = 0;
    for i in 0..MAX_LENGTH_BYTES {
        let at = pos.saturating_add(i);
        let Some(&b) = data.get(at) else {
            return Err(FlpError::UnterminatedLength { offset });
        };
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            let len = u32::try_from(value).ok().and_then(|v| usize::try_from(v).ok());
            return len.map(|len| (len, at + 1)).ok_or(FlpError::BadLength { offset });
        }
    }
    Err(FlpError::BadLength { offset })
}
