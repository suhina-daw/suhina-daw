// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native Instrument state through the existing preset commands.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use suhina_device_registry::{InstrumentPreset, PresetSample, PresetSourceScope};
use suhina_media::verified_source::{decode_source_bytes, read_source, source_identity};
use suhina_model::{Device, InstrumentSourceIdentity};

use crate::{CommandError, Session};

fn identity(sample: &PresetSample) -> InstrumentSourceIdentity {
    InstrumentSourceIdentity { sha256: sample.sha256, bytes: sample.bytes }
}

/// Nested source of a successfully parsed native preset, for caller path confinement.
pub enum InstrumentPresetReference {
    Library,
    Collected(PathBuf),
}

pub fn instrument_preset_reference(path: &Path) -> Result<Option<InstrumentPresetReference>, String> {
    let bytes = suhina_fs::read_plain(path, suhina_device_registry::MAX_PRESET_BYTES)
        .map_err(|e| e.message(path, suhina_device_registry::MAX_PRESET_BYTES))?;
    // The command reports malformed/foreign preset data. This check never interprets it as
    // another permission; only a successfully parsed native source needs extra confinement.
    let Ok(preset) = suhina_device_registry::preset(&bytes, 1) else { return Ok(None) };
    let Some(sample) = preset.instrument.and_then(|state| state.sample) else { return Ok(None) };
    Ok(Some(match sample.scope {
        PresetSourceScope::Library => InstrumentPresetReference::Library,
        PresetSourceScope::Preset => InstrumentPresetReference::Collected(
            crate::files::absolute(path).parent().ok_or("the preset has no folder")?.join(sample.path),
        ),
    }))
}

/// Installs only verified audio; a path already decoded as different bytes is not replaced
/// underneath other devices. An explicit new source should use another file or reopen.
fn install_audio(
    session: &mut Session,
    path: &Path,
    data: suhina_model::AudioData,
) -> Result<Arc<suhina_model::AudioData>, CommandError> {
    let key = path.display().to_string();
    if let Some(old) = session.audio.pool.files.get(&key) {
        if old.sample_rate != data.sample_rate || old.channels != data.channels || old.samples != data.samples {
            return Err(
                "this source path already holds different decoded audio; reload the project or choose another file"
                    .into(),
            );
        }
        return Ok(Arc::clone(old));
    }
    session.add_audio(key.clone(), data);
    session.audio.pool.files.get(&key).cloned().ok_or_else(|| CommandError::new("prepared source was not retained"))
}

/// Reads the chosen Instrument source once for both identity and decoder input.
pub(crate) fn chosen_source(
    session: &mut Session,
    path: &Path,
) -> Result<(Arc<suhina_model::AudioData>, InstrumentSourceIdentity), CommandError> {
    let bytes = read_source(path).map_err(|e| e.to_string())?;
    let identity = source_identity(&bytes);
    let data = decode_source_bytes(path, bytes, identity).map_err(|e| e.to_string())?;
    Ok((install_audio(session, path, data)?, identity))
}

/// Prepares a local scratch device before it is inserted by the normal undoable add.
pub(crate) fn prepare(
    session: &mut Session,
    preset: &InstrumentPreset,
    path: Option<&Path>,
    device: &mut Device,
) -> Result<Option<String>, CommandError> {
    if let Some(profile) = &preset.profile {
        device.set_instrument_modal_profile(&profile.encode()).map_err(CommandError::new)?;
    }
    let Some(sample) = &preset.sample else { return Ok(None) };
    let root = match sample.scope {
        PresetSourceScope::Library => session.library_base().ok_or("the library folder is not set")?.to_path_buf(),
        PresetSourceScope::Preset => crate::files::absolute(path.ok_or("a collected source needs its preset file")?)
            .parent()
            .ok_or("the preset has no folder")?
            .to_path_buf(),
    };
    let root = crate::files::absolute(&root);
    let recorded = root.join(&sample.path);
    let expected = identity(sample);
    device.set_sample_file(&recorded.display().to_string());
    device.set_instrument_source_identity(expected);
    // No fallback to another library, absolute path, network share or outside symlink.
    let Some(found) = suhina_media::locate(&sample.path, None, &root, &suhina_media::Trusted::default()) else {
        return Ok(Some(format!("Instrument source {} is missing or outside its permitted folder", sample.path)));
    };
    let data = suhina_media::verified_source::decode_verified_source(&found, expected).map_err(|e| e.to_string())?;
    install_audio(session, &found, data)?;
    device.set_sample_file(&found.display().to_string());
    Ok(None)
}

/// Checks every affected hashed Instrument before a relink decodes or changes a source.
pub(crate) fn verify_relink(session: &mut Session, file: &str, target: &Path) -> Result<(), CommandError> {
    let mut expected = Vec::new();
    for device in suhina_media::sampler_devices(session.project())
        .into_iter()
        .filter(|d| d.sample_file() == Some(file) || d.sample_absolute_file() == Some(file))
    {
        if let Some(identity) = device.instrument_source_identity()? {
            expected.push(identity);
        }
    }
    let Some(first) = expected.first().copied() else { return Ok(()) };
    if expected.iter().any(|other| *other != first) {
        return Err("the source is used with conflicting Instrument identities".into());
    }
    let data = suhina_media::verified_source::decode_verified_source(target, first).map_err(|e| e.to_string())?;
    install_audio(session, target, data)?;
    Ok(())
}

