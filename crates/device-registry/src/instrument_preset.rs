// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bounded native Instrument payload in the existing preset codec.

use serde_json::{Value, json};
use suhina_dsp::modal::profile::{ModalProfile, PROFILE_BYTES, PROFILE_MODEL_REVISION};

/// Encoded source limit, independent of the audio decoder's decoded-frame bound.
pub const MAX_PRESET_SAMPLE_BYTES: u64 = 128 * 1024 * 1024;

/// The root relative to which a preset source is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSourceScope {
    Library,
    Preset,
}

/// One source whose identity is checked before it is decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetSample {
    pub scope: PresetSourceScope,
    pub path: String,
    pub sha256: [u8; 32],
    pub bytes: u64,
}

impl PresetSample {
    pub fn validate(&self) -> Result<(), String> {
        if !portable_relative_path(&self.path) {
            return Err("an Instrument source needs a portable relative path".into());
        }
        if self.bytes == 0 || self.bytes > MAX_PRESET_SAMPLE_BYTES {
            return Err("an Instrument preset source must be between 1 byte and 128 MiB".into());
        }
        Ok(())
    }

    pub fn digest_text(&self) -> String {
        self.sha256.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn value(&self) -> Value {
        json!({
            "scope": match self.scope { PresetSourceScope::Library => "library", PresetSourceScope::Preset => "preset" },
            "path": self.path, "sha256": self.digest_text(), "bytes": self.bytes,
        })
    }

    fn read(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("an Instrument sample must be an object")?;
        if object.len() != 4 {
            return Err("an Instrument sample has scope, path, sha256 and bytes".into());
        }
        let scope = match value["scope"].as_str() {
            Some("library") => PresetSourceScope::Library,
            Some("preset") => PresetSourceScope::Preset,
            _ => return Err("an Instrument sample scope is library or preset".into()),
        };
        let path = value["path"].as_str().ok_or("an Instrument sample needs a path")?.to_owned();
        let text = value["sha256"].as_str().ok_or("an Instrument sample needs its SHA-256")?;
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("an Instrument SHA-256 has 64 lowercase hexadecimal characters".into());
        }
        let mut sha256 = [0; 32];
        for (out, pair) in sha256.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let digit = |b: u8| if b.is_ascii_digit() { b - b'0' } else { b - b'a' + 10 };
            if let [high, low] = pair {
                *out = digit(*high) * 16 + digit(*low);
            }
        }
        let bytes = value["bytes"].as_u64().ok_or("an Instrument sample needs an encoded byte length")?;
        let sample = Self { scope, path, sha256, bytes };
        sample.validate()?;
        Ok(sample)
    }
}

/// Prepared model data, not executable code or derived analysis cache.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentPreset {
    pub profile: Option<ModalProfile>,
    pub sample: Option<PresetSample>,
}

impl InstrumentPreset {
    pub(crate) fn read(value: &Value, revision: f64) -> Result<Self, String> {
        let object = value.as_object().ok_or("an Instrument v2 preset needs its instrument object")?;
        if object.len() != 2 || !object.contains_key("profile") || !object.contains_key("sample") {
            return Err("the instrument object has profile and sample fields".into());
        }
        let profile = match &value["profile"] {
            Value::Null => None,
            Value::Array(bytes) if bytes.len() == PROFILE_BYTES => {
                let mut encoded = [0; PROFILE_BYTES];
                for (out, value) in encoded.iter_mut().zip(bytes) {
                    *out =
                        value.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or("a profile byte is 0 through 255")?;
                }
                Some(ModalProfile::decode(&encoded).map_err(|e| format!("invalid Instrument profile: {e}"))?)
            }
            _ => return Err(format!("an Instrument profile has exactly {PROFILE_BYTES} bytes")),
        };
        let sample = if value["sample"].is_null() { None } else { Some(PresetSample::read(&value["sample"])?) };
        let preset = Self { profile, sample };
        preset.validate(revision)?;
        Ok(preset)
    }

    pub fn validate(&self, revision: f64) -> Result<(), String> {
        let expected = if self.profile.is_some() { f64::from(PROFILE_MODEL_REVISION) } else { 1.0 };
        if revision != expected {
            return Err(format!(
                "Instrument model revision {revision} does not match supported profile revision {expected}"
            ));
        }
        if let Some(sample) = &self.sample {
            sample.validate()?;
        }
        Ok(())
    }

    pub(crate) fn value(&self) -> Value {
        let profile = self.profile.as_ref().map(|p| p.encode().to_vec());
        json!({ "profile": profile, "sample": self.sample.as_ref().map(PresetSample::value) })
    }
}

/// A portable component check independent of the machine parsing the document.
pub fn portable_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .split('.')
                    .next()
                    .is_some_and(|stem| super::RESERVED_FILE_NAMES.contains(&stem.to_ascii_uppercase().as_str()))
        })
}
