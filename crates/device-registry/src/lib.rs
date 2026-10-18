// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in effect or instrument by its stable id (ADR 0004): what the model stores as
//! `DeviceKind::Builtin(id)` turns into a running device here. The engine builds device chains
//! through this crate; front ends list what can be added.

use std::sync::Arc;

use suhina_device_api::{Device, DeviceInfo};

/// The built-in effects and instruments, in the order a device browser lists them.
pub const DEVICES: [&DeviceInfo; 10] = [
    &suhina_device_fm_synth::INFO,
    &suhina_device_utility::INFO,
    &suhina_device_auto_pan::INFO,
    &suhina_device_dj_eq::INFO,
    &suhina_device_vinyl_distortion::INFO,
    &suhina_device_overdrive::INFO,
    &suhina_device_compressor::INFO,
    &suhina_device_limiter::INFO,
    &suhina_device_nam_amp_modeler::INFO,
    &suhina_device_nam_cab_loader::INFO,
];

/// What the device with this id is, or `None` for an unknown id.
pub fn info(id: &str) -> Option<&'static DeviceInfo> {
    DEVICES.iter().copied().find(|info| info.id == id)
}

/// A new device with this id at its defaults, prepared for 48 kHz, or `None` for an unknown
/// id. Allocates: call it off the audio thread.
pub fn create(id: &str) -> Option<Box<dyn Device>> {
    Some(match id {
        "fm-synth" => Box::new(suhina_device_fm_synth::FmSynth::new()),
        "utility" => Box::new(suhina_device_utility::Utility::new()),
        "auto-pan" => Box::new(suhina_device_auto_pan::AutoPan::new()),
        "dj-eq" => Box::new(suhina_device_dj_eq::DjEq::new()),
        "vinyl-distortion" => Box::new(suhina_device_vinyl_distortion::VinylDistortion::new()),
        "overdrive" => Box::new(suhina_device_overdrive::Overdrive::new()),
        "compressor" => Box::new(suhina_device_compressor::Compressor::new()),
        "limiter" => Box::new(suhina_device_limiter::Limiter::new()),
        "nam-amp-modeler" => Box::new(suhina_device_nam_amp_modeler::NamAmp::new()),
        "nam-cab-loader" => Box::new(suhina_device_nam_cab_loader::CabLoader::new()),
        _ => return None,
    })
}

/// The file a file-backed device runs, already read and parsed or decoded (docs/spec/nam-amp-and-cab.md):
/// a model for the Amp Modeler, an impulse response for the Cab Loader. The session keeps a
/// map of these by the file's path; the engine hands the device the map's entry when it builds
/// its chains.
#[derive(Debug, Clone)]
pub enum DeviceAsset {
    Nam(Arc<suhina_device_nam_amp_modeler::NamModel>),
    Ir(Arc<suhina_dsp::Ir>),
}

/// The ids whose devices run a file stored in `Device::state` (docs/spec/nam-amp-and-cab.md).
pub fn is_file_device(id: &str) -> bool {
    matches!(id, "nam-amp-modeler" | "nam-cab-loader")
}

/// A new device with this id, its defaults, and `asset` already loaded — `None` for an id that
/// takes no file, or for an unknown id. Allocation: off the audio thread.
pub fn create_with(id: &str, asset: Option<&DeviceAsset>) -> Option<Box<dyn Device>> {
    match (id, asset) {
        ("nam-amp-modeler", Some(DeviceAsset::Nam(model))) => {
            Some(Box::new(suhina_device_nam_amp_modeler::NamAmp::with_model(Arc::clone(model))))
        }
        ("nam-cab-loader", Some(DeviceAsset::Ir(ir))) => {
            Some(Box::new(suhina_device_nam_cab_loader::CabLoader::with_ir((**ir).clone())))
        }
        _ => create(id),
    }
}

/// Largest preset file read.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;

/// A device's values from a preset file (`docs/spec/browser.md` › Built-in effects and instruments, presets
/// and plug-ins).
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// The device it is for.
    pub device: &'static str,
    /// The preset's own name (a DX7 voice's name).
    pub name: String,
    /// Every parameter by its id, with the preset's value.
    pub params: Vec<(&'static str, f64)>,
}

/// The voices a preset file holds: a DX7 single voice (1) or bank (32). Each is `(number from
/// 1, name)`.
pub fn preset_voices(data: &[u8]) -> Result<Vec<(usize, String)>, String> {
    let voices = read_voices(data)?;
    Ok(voices.patches.iter().enumerate().map(|(i, p)| (i + 1, p.name().trim().to_string())).collect())
}

/// The preset `voice` (from 1) of a preset file: DX7 voice data for the FM synth.
pub fn preset(data: &[u8], voice: usize) -> Result<Preset, String> {
    let voices = read_voices(data)?;
    let count = voices.patches.len();
    let patch = voice
        .checked_sub(1)
        .and_then(|i| voices.patches.get(i))
        .ok_or_else(|| format!("voice {voice} is not in the file; it holds {count}"))?;
    let mut synth = suhina_device_fm_synth::FmSynth::new();
    synth.load_patch(patch);
    let info = &suhina_device_fm_synth::INFO;
    let params =
        info.params.iter().enumerate().filter_map(|(i, p)| synth.param(i).map(|value| (p.id, value))).collect();
    Ok(Preset { device: info.id, name: patch.name().trim().to_string(), params })
}

fn read_voices(data: &[u8]) -> Result<suhina_device_fm_synth::patch::Voices, String> {
    if data.len() > MAX_PRESET_BYTES {
        return Err(format!("a preset file is at most {} KiB", MAX_PRESET_BYTES / 1024));
    }
    suhina_device_fm_synth::patch::read(data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dx7_voice_is_a_preset_for_the_fm_synth() {
        use suhina_device_fm_synth::patch::{Patch, write_bank, write_voice};
        let mut patch = Patch::default();
        patch.set_name("GLASS BELL");
        patch.set(134, 21); // algorithm 22
        let single = write_voice(&patch);
        let preset = preset(&single, 1).unwrap();
        assert_eq!((preset.device, preset.name.as_str()), ("fm-synth", "GLASS BELL"));
        assert_eq!(preset.params.len(), suhina_device_fm_synth::INFO.params.len());
        let mut synth = suhina_device_fm_synth::FmSynth::new();
        synth.load_patch(&patch);
        for (i, (id, value)) in preset.params.iter().enumerate() {
            assert_eq!(Some(*value), synth.param(i), "{id}");
        }
        assert_eq!(preset_voices(&single).unwrap(), vec![(1, "GLASS BELL".to_string())]);

        let bank = write_bank(&vec![patch; 32]);
        assert_eq!(preset_voices(&bank).unwrap().len(), 32);
        assert_eq!(super::preset(&bank, 32).unwrap().name, "GLASS BELL");
        assert!(super::preset(&bank, 33).is_err());
        assert!(super::preset(&bank, 0).is_err());
        assert!(super::preset(b"not a preset", 1).is_err());
        assert!(super::preset(&vec![0u8; MAX_PRESET_BYTES + 1], 1).is_err());
    }

    #[test]
    fn every_listed_device_can_be_made_and_ids_are_unique() {
        for (i, info) in DEVICES.iter().enumerate() {
            let device = create(info.id).unwrap_or_else(|| panic!("{} cannot be made", info.id));
            assert_eq!(device.info(), *info);
            assert_eq!(super::info(info.id), Some(*info));
            assert!(DEVICES.iter().skip(i + 1).all(|other| other.id != info.id), "{} twice", info.id);
        }
        assert!(create("no-such-device").is_none());
        assert!(info("").is_none());
    }
}
