// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded portable parameter state; decoding validates the whole state before applying it.

use super::{ExportError, MAX_PARAMS, Parameters};

pub const MAX_BYTES: usize = 1_048_576;

pub fn save(parameters: &Parameters) -> Vec<u8> {
    let mut bytes = b"SHDV\x01\x00".to_vec();
    write_id(&mut bytes, &parameters.info.id);
    bytes.extend_from_slice(&(parameters.info.params.len() as u32).to_le_bytes());
    for (index, parameter) in parameters.info.params.iter().enumerate() {
        write_id(&mut bytes, &parameter.id);
        bytes.extend_from_slice(&parameters.value(index).unwrap_or(parameter.default).to_le_bytes());
    }
    bytes
}

fn write_id(bytes: &mut Vec<u8>, id: &str) {
    bytes.extend_from_slice(&(id.len() as u16).to_le_bytes());
    bytes.extend_from_slice(id.as_bytes());
}

/// Control-thread only. The processor takes a consistent snapshot at the next block boundary.
pub fn load(parameters: &Parameters, bytes: &[u8]) -> Result<(), ExportError> {
    if bytes.len() > MAX_BYTES {
        return Err(ExportError::InvalidState);
    }
    let mut reader = Reader(bytes);
    if reader.take(6)? != b"SHDV\x01\x00" || reader.id()? != parameters.info.id {
        return Err(ExportError::InvalidState);
    }
    let count = u32::from_le_bytes(reader.array()?) as usize;
    if count > MAX_PARAMS {
        return Err(ExportError::InvalidState);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let id = reader.id()?;
        let value = f64::from_le_bytes(reader.array()?);
        if !value.is_finite() || values.iter().any(|(previous, _)| *previous == id) {
            return Err(ExportError::InvalidState);
        }
        values.push((id, value));
    }
    if !reader.0.is_empty() {
        return Err(ExportError::InvalidState);
    }
    parameters.restore(&values)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ExportError> {
        let bytes = self.0.get(..length).ok_or(ExportError::InvalidState)?;
        self.0 = self.0.get(length..).ok_or(ExportError::InvalidState)?;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ExportError> {
        self.take(N)?.try_into().map_err(|_| ExportError::InvalidState)
    }

    fn id(&mut self) -> Result<&'a str, ExportError> {
        let length = usize::from(u16::from_le_bytes(self.array()?));
        if length == 0 || length > 256 {
            return Err(ExportError::InvalidState);
        }
        std::str::from_utf8(self.take(length)?).map_err(|_| ExportError::InvalidState)
    }
}
