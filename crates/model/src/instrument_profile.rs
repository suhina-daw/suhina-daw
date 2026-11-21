// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opaque native modal state, independent of the sample-reference envelope.

use crate::{Device, DeviceKind, Extension};

const KEY: &str = "instrument_modal_profile";
/// Room for a revision-4 set of 16 fitted recordings with their attacks (about 800 KiB).
const MAX_BYTES: usize = 4 << 20;

impl Device {
    /// Opaque modal bytes. The native profile codec validates their contents before rendering.
    pub fn instrument_modal_profile(&self) -> Option<&[u8]> {
        if !matches!(&self.kind, DeviceKind::Builtin(id) if id == "instrument") {
            return None;
        }
        self.extensions.0.iter().find(|ext| ext.format == "suhina" && ext.key == KEY).map(|ext| ext.data.as_slice())
    }

    /// Store already-validated profile bytes without touching sample references. Bounds are
    /// deliberately format-independent here; DSP owns the canonical schema and stricter size.
    pub fn set_instrument_modal_profile(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if !matches!(&self.kind, DeviceKind::Builtin(id) if id == "instrument") {
            return Err("modal profiles need an Instrument");
        }
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err("modal profile state needs 1 byte to 4 MiB");
        }
        self.clear_instrument_modal_profile();
        self.extensions.0.push(Extension { format: "suhina".into(), key: KEY.into(), data: bytes.to_vec() });
        Ok(())
    }

    pub fn clear_instrument_modal_profile(&mut self) {
        self.extensions.0.retain(|ext| !(ext.format == "suhina" && ext.key == KEY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_and_sample_references_have_independent_lifetimes() {
        let mut device = Device::new("Instrument", DeviceKind::Builtin("instrument".into()));
        device.set_sample_file("source.wav");
        device.set_instrument_modal_profile(&[1, 2, 3]).unwrap();
        device.set_sample_reference("moved.wav", "C:/sources/moved.wav");
        assert_eq!(device.instrument_modal_profile(), Some([1, 2, 3].as_slice()));
        assert_eq!(device.sample_file(), Some("moved.wav"));
        let before = device.clone();
        assert!(device.set_instrument_modal_profile(&vec![0; MAX_BYTES + 1]).is_err());
        assert_eq!(device, before);
        device.clear_instrument_modal_profile();
        assert_eq!(device.sample_file(), Some("moved.wav"));
        assert!(device.instrument_modal_profile().is_none());
    }
}
