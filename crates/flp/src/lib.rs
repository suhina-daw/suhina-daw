// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the container and the event stream of FL Studio projects (`.flp`).
//!
//! The format is described in `docs/spec/flp-format.md`, a draft written from public
//! documentation and from projects we authored ourselves. This crate makes a file's structure
//! readable: the `FLhd` header, the `FLdt` data chunk and the events in it, with their values
//! decoded on request. Mapping the events onto the project model is a later piece of work.
//!
//! Every error carries the offset in the file where reading stopped, counted from its first byte.

mod event;

use std::fmt;
use std::io::Read;
use std::path::Path;

pub use event::{Event, MAX_EVENTS, Payload, Value, ValueKind, ids, kind_of};

/// Upper bound for a file read into memory, so a hostile file cannot exhaust it.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The header chunk and the data chunk header, before the events.
pub const PREAMBLE_LEN: usize = 22;
const HEADER_MAGIC: [u8; 4] = *b"FLhd";
const DATA_MAGIC: [u8; 4] = *b"FLdt";
const HEADER_LEN: u32 = 6;
/// Offset of the data chunk's magic.
const DATA_MAGIC_OFFSET: usize = 14;
/// FL Studio 11.5 was the first version to write strings as UTF-16LE.
const FIRST_UTF16_VERSION: (u32, u32) = (11, 5);

/// What kind of file the header says this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A whole project (0).
    Project,
    /// Any other value, kept as it is until a spec says what it means.
    Other(u16),
}

impl From<u16> for FileKind {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::Project,
            other => Self::Other(other),
        }
    }
}

/// The fields of the `FLhd` chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: FileKind,
    /// Channels in the channel rack.
    pub channels: u16,
    /// Ticks per quarter note.
    pub ppq: u16,
}

/// How the strings of a file are encoded, decided by its version event (199).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// One byte per character, before FL Studio 11.5.
    Ascii,
    /// FL Studio 11.5 and later.
    Utf16Le,
}

/// A file's header and the events of its data chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct Flp {
    pub header: Header,
    /// Bytes of events, as the data chunk's header gives it.
    pub data_len: u32,
    pub events: Vec<Event>,
    /// The version event's text and offset, when the file has one.
    version: Option<(String, usize)>,
    encoding: Option<TextEncoding>,
}

impl Flp {
    /// The FL Studio version that saved the file (event 199), for example `26.1.0.5530`.
    pub fn version(&self) -> Option<&str> {
        self.version.as_ref().map(|(v, _)| v.as_str())
    }

    /// The encoding of the file's strings, or [`FlpError::NoVersion`] when the file has no
    /// version event to decide it.
    pub fn encoding(&self) -> Result<TextEncoding, FlpError> {
        self.encoding.ok_or(FlpError::NoVersion)
    }

    /// Decodes an event's value by the kind its id has ([`kind_of`]); an id with no known kind
    /// comes back as a plain number or as binary.
    pub fn value(&self, event: &Event) -> Result<Value, FlpError> {
        match kind_of(event.id) {
            ValueKind::String => Ok(Value::String(event.text(self.encoding()?)?)),
            ValueKind::AsciiString => Ok(Value::String(event.text(TextEncoding::Ascii)?)),
            kind => event.decode(kind),
        }
    }

    /// The first event with this id.
    pub fn first(&self, id: u8) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }
}

#[derive(Debug)]
pub enum FlpError {
    Io(std::io::Error),
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// A chunk does not start with the magic it should.
    BadMagic {
        offset: usize,
        expected: [u8; 4],
    },
    /// The file ends inside the header or the data chunk's header.
    TruncatedHeader {
        offset: usize,
    },
    /// The header chunk's length is not 6.
    UnsupportedHeader {
        offset: usize,
        len: u32,
    },
    /// The file is not exactly `22 + data length` bytes.
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    /// An event's value runs past the end of the data chunk.
    TruncatedEvent {
        offset: usize,
        id: u8,
    },
    /// More events than [`event::MAX_EVENTS`].
    TooManyEvents {
        offset: usize,
    },
    /// A length's bytes run to the end of the data chunk without a last byte.
    UnterminatedLength {
        offset: usize,
    },
    /// A length longer than five bytes or larger than 32 bits.
    BadLength {
        offset: usize,
    },
    /// A string without its terminator, of odd length, or not valid in its encoding.
    BadString {
        offset: usize,
        reason: &'static str,
    },
    /// A value whose size does not fit the kind it is read as.
    WrongSize {
        offset: usize,
        id: u8,
        expected: usize,
        actual: usize,
    },
    /// The version event is not dot-separated decimal numbers.
    BadVersion {
        offset: usize,
    },
    /// The file has no version event, so its strings' encoding is unknown.
    NoVersion,
}

