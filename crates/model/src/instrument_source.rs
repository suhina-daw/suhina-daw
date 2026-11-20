// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent source identity, independent of the sample path and modal profile.

use crate::{Device, DeviceKind, Extension};

const KEY: &str = "instrument_sample_identity";

/// Identity of encoded source bytes; a digest does not authorize reading a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstrumentSourceIdentity {
    pub sha256: [u8; 32],
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_survives_rebasing_but_malformed_state_is_an_error() {
        let mut device = Device::new("Instrument", DeviceKind::Builtin("instrument".into()));
        let identity = InstrumentSourceIdentity { sha256: [7; 32], bytes: 123 };
        device.set_instrument_source_identity(identity);
        device.set_sample_reference("moved.wav", "C:/sources/moved.wav");
        assert_eq!(device.instrument_source_identity(), Ok(Some(identity)));
        let entry = device.extensions.0.iter().find(|e| e.key == KEY).unwrap().clone();
        device.extensions.0.push(entry);
        assert!(device.instrument_source_identity().is_err());
        device.set_instrument_source_identity(identity);
        device.extensions.0.iter_mut().find(|e| e.key == KEY).unwrap().data.pop();
        assert!(device.instrument_source_identity().is_err());
        device.clear_instrument_source_identity();
        assert_eq!(device.instrument_source_identity(), Ok(None));
        assert_eq!(device.sample_file(), Some("moved.wav"));
    }
}

impl Device {
    pub fn instrument_source_identity(&self) -> Result<Option<InstrumentSourceIdentity>, &'static str> {
        if !matches!(&self.kind, DeviceKind::Builtin(id) if id == "instrument") {
            return Ok(None);
        }
        let mut entries = self.extensions.0.iter().filter(|e| e.format == "suhina" && e.key == KEY);
        let Some(entry) = entries.next() else { return Ok(None) };
        let data = &entry.data;
        if entries.next().is_some() {
            return Err("duplicate Instrument source identities");
        }
        if data.len() != 40 {
            return Err("an Instrument source identity has 40 bytes");
        }
        let sha256 = data.get(..32).and_then(|b| b.try_into().ok()).ok_or("invalid source digest")?;
        let bytes = u64::from_le_bytes(data.get(32..).and_then(|b| b.try_into().ok()).ok_or("invalid source length")?);
        if bytes == 0 {
            return Err("an Instrument source cannot be empty");
        }
        Ok(Some(InstrumentSourceIdentity { sha256, bytes }))
    }

    pub fn set_instrument_source_identity(&mut self, identity: InstrumentSourceIdentity) {
        self.clear_instrument_source_identity();
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&identity.sha256);
        data.extend_from_slice(&identity.bytes.to_le_bytes());
        self.extensions.0.push(Extension { format: "suhina".into(), key: KEY.into(), data });
    }

    pub fn clear_instrument_source_identity(&mut self) {
        self.extensions.0.retain(|e| !(e.format == "suhina" && e.key == KEY));
    }
}
