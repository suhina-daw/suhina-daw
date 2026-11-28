// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated fixed-capacity modal data shared by estimation, rendering and persistence.
//! The codec is data-only and allocates nothing; see `docs/spec/instrument-modal-profile.md`.

pub const MAX_PROFILE_MODES: usize = 16;
pub const PROFILE_SCHEMA_VERSION: u16 = 1;
pub const PROFILE_MODEL_REVISION: u16 = 2;
pub const PROFILE_BYTES: usize = 24 + MAX_PROFILE_MODES * 24;
const MAGIC: &[u8; 8] = b"SHMODAL\0";

/// One resonance relative to the profile's reference frequency. T60 is in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProfileMode {
    pub ratio: f64,
    pub magnitude: f64,
    pub decay_seconds: f64,
}

/// A bounded, validated profile. Reference key is a mapping anchor, not an inferred fundamental.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModalProfile {
    reference_key: u8,
    reference_hz: f64,
    count: u8,
    modes: [ProfileMode; MAX_PROFILE_MODES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileError(pub &'static str);

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ProfileError {}

impl ModalProfile {
    pub fn new(reference_key: u8, reference_hz: f64, modes: &[ProfileMode]) -> Result<Self, ProfileError> {
        if reference_key > 127 || !reference_hz.is_finite() || !(8.0..=20000.0).contains(&reference_hz) {
            return Err(ProfileError("modal reference needs key 0..127 and finite frequency 8..20000 Hz"));
        }
        if modes.is_empty() || modes.len() > MAX_PROFILE_MODES {
            return Err(ProfileError("modal profile needs 1..16 modes"));
        }
        let mut previous = 0.0;
        let mut audible = false;
        for mode in modes {
            if !mode.ratio.is_finite()
                || !(1.0 / 256.0..=256.0).contains(&mode.ratio)
                || mode.ratio <= previous
                || !mode.magnitude.is_finite()
                || !(0.0..=1.0).contains(&mode.magnitude)
                || !mode.decay_seconds.is_finite()
                || !(0.001..=60.0).contains(&mode.decay_seconds)
            {
                return Err(ProfileError(
                    "modal modes need increasing finite ratios, magnitudes 0..1 and T60 0.001..60",
                ));
            }
            previous = mode.ratio;
            audible |= mode.magnitude > 0.0;
        }
        if !audible {
            return Err(ProfileError("modal profile needs a nonzero magnitude"));
        }
        let mut stored = [ProfileMode::default(); MAX_PROFILE_MODES];
        for (target, mode) in stored.iter_mut().zip(modes) {
            *target = *mode;
        }
        Ok(Self { reference_key, reference_hz, count: modes.len() as u8, modes: stored })
    }

    pub fn reference_key(&self) -> u8 {
        self.reference_key
    }

    pub fn reference_hz(&self) -> f64 {
        self.reference_hz
    }

    pub fn modes(&self) -> &[ProfileMode] {
        self.modes.get(..usize::from(self.count)).unwrap_or(&[])
    }

    /// Canonical fixed-size little-endian representation, including zero unused slots.
    pub fn encode(&self) -> [u8; PROFILE_BYTES] {
        let mut bytes = [0; PROFILE_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&PROFILE_SCHEMA_VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&PROFILE_MODEL_REVISION.to_le_bytes());
        bytes[12] = self.reference_key;
        bytes[13] = self.count;
        bytes[16..24].copy_from_slice(&self.reference_hz.to_le_bytes());
        for (record, mode) in bytes[24..].as_chunks_mut::<24>().0.iter_mut().zip(self.modes()) {
            record[..8].copy_from_slice(&mode.ratio.to_le_bytes());
            record[8..16].copy_from_slice(&mode.magnitude.to_le_bytes());
            record[16..24].copy_from_slice(&mode.decay_seconds.to_le_bytes());
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProfileError> {
        let bytes: &[u8; PROFILE_BYTES] =
            bytes.try_into().map_err(|_| ProfileError("modal profile must be 408 bytes"))?;
        if bytes.get(..8) != Some(MAGIC.as_slice())
            || bytes.get(8..10) != Some(PROFILE_SCHEMA_VERSION.to_le_bytes().as_slice())
            || bytes.get(10..12) != Some(PROFILE_MODEL_REVISION.to_le_bytes().as_slice())
            || bytes.get(14..16) != Some([0, 0].as_slice())
        {
            return Err(ProfileError("unsupported modal profile header or revision"));
        }
        let count = usize::from(bytes[13]);
        if !(1..=MAX_PROFILE_MODES).contains(&count) {
            return Err(ProfileError("modal profile needs 1..16 modes"));
        }
        let mut modes = [ProfileMode::default(); MAX_PROFILE_MODES];
        for (index, record) in bytes[24..].as_chunks::<24>().0.iter().enumerate() {
            if index < count {
                if let Some(mode) = modes.get_mut(index) {
                    *mode = ProfileMode {
                        ratio: read_f64(record.get(..8))?,
                        magnitude: read_f64(record.get(8..16))?,
                        decay_seconds: read_f64(record.get(16..24))?,
                    };
                }
            } else if record.iter().any(|byte| *byte != 0) {
                return Err(ProfileError("unused modal profile slots must be zero"));
            }
        }
        Self::new(bytes[12], read_f64(bytes.get(16..24))?, modes.get(..count).unwrap_or(&[]))
    }
}

fn read_f64(bytes: Option<&[u8]>) -> Result<f64, ProfileError> {
    let bytes = bytes.and_then(|bytes| bytes.try_into().ok()).ok_or(ProfileError("truncated modal number"))?;
    Ok(f64::from_le_bytes(bytes))
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