fn portable_path(path: &Path) -> Result<String, CommandError> {
    let text = path
        .components()
        .map(|c| c.as_os_str().to_str().ok_or("a source path must be Unicode"))
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    if !suhina_device_registry::portable_relative_path(&text) {
        return Err("this source filename is not portable; choose a file with a portable name".into());
    }
    Ok(text)
}

#[derive(Default)]
struct SavedSource {
    sample: Option<PresetSample>,
    copy: Option<(PathBuf, Vec<u8>)>,
}

fn save_source(
    session: &Session,
    device: &Device,
    folder: &Path,
    name: &str,
    collect: bool,
) -> Result<SavedSource, CommandError> {
    let Some(file) = device.sample_file() else { return Ok(SavedSource::default()) };
    let path = session
        .loaded_path(file, device.sample_absolute_file())
        .ok_or("the Instrument source is missing; locate it before saving a preset")?;
    let bytes = read_source(&path).map_err(|e| e.to_string())?;
    let got = source_identity(&bytes);
    if device.instrument_source_identity()?.is_some_and(|expected| expected != got) {
        return Err("the source changed after it was loaded; the preset was not saved".into());
    }
    // Legacy un-hashed projects also save only the audio that is actually sounding.
    let decoded = decode_source_bytes(&path, bytes.clone(), got).map_err(|e| e.to_string())?;
    if let Some(loaded) = session.audio.pool.files.get(file)
        && (loaded.sample_rate != decoded.sample_rate
            || loaded.channels != decoded.channels
            || loaded.samples != decoded.samples)
    {
        return Err("the source changed after it was decoded; the preset was not saved".into());
    }
    let digest = got.sha256.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let (scope, relative, copy) = if collect {
        let owner_name = path.file_name().ok_or("the source has no filename")?;
        let relative = PathBuf::from(format!("{name}.assets")).join(digest).join(owner_name);
        let copy = folder.join(&relative);
        (PresetSourceScope::Preset, portable_path(&relative)?, Some((copy, bytes)))
    } else {
        let base =
            session.library_base().ok_or("the library folder is not set")?.canonicalize().map_err(|e| e.to_string())?;
        let canonical = path.canonicalize().map_err(|e| e.to_string())?;
        let relative = canonical
            .strip_prefix(&base)
            .map_err(|_| "the source is outside the library; select Collect source with preset")?;
        (PresetSourceScope::Library, portable_path(relative)?, None)
    };
    Ok(SavedSource { sample: Some(PresetSample { scope, path: relative, sha256: got.sha256, bytes: got.bytes }), copy })
}

/// Adds the complete payload and publishes the preset last, after any verified collected asset.
pub(crate) fn save(
    session: &Session,
    device: &Device,
    name: &str,
    values: &[(&str, f64)],
    collect: bool,
) -> Result<Value, CommandError> {
    // Validate the name and declared controls before deriving any filesystem destination.
    suhina_device_registry::write_preset_file("instrument", name, None, values).map_err(CommandError::new)?;
    let base = session.library_base().ok_or("the library folder is not set")?;
    let folder = base.join("Presets").join("instrument");
    let profile = device.instrument_modal_profile().map(decode_profile).transpose()?;
    let revision = device.params.iter().rev().find(|p| p.id == "model_revision").map_or(1.0, |p| p.value);
    InstrumentPreset { profile, sample: None }.validate(revision).map_err(CommandError::new)?;
    let SavedSource { sample, copy } = save_source(session, device, &folder, name, collect)?;
    let payload = InstrumentPreset { profile, sample };
    let encoded = suhina_device_registry::write_instrument_preset(name, values, &payload).map_err(CommandError::new)?;
    if let Some((target, bytes)) = copy {
        checked_destination(base, &target)?;
        checked_destination(base, &suhina_fs::beside(&target))?;
        // A collected source keeps its owner's name. Hash directories make reuse explicit.
        if target.exists() {
            let found = read_source(&target).map_err(|e| e.to_string())?;
            if found != bytes {
                return Err("the collected source destination holds different bytes".into());
            }
        } else {
            // Refuse a pre-existing link anywhere below the library before writing.
            suhina_fs::write_atomic(&target, &bytes).map_err(|e| e.to_string())?;
        }
    }
    let target = folder.join(format!("{name}.instrument.preset.suhina"));
    checked_destination(base, &target)?;
    checked_destination(base, &suhina_fs::beside(&target))?;
    suhina_fs::write_atomic(&target, encoded).map_err(|e| e.to_string())?;
    Ok(json!({"preset":target.display().to_string(),"name":name,"collected":collect}))
}

fn decode_profile(bytes: &[u8]) -> Result<suhina_device_registry::ModalProfile, CommandError> {
    suhina_device_registry::ModalProfile::decode(bytes).map_err(|e| CommandError::new(e.to_string()))
}

fn checked_destination(base: &Path, target: &Path) -> Result<(), CommandError> {
    let relative = target.strip_prefix(base).map_err(|_| "preset destination leaves the library")?;
    let mut current = base.to_path_buf();
    for part in relative.components() {
        current.push(part);
        if std::fs::read_link(&current).is_ok() {
            return Err("a preset destination must not follow a symbolic link or junction".into());
        }
    }
    Ok(())
}
