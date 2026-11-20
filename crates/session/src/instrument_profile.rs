// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One atomic mutation shared by explicit modal adoption and future fit acceptance.

use serde_json::{Value, json};
use suhina_dsp::modal::profile::{MAX_PROFILE_MODES, ModalProfile, PROFILE_MODEL_REVISION, ProfileMode};
use suhina_model::{Device, DeviceKind, DeviceParam, device_at_mut};

use crate::commands::{Args, device_channel, with_override};
use crate::device_groups::{device_path, path_value};
use crate::{CommandError, Session};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProfileSource {
    Modal,
    Hybrid,
}

/// Called inside an already-authorized EDIT handler, never by nested `execute`. A job-owning
/// caller checks its captured DeviceId/source identity first. Returns whether a lane was overridden.
pub(crate) fn apply_profile(
    device: &mut Device,
    profile: &ModalProfile,
    source: ProfileSource,
) -> Result<bool, CommandError> {
    if !matches!(&device.kind, DeviceKind::Builtin(id) if id == "instrument") {
        return Err("modal adoption needs an Instrument".into());
    }
    device.set_instrument_modal_profile(&profile.encode())?;
    let values = [
        ("model_revision", f64::from(PROFILE_MODEL_REVISION)),
        (
            "source",
            match source {
                ProfileSource::Modal => 0.0,
                ProfileSource::Hybrid => 2.0,
            },
        ),
        ("quality", 1.0),
        ("body_decay", 2.0),
        ("brightness", 0.5),
        ("structure", 0.002),
        ("strike", 0.25),
    ];
    let mut overridden = false;
    for (id, value) in values {
        if let Some(param) = device.params.iter_mut().find(|param| param.id == id) {
            param.value = value;
        } else {
            device.params.push(DeviceParam::new(id, value));
        }
        overridden |= crate::automation::override_lane(&mut device.automation, |lane| lane.param == id);
    }
    Ok(overridden)
}

/// The revision-4 counterpart of [`apply_profile`]: writes already-encoded set bytes, revision 4
/// and Source Modal; with `reset_controls` also the full mode budget and the neutral model
/// controls (acceptance), without them when a recording joins an existing set.
pub(crate) fn apply_model_set(device: &mut Device, bytes: &[u8], reset_controls: bool) -> Result<bool, CommandError> {
    if !matches!(&device.kind, DeviceKind::Builtin(id) if id == "instrument") {
        return Err("modal adoption needs an Instrument".into());
    }
    device.set_instrument_modal_profile(bytes)?;
    let mut values = vec![("model_revision", f64::from(suhina_dsp::modal::model::MODEL_REVISION)), ("source", 0.0)];
    if reset_controls {
        values.extend([
            ("quality", 1.0),
            ("body_decay", 2.0),
            ("brightness", 0.5),
            ("structure", 0.002),
            ("strike", 0.25),
        ]);
    }
    let mut overridden = false;
    for (id, value) in values {
        if let Some(param) = device.params.iter_mut().find(|param| param.id == id) {
            param.value = value;
        } else {
            device.params.push(DeviceParam::new(id, value));
        }
        overridden |= crate::automation::override_lane(&mut device.automation, |lane| lane.param == id);
    }
    Ok(overridden)
}

fn parse_profile(value: &Value) -> Result<ModalProfile, CommandError> {
    let object = value.as_object().ok_or("profile must be an object")?;
    if object.keys().any(|key| !matches!(key.as_str(), "referenceKey" | "referenceHz" | "modes")) {
        return Err("profile has an unknown field".into());
    }
    let args = Args::new(value);
    let key = u8::try_from(args.index("referenceKey", "from 0 to 127")?).map_err(|_| "referenceKey must be 0..127")?;
    let hz = args.f64("referenceHz")?;
    let supplied = args.value("modes").and_then(Value::as_array).ok_or("modes must be an array")?;
    if supplied.is_empty() || supplied.len() > MAX_PROFILE_MODES {
        return Err("modal profile needs 1..16 modes".into());
    }
    let mut modes = [ProfileMode::default(); MAX_PROFILE_MODES];
    for (target, supplied) in modes.iter_mut().zip(supplied) {
        let object = supplied.as_object().ok_or("each mode must be an object")?;
        if object.keys().any(|key| !matches!(key.as_str(), "ratio" | "magnitude" | "decaySeconds")) {
            return Err("mode has an unknown field".into());
        }
        let args = Args::new(supplied);
        *target = ProfileMode {
            ratio: args.f64("ratio")?,
            magnitude: args.f64("magnitude")?,
            decay_seconds: args.f64("decaySeconds")?,
        };
    }
    ModalProfile::new(key, hz, modes.get(..supplied.len()).unwrap_or(&[])).map_err(|error| error.to_string().into())
}

pub(crate) fn adopt(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let profile = parse_profile(args.value("profile").ok_or("profile is required")?)?;
    let source = match args.opt_text("source")?.unwrap_or("Modal") {
        "Modal" => ProfileSource::Modal,
        "Hybrid" => ProfileSource::Hybrid,
        _ => return Err("source must be Modal or Hybrid".into()),
    };
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = device_at_mut(&mut channel.devices, &path).ok_or("device is missing")?;
    let overridden = apply_profile(device, &profile, source)?;
    Ok(with_override(
        json!({
            "device": path_value(&path), "revision": PROFILE_MODEL_REVISION,
            "modes": profile.modes().len(), "referenceKey": profile.reference_key(),
            "referenceHz": profile.reference_hz(), "modelControlsReset": true, "fullModes": true
        }),
        overridden,
    ))
}