impl fmt::Display for FlpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read the FL Studio project: {e}"),
            Self::TooLarge => write!(f, "the file is larger than {} MiB", MAX_FILE_BYTES / 1024 / 1024),
            Self::BadMagic { offset, expected } => {
                write!(
                    f,
                    "not an FL Studio project: expected {:?} at offset {offset}",
                    String::from_utf8_lossy(expected)
                )
            }
            Self::TruncatedHeader { offset } => write!(f, "the file ends inside its header, at offset {offset}"),
            Self::UnsupportedHeader { offset, len } => {
                write!(f, "unsupported header length {len} at offset {offset} (expected 6)")
            }
            Self::SizeMismatch { expected, actual } => {
                write!(f, "the data chunk says the file is {expected} bytes, but it is {actual}")
            }
            Self::TruncatedEvent { offset, id } => {
                write!(f, "event {id} at offset {offset} runs past the end of the data")
            }
            Self::TooManyEvents { offset } => write!(f, "more than {} events, at offset {offset}", event::MAX_EVENTS),
            Self::UnterminatedLength { offset } => write!(f, "the length at offset {offset} never ends"),
            Self::BadLength { offset } => write!(f, "the length at offset {offset} is too large"),
            Self::BadString { offset, reason } => write!(f, "the string at offset {offset} {reason}"),
            Self::WrongSize { offset, id, expected, actual } => {
                write!(f, "event {id} at offset {offset} has {actual} bytes, expected {expected}")
            }
            Self::BadVersion { offset } => write!(f, "the version at offset {offset} is not a version number"),
            Self::NoVersion => write!(f, "the file has no version event, so its strings cannot be read"),
        }
    }
}

impl std::error::Error for FlpError {}

impl From<std::io::Error> for FlpError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Reads a `.flp` file from disk.
pub fn read_file(path: &Path) -> Result<Flp, FlpError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(FlpError::TooLarge);
    }
    read_bytes(&bytes)
}

/// Reads a whole `.flp` file held in memory.
pub fn read_bytes(bytes: &[u8]) -> Result<Flp, FlpError> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(FlpError::TooLarge);
    }
    let (header, data_len) = read_preamble(bytes)?;
    let expected = PREAMBLE_LEN as u64 + u64::from(data_len);
    if bytes.len() as u64 != expected {
        return Err(FlpError::SizeMismatch { expected, actual: bytes.len() as u64 });
    }
    let data = bytes.get(PREAMBLE_LEN..).unwrap_or_default();
    let events = event::walk(data, PREAMBLE_LEN)?;
    let version = match events.iter().find(|e| e.id == ids::VERSION) {
        Some(e) => Some((e.text(TextEncoding::Ascii)?, e.offset)),
        None => None,
    };
    let encoding = match &version {
        Some((v, offset)) => Some(encoding_for(v, *offset)?),
        None => None,
    };
    Ok(Flp { header, data_len, events, version, encoding })
}

fn read_preamble(bytes: &[u8]) -> Result<(Header, u32), FlpError> {
    fn array<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N], FlpError> {
        bytes
            .get(at..at.saturating_add(N))
            .and_then(|b| <[u8; N]>::try_from(b).ok())
            .ok_or(FlpError::TruncatedHeader { offset: bytes.len() })
    }
    let magic = |at: usize, expected: [u8; 4]| -> Result<(), FlpError> {
        if array::<4>(bytes, at)? != expected {
            return Err(FlpError::BadMagic { offset: at, expected });
        }
        Ok(())
    };
    let u16_at = |at: usize| array::<2>(bytes, at).map(u16::from_le_bytes);
    let u32_at = |at: usize| array::<4>(bytes, at).map(u32::from_le_bytes);
    magic(0, HEADER_MAGIC)?;
    let header_len = u32_at(4)?;
    if header_len != HEADER_LEN {
        return Err(FlpError::UnsupportedHeader { offset: 4, len: header_len });
    }
    let header = Header { kind: FileKind::from(u16_at(8)?), channels: u16_at(10)?, ppq: u16_at(12)? };
    magic(DATA_MAGIC_OFFSET, DATA_MAGIC)?;
    let data_len = u32_at(DATA_MAGIC_OFFSET + 4)?;
    Ok((header, data_len))
}

/// The strings' encoding for a version text such as `26.1.0.5530`.
fn encoding_for(version: &str, offset: usize) -> Result<TextEncoding, FlpError> {
    let mut parts = version.split('.').map(|p| {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(FlpError::BadVersion { offset });
        }
        p.parse::<u32>().map_err(|_| FlpError::BadVersion { offset })
    });
    let major = parts.next().ok_or(FlpError::BadVersion { offset })??;
    let minor = parts.next().transpose()?.unwrap_or(0);
    for part in parts {
        part?;
    }
    Ok(if (major, minor) >= FIRST_UTF16_VERSION { TextEncoding::Utf16Le } else { TextEncoding::Ascii })
}
