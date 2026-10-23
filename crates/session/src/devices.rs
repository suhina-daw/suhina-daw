// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for devices on tracks (ADR 0004): listing the built-in effects and instruments, adding, removing and
//! switching them, and setting their parameters. Each edit is one undo step, and a playing
//! engine hears it at once: a chain that keeps its devices keeps playing (held notes, tails).

use serde_json::{Value, json};
use suhina_device_api::{DeviceInfo, ParamKind, Role, Unit};
use suhina_engine::param_id;
use suhina_model::{AutomationLane, AutomationPoint, Device, DeviceKind, DeviceParam};

use crate::commands::{channel_param, channel_read, number, track_id, with_override};
use crate::device_groups::{device_path, path_value};
use crate::{CommandError, ParamEdit, Session};
use suhina_model::{MAX_DEVICES, device_at, device_at_mut};

fn source_tempo(path: &std::path::Path, audio: &suhina_engine::AudioData, fallback: f64) -> f64 {
    crate::files::file_tempo(path, audio)
        .map(|tempo| tempo.bpm)
        .or_else(|| suhina_media::info::tempo_from_name(&path.to_string_lossy()))
        .filter(|bpm| bpm.is_finite() && *bpm > 0. && *bpm <= 999.)
        .unwrap_or(fallback.clamp(1., 999.))
}

fn initial_sampler_tempo(session: &mut Session, params: &Value, detect: bool) -> Result<Option<f64>, CommandError> {
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get(index).ok_or("device is missing")?;
    if !matches!(&device.kind, DeviceKind::Builtin(id) if id == "sampler")
        || (!detect && device.params.iter().any(|p| p.id == "source_bpm" && p.value > 0.))
    {
        return Ok(None);
    }
    let Some(file) = device.sample_file().map(str::to_owned) else {
        return Ok(None);
    };
    let beats = (!detect).then(|| device.params.iter().find(|p| p.id == "warp_beats").map(|p| p.value)).flatten();
    let start = device.params.iter().find(|p| p.id == "start").map_or(0., |p| p.value);
    let end = device.params.iter().find(|p| p.id == "end").map_or(1., |p| p.value);
    let Some(audio) = session.audio().files.get(&file) else {
        return Ok(None);
    };
    let duration = audio.frames() as f64 / f64::from(audio.sample_rate.max(1));
    let fallback = session.player().map_or(session.project().transport.tempo_bpm, |p| p.monitor().tempo_bpm());
    let bpm = beats
        .filter(|_| duration > 0. && end > start)
        .map(|beats| (beats * 60. / (duration * (end - start))).clamp(1., 999.))
        .unwrap_or_else(|| source_tempo(std::path::Path::new(&file), audio, fallback));
    Ok(Some(bpm))
}

pub(crate) fn load_sample(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be an audio file")?;
    let path = crate::files::absolute(std::path::Path::new(path));
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    if !channel.devices.get(index).is_some_and(|d| matches!(&d.kind, DeviceKind::Builtin(id) if id == "sampler")) {
        return Err("loadSample needs a Sampler instrument".into());
    }
    let audio = session.decoded_audio(&path)?;
    if audio.frames() < 2 || !(1..=32).contains(&audio.channels) || !(1000..=384000).contains(&audio.sample_rate) {
        return Err("sample needs at least two frames, 1..32 channels and a sample rate of 1000..384000 Hz".into());
    }
    let file = path.display().to_string();
    let fallback = session.player().map_or(session.project().transport.tempo_bpm, |p| p.monitor().tempo_bpm());
    let bpm = source_tempo(&path, &audio, fallback);
    let beats = (audio.frames() as f64 / f64::from(audio.sample_rate) * bpm / 60.).clamp(0.25, 1024.);
    let device = channel_param(session, params)?.devices.get_mut(index).ok_or("device is missing")?;
    device.set_sample_file(&file);
    for (id, value) in [("warp_beats", beats), ("source_bpm", bpm)] {
        if let Some(p) = device.params.iter_mut().find(|p| p.id == id) {
            p.value = value;
        } else {
            device.params.push(DeviceParam::new(id, value));
        }
    }
    Ok(json!({"device": index, "path": file, "frames": audio.frames(), "source_bpm": bpm}))
}

pub(crate) fn set_sample_slices(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let values =
        params.get("points").and_then(Value::as_array).ok_or("points must be a list of normalized positions")?;
    if values.len() > suhina_model::MAX_SAMPLE_SLICES {
        return Err("slice map exceeds the input memory budget".into());
    }
    let mut points = Vec::with_capacity(values.len());
    for value in values {
        let point = value
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0. && *v < 1.)
            .ok_or("slice positions must be finite and strictly between 0 and 1")?;
        points.push(point);
    }
    points.sort_unstable_by(f64::total_cmp);
    points.dedup();
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get_mut(index).ok_or("device is missing")?;
    if !matches!(&device.kind, DeviceKind::Builtin(id) if id == "sampler") {
        return Err("slice editing needs a Sampler instrument".into());
    }
    device.set_sample_slices(&points)?;
    Ok(json!({"points":points}))
}

fn sd_role(device: &Device) -> Option<suhina_device_sdk::DeviceRole> {
    (device.kind == DeviceKind::Sd).then_some(())?;
    let bytes = device.state.as_deref()?;
    let text = std::str::from_utf8(bytes).ok()?;
    suhina_device_sdk::DeviceGraph::from_sd(text).ok().map(|graph| graph.role)
}

/// What a `.sd` graph's role is as a device role.
fn role_of_sd(role: suhina_device_sdk::DeviceRole) -> Role {
    match role {
        suhina_device_sdk::DeviceRole::Effect => Role::Effect,
        suhina_device_sdk::DeviceRole::Instrument => Role::Instrument,
        suhina_device_sdk::DeviceRole::MidiEffect => Role::MidiEffect,
    }
}

/// The role a built-in or `.sd` device plays in a chain; `None` for a plug-in or an unknown id.
fn chain_role(device: &Device) -> Option<Role> {
    match &device.kind {
        DeviceKind::Builtin(id) => suhina_device_registry::info(id).map(|info| info.role),
        DeviceKind::Sd => sd_role(device).map(role_of_sd),
        _ => None,
    }
}

/// The track's instrument (or instrument group, a plug-in one too) in its chain: where it is
/// and its name. An instrument added to the track takes its place, one undo step: a track
/// plays one instrument.
fn instrument_in(session: &Session, params: &Value) -> Result<Option<(usize, String)>, CommandError> {
    use crate::device_groups::{Kind, kind_of};
    let devices = &channel_read(session, params)?.devices;
    let at = devices.iter().position(|d| kind_of(session, d) == Kind::Instrument);
    Ok(at.and_then(|at| devices.get(at).map(|d| (at, d.name.clone()))))
}

/// Takes the instrument `replaced` names out of `devices`; the place the new one goes.
fn take_out(devices: &mut Vec<Device>, replaced: Option<&(usize, String)>, position: usize) -> usize {
    match replaced {
        Some((at, _)) if *at < devices.len() => {
            devices.remove(*at);
            *at
        }
        _ => position,
    }
}

/// How many MIDI effects lead a chain: the instrument goes right after them.
fn midi_prefix(devices: &[Device]) -> usize {
    devices.iter().take_while(|device| chain_role(device) == Some(Role::MidiEffect)).count()
}

/// The first position an audio effect may take on the channel `params` names: after the MIDI
/// effects that lead the chain and the instrument right after them, whether or not there is one.
/// An instrument group and a plug-in instrument count as the instrument, as a built-in one does.
fn first_effect_position(session: &Session, params: &Value) -> Result<usize, CommandError> {
    use crate::device_groups::{Kind, kind_of};
    let devices = &channel_read(session, params)?.devices;
    let prefix = midi_prefix(devices);
    Ok(prefix + usize::from(devices.get(prefix).is_some_and(|d| kind_of(session, d) == Kind::Instrument)))
}

/// Refuses an audio effect at `position` before the track's MIDI effects or its instrument
/// (`first`, from [`first_effect_position`]), in a chain of `len` devices.
fn check_effect_position(first: usize, len: usize, position: usize) -> Result<(), CommandError> {
    if position < first {
        return Err(format!(
            "an effect goes after the track's MIDI effects and instrument (position {first} to {len})"
        )
        .into());
    }
    Ok(())
}

pub(crate) fn sd_graph(device: &Device) -> Result<suhina_device_sdk::DeviceGraph, CommandError> {
    let bytes = device.state.as_deref().ok_or("the .sd graph is missing")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "the .sd graph is invalid UTF-8")?;
    suhina_device_sdk::DeviceGraph::from_sd(text).map_err(|e| e.to_string().into())
}

/// The built-in effects and instruments with their parameters.
pub(crate) fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(suhina_device_registry::DEVICES.iter().map(|info| info_json(info)).collect()))
}

/// Discoverable SDK node contracts, available through UI/CLI/MCP's shared commands.
pub(crate) fn sd_nodes(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({
        "format": suhina_device_sdk::FORMAT,
        "version": suhina_device_sdk::VERSION,
        "roles": ["effect", "midi_effect", "instrument"],
        "signal": "audio graphs process stereo f64 at sample rate; midi_effect graphs process timestamped NoteEvent streams",
        "limits": {
            "file_bytes": suhina_device_sdk::MAX_FILE_BYTES,
            "nodes": suhina_device_sdk::MAX_NODES,
            "connections": suhina_device_sdk::MAX_CONNECTIONS,
            "parameters": suhina_device_sdk::MAX_PARAMETERS,
            "delay_frames": suhina_device_sdk::MAX_DELAY_FRAMES,
            "budget_sample_rate": 384000
        },
        "waveforms": ["sine", "triangle", "saw_up", "saw_down", "square", "random"],
        "filter_modes": ["lowpass", "highpass", "bandpass", "peak", "low_shelf", "high_shelf"],
        "custom_ui": {
            "defaults": suhina_device_sdk::ui::DeviceUi::default(),
            "drawing": ["rect", "circle", "line", "path", "arc", "text", "image", "gradient"],
            "expressions": ["parameter", "add", "multiply", "sin", "cos", "equal", "exp", "log"],
            "gestures": ["drag", "position", "toggle", "momentary", {"set": 3}, "menu"],
            "image_formats": ["png", "jpeg"],
            "image_bytes": suhina_device_sdk::ui::MAX_ASSET_BYTES,
            "total_image_bytes": suhina_device_sdk::ui::MAX_ASSETS_BYTES,
            "image_pixels": suhina_device_sdk::ui::MAX_IMAGE_PIXELS,
            "draw_commands": suhina_device_sdk::ui::MAX_DRAW_COMMANDS,
            "reference": "docs/spec/sdk-custom-ui.md"
        },
        "parameter_ui_defaults": suhina_device_sdk::ParameterUi::default(),
        "label_positions": ["left", "top", "hidden"],
        "control_shapes": ["rounded", "square", "pill"],
        "controls": ["slider", "log_slider", "integer", "toggle", "knob", "log_knob", "number", "vertical_slider", "button", "choice", "radio"],
        "nodes": suhina_device_sdk::NODE_CATALOG,
    }))
}

fn info_json(info: &DeviceInfo) -> Value {
    json!({
        "id": info.id,
        "name": info.name,
        "role": role_name(info.role),
        "params": info.params.iter().map(|p| {
            let mut v = json!({
                "id": p.id,
                "name": p.name,
                "min": p.min,
                "max": p.max,
                "default": p.default,
                "unit": unit_name(p.unit),
            });
            let kind = match p.kind {
                ParamKind::Continuous(_) => "number",
                ParamKind::Toggle => "toggle",
                ParamKind::Integer => "integer",
                ParamKind::Choice(options) => {
                    v["options"] = json!(options);
                    "choice"
                }
            };
            v["kind"] = json!(kind);
            v
        }).collect::<Vec<_>>(),
    })
}

/// A device role as commands name it: `effect`, `instrument` or `midi_effect`.
pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Effect => "effect",
        Role::Instrument => "instrument",
        Role::MidiEffect => "midi_effect",
    }
}

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::None => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Percent => "%",
        Unit::Balance => "balance",
        Unit::Octaves => "octaves",
        Unit::Semitones => "semitones",
        Unit::ScaleDegrees => "scale degrees",
        Unit::Key => "key",
    }
}

/// The devices on a track, in order, with every parameter's value for built-in ones.
pub(crate) fn track_devices(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let devices = channel_param(session, params)?.devices.clone();
    // Plug-in reports first, by path, so the listing below can be made without the session.
    let mut reports = Vec::new();
    for (path, device) in suhina_model::walk_devices(&devices) {
        if is_plugin(&device.kind) {
            reports.push((path.clone(), plugin_json(session.plugin_report_at(track, &path))));
        }
    }
    fn listed(path: &[usize], device: &Device, reports: &[(Vec<usize>, Value)]) -> Value {
        let mut value = device_json(path, device);
        if let Some((_, report)) = reports.iter().find(|(p, _)| p == path) {
            value["plugin"] = report.clone();
        }
        if let Some(group) = device.as_group() {
            let inner = |path: &[usize], d: &Device| listed(path, d, reports);
            let fields = crate::device_groups::group_json(group, device, path, &inner);
            if let (Some(value), Value::Object(fields)) = (value.as_object_mut(), fields) {
                value.extend(fields);
            }
        }
        value
    }
    Ok(Value::Array(devices.iter().enumerate().map(|(index, d)| listed(&[index], d, &reports)).collect()))
}

fn device_json(path: &[usize], device: &Device) -> Value {
    let (kind, id) = match &device.kind {
        DeviceKind::Builtin(id) => ("builtin", Some(id.clone())),
        DeviceKind::Sd => ("sd", None),
        DeviceKind::Clap { id } => ("clap", Some(id.clone())),
        DeviceKind::Vst3 { id } => ("vst3", Some(id.clone())),
        DeviceKind::Vst2 { id } => ("vst2", Some(id.to_string())),
        DeviceKind::Group(_) => ("group", None),
        _ => ("other", None),
    };
    let info = id.as_deref().filter(|_| kind == "builtin").and_then(suhina_device_registry::info);
    let graph = (kind == "sd")
        .then(|| {
            device
                .state
                .as_deref()
                .and_then(|state| std::str::from_utf8(state).ok())
                .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
        })
        .flatten();
    let params = info.map(|info| {
        info.params
            .iter()
            .map(|p| {
                let value = device.params.iter().find(|v| v.id == p.id).map_or(p.default, |v| p.clamp(v.value));
                json!({ "id": p.id, "value": value })
            })
            .collect::<Vec<_>>()
    });
    let params = params.or_else(|| {
        graph.as_ref().map(|graph| {
            graph
                .parameters
                .iter()
                .map(|p| {
                    let value =
                        device.params.iter().find(|stored| stored.id == p.id).map_or(p.default, |stored| p.clamp(stored.value));
                    json!({ "id": p.id, "name": p.name, "value": value, "min": p.min, "max": p.max, "control": p.control, "unit": p.unit, "ui": p.ui, "smoothing_ms": p.smoothing_ms })
                })
                .collect::<Vec<_>>()
        })
    });
    // A plug-in's: the values set on it, by parameter id (the rest are as its state says).
    let params = params.or_else(|| {
        is_plugin(&device.kind).then(|| {
            device
                .params
                .iter()
                .filter_map(|p| Some(json!({ "id": param_id(&p.id)?, "value": p.value })))
                .collect::<Vec<_>>()
        })
    });
    let mut value = json!({
        "index": path.last().copied().unwrap_or(0),
        "path": path_value(path),
        "name": device.name,
        "kind": kind,
        "id": id,
        "role": graph.as_ref().map(|graph| role_of_sd(graph.role)).or(info.map(|info| info.role)).map(role_name),
        "enabled": device.enabled,
        "known": kind != "builtin" || info.is_some(),
        "params": params,
        "nodes": graph.as_ref().map(|graph| graph.ordered_nodes().iter().filter(|node| node.visible).map(|node| {
            let kind = node.kind.info().name;
            json!({ "id": node.id, "kind": kind })
        }).collect::<Vec<_>>()),
    });
    if let Some(points) = device.sample_slices() {
        value["sample_slices"] = json!(points);
    }
    if !device.automation.is_empty() {
        value["automation"] = device
            .automation
            .iter()
            .map(|lane| {
                let points: Vec<[f64; 2]> = lane.points.iter().map(|p| [p.beat, p.value]).collect();
                json!({ "param": param_id(&lane.param), "points": points })
            })
            .collect();
    }
    value
}

pub(crate) fn is_plugin(kind: &DeviceKind) -> bool {
    matches!(kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. })
}

/// The `device` index parameter, checked against the track's devices.
pub(crate) fn device_index(len: usize, params: &Value) -> Result<usize, CommandError> {
    let index = params
        .get("device")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("device must be a device index, from 0, as in track.devices")?;
    if index >= len {
        return Err(format!("the track has no device {index}; it has {len}").into());
    }
    Ok(index)
}

pub(crate) fn add(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let preset = match params.get("preset") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let path = v.as_str().ok_or("preset must be a preset file's path")?;
            let voice = match params.get("voice") {
                None | Some(Value::Null) => 1,
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n >= 1)
                    .ok_or("voice must be a whole number from 1")?,
            };
            Some(read_preset(std::path::Path::new(path), voice)?)
        }
    };
    let id = match (params.get("id").and_then(Value::as_str), &preset) {
        (Some(id), Some(p)) if id != p.device => {
            return Err(format!("the preset is for {:?}, not {id:?}", p.device).into());
        }
        (Some(id), _) => id,
        (None, Some(p)) => p.device,
        (None, None) => return Err("id must be the id of a built-in effect or instrument, from device.list".into()),
    };
    let info = suhina_device_registry::info(id)
        .ok_or_else(|| format!("no built-in effect or instrument {id:?}; device.list lists them"))?;
    let params_of_preset = preset
        .as_ref()
        .map_or_else(Vec::new, |p| p.params.iter().map(|(id, value)| DeviceParam::new(*id, *value)).collect());
    let device =
        Device { params: params_of_preset, ..Device::new(info.name, DeviceKind::Builtin(info.id.to_string())) };
    if let Some(value) = crate::device_groups::insert_into_group(session, params, device.clone())? {
        return Ok(json!({ "device": value, "id": info.id, "name": info.name, "preset": preset.map(|p| p.name) }));
    }
    let instrument = info.role == Role::Instrument;
    let midi_effect = info.role == Role::MidiEffect;
    if instrument || midi_effect {
        let track = track_id(&session.project, params)?;
        if session.project.track(track).is_some_and(|t| !t.content.notes) {
            let what = if instrument { "an instrument" } else { "a MIDI effect" };
            return Err(format!("{what} goes on a MIDI track").into());
        }
    }
    let replaced = if instrument { instrument_in(session, params)? } else { None };
    let first_effect = first_effect_position(session, params)?;
    let channel = channel_param(session, params)?;
    if suhina_model::count_devices(&channel.devices) >= MAX_DEVICES && replaced.is_none() {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let midi_prefix = midi_prefix(&channel.devices);
    let position = match params.get("position") {
        None => {
            if instrument || midi_effect {
                midi_prefix
            } else {
                channel.devices.len()
            }
        }
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    if instrument && position != midi_prefix {
        return Err(format!("an instrument goes after the track's {midi_prefix} MIDI effects").into());
    }
    if midi_effect && position > midi_prefix {
        return Err(format!(
            "a MIDI effect goes before the instrument and the audio effects (position 0 to {midi_prefix})"
        )
        .into());
    }
    if !instrument && !midi_effect {
        check_effect_position(first_effect, channel.devices.len(), position)?;
    }
    let position = take_out(&mut channel.devices, replaced.as_ref(), position);
    channel.devices.insert(position, device);
    Ok(json!({
        "device": position, "id": info.id, "name": info.name, "preset": preset.map(|p| p.name),
        "replaced": replaced.map(|(_, name)| name),
    }))
}

pub(crate) fn add_sd(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be a .sd file path")?;
    let graph = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    let values: Vec<DeviceParam> =
        graph.parameters.iter().map(|parameter| DeviceParam::new(parameter.id.clone(), parameter.default)).collect();
    let state = graph.to_sd().map_err(|e| e.to_string())?.into_bytes();
    let device = Device { state: Some(state), params: values, ..Device::new(graph.name.clone(), DeviceKind::Sd) };
    if let Some(value) = crate::device_groups::insert_into_group(session, params, device.clone())? {
        return Ok(json!({ "device": value, "id": graph.id, "name": graph.name, "kind": "sd" }));
    }
    if graph.role != suhina_device_sdk::DeviceRole::Effect {
        let track = track_id(&session.project, params)?;
        if session.project.track(track).is_some_and(|t| !t.content.notes) {
            return Err("a MIDI effect goes on a MIDI track".into());
        }
    }
    let instrument = graph.role == suhina_device_sdk::DeviceRole::Instrument;
    let replaced = if instrument { instrument_in(session, params)? } else { None };
    let first_effect = first_effect_position(session, params)?;
    let channel = channel_param(session, params)?;
    if suhina_model::count_devices(&channel.devices) >= MAX_DEVICES && replaced.is_none() {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    // A MIDI effect or an instrument goes right after the MIDI effects that lead the chain, an
    // effect at the end.
    let midi_prefix = midi_prefix(&channel.devices);
    let position = match params.get("position") {
        None if graph.role == suhina_device_sdk::DeviceRole::Effect => channel.devices.len(),
        None => midi_prefix,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    match graph.role {
        suhina_device_sdk::DeviceRole::Instrument if position != midi_prefix => {
            return Err(format!(
                "an instrument goes after the track's {midi_prefix} MIDI effects (position {midi_prefix})"
            )
            .into());
        }
        suhina_device_sdk::DeviceRole::MidiEffect if position > midi_prefix => {
            return Err(format!(
                "a MIDI effect goes before the instrument and the audio effects (position 0 to {midi_prefix})"
            )
            .into());
        }
        suhina_device_sdk::DeviceRole::Effect => check_effect_position(first_effect, channel.devices.len(), position)?,
        _ => {}
    }
    let position = take_out(&mut channel.devices, replaced.as_ref(), position);
    channel.devices.insert(position, device);
    Ok(json!({
        "device": position, "id": graph.id, "name": graph.name, "kind": "sd",
        "replaced": replaced.map(|(_, name)| name),
    }))
}

pub(crate) fn update_sd(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be the newer .sd device file")?;
    let channel = channel_param(session, params)?;
    let at = device_path(&channel.devices, params, "device")?;
    let index = path_value(&at);
    let device = device_at_mut(&mut channel.devices, &at).ok_or("the device is missing")?;
    if device.kind != DeviceKind::Sd {
        return Err("device.updateSd only updates .sd devices".into());
    }
    let bytes = device.state.as_deref().ok_or("the .sd graph is missing")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "the embedded .sd graph is invalid UTF-8")?;
    let current = suhina_device_sdk::DeviceGraph::from_sd(text).map_err(|e| e.to_string())?;
    let newer = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    if current.id != newer.id || current.role != newer.role {
        return Err("the .sd file must have the same device id and role".into());
    }
    if newer.device_version <= current.device_version {
        return Err("the .sd file must have a newer device version".into());
    }
    let values = device.params.iter().map(|parameter| (parameter.id.clone(), parameter.value)).collect::<Vec<_>>();
    let migrated = newer.migrate_parameter_values(current.device_version, &values).map_err(|e| e.to_string())?;
    let state = newer.to_sd().map_err(|e| e.to_string())?.into_bytes();
    let from_version = current.device_version;
    device.name = newer.name.clone();
    device.state = Some(state);
    device.params = migrated.into_iter().map(|(id, value)| DeviceParam::new(id, value)).collect();
    Ok(
        json!({ "device": index, "id": newer.id, "name": newer.name, "from_version": from_version, "to_version": newer.device_version }),
    )
}

/// Reads voice `voice` of the preset file at `path`.
fn read_preset(path: &std::path::Path, voice: usize) -> Result<suhina_device_registry::Preset, CommandError> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > suhina_device_registry::MAX_PRESET_BYTES as u64 {
        return Err(format!("{}: a preset file is at most 64 KiB", path.display()).into());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    suhina_device_registry::preset(&data, voice).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Removes the device at `path` from `devices`.
fn remove_at(devices: &mut Vec<Device>, path: &[usize]) -> Option<Device> {
    let (&index, parent) = path.split_last()?;
    let list = crate::device_groups::container_mut(devices, parent)?;
    (index < list.len()).then(|| list.remove(index))
}

pub(crate) fn remove(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let channel = channel_param(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let removed = remove_at(&mut channel.devices, &path).ok_or("the device is missing")?;
    Ok(json!({ "removed": removed.name, "devices": channel.devices.len() }))
}

/// The device paths `params.devices` names among `devices`, each once and none inside another
/// one named, last first (so removing them in this order keeps the others' paths).
fn device_list(devices: &[Device], params: &Value) -> Result<Vec<Vec<usize>>, CommandError> {
    let list = params.get("devices").and_then(Value::as_array).ok_or("devices must be a list of device paths")?;
    let mut out: Vec<Vec<usize>> = Vec::with_capacity(list.len());
    for value in list {
        let path = crate::device_groups::parse_path(value)
            .filter(|p| p.len() % 2 == 1 && device_at(devices, p).is_some())
            .ok_or("each device must be a device index or path on the track, as in track.devices")?;
        if !out.contains(&path) {
            out.push(path);
        }
    }
    if out.is_empty() {
        return Err("devices must name at least one device".into());
    }
    let named = out.clone();
    out.retain(|p| !named.iter().any(|other| other.len() < p.len() && p.starts_with(other)));
    out.sort_unstable_by(|a, b| b.cmp(a));
    Ok(out)
}

/// `device.removeMany {track, devices}`: removes several devices of a track as one undo step.
pub(crate) fn remove_many(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let channel = channel_param(session, params)?;
    let paths = device_list(&channel.devices, params)?;
    let removed: Vec<String> =
        paths.iter().filter_map(|p| remove_at(&mut channel.devices, p)).map(|d| d.name).collect();
    Ok(json!({ "removed": removed, "devices": channel.devices.len() }))
}

/// `device.copy {track, devices}`: copies devices, with their parameters, automation and a
/// running plug-in's state as it is now, for `device.paste`.
pub(crate) fn copy(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let (heard, _) = session.heard();
    let channel = heard
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .and_then(|c| heard.channel(c))
        .ok_or("the track has no mixer channel")?;
    let mut paths = device_list(&channel.devices, params)?;
    paths.reverse();
    let copied: Vec<Device> = paths.iter().filter_map(|p| device_at(&channel.devices, p).cloned()).collect();
    let names: Vec<String> = copied.iter().map(|d| d.name.clone()).collect();
    session.clipboards.devices = copied;
    Ok(json!({ "copied": names }))
}

/// `device.duplicateMany {track, devices}`: copies of several devices, in their order, right
/// after the last of them, as one undo step; a running plug-in's copy starts with its state as it
/// is now. The device clipboard stays as it was (#685). The devices must share a chain or branch.
/// The chain's rules hold for the result (one instrument, MIDI effects first); a copy that would
/// break them is refused (#803).
pub(crate) fn duplicate_many(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let (heard, _) = session.heard();
    let heard_devices = heard
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .and_then(|c| heard.channel(c))
        .map(|c| c.devices.clone());
    let own = channel_read(session, params)?.devices.clone();
    let mut paths = device_list(&own, params)?;
    paths.reverse();
    let copies: Vec<Device> = paths
        .iter()
        .map(|p| {
            heard_devices
                .as_deref()
                .and_then(|h| device_at(h, p))
                .or_else(|| device_at(&own, p))
                .cloned()
                .ok_or_else(|| CommandError::from("a device is missing"))
        })
        .collect::<Result<_, _>>()?;
    let last = paths.last().ok_or("devices must name at least one device")?;
    let (&index, parent) = last.split_last().ok_or("a device path is empty")?;
    // The copies go in one place, so the picks must share it: a copy of a top-level device does
    // not belong inside a group's branch.
    if paths.iter().any(|p| p.split_last().is_none_or(|(_, other)| other != parent)) {
        return Err("the devices must be in the same chain or branch".into());
    }
    let count = copies.len();
    crate::device_groups::edit_chain(session, params, |devices| {
        let list = crate::device_groups::container_mut(devices, parent).ok_or("the device is missing")?;
        for (offset, copy) in copies.into_iter().enumerate() {
            list.insert(index + 1 + offset, copy);
        }
        let made: Vec<Value> = (index + 1..index + 1 + count)
            .map(|i| {
                let mut path = parent.to_vec();
                path.push(i);
                path_value(&path)
            })
            .collect();
        Ok(json!({ "devices": made }))
    })
}

/// `device.moveToTrack {track, device, to_track, copy?}`: a device (a group with everything in
/// it) dragged onto another track (#688): moved there, or with `copy` copied, as one undo step.
/// It goes where its kind goes on that track: an instrument or a MIDI effect after the MIDI
/// effects that lead the chain (on a track that plays notes), an effect at the end. A running
/// plug-in's state goes with it as it is now.
pub(crate) fn move_to_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from = track_id(&session.project, params)?;
    let to = crate::commands::track_at(&session.project, params, "toTrack")?;
    if from == to {
        return Err("the device is on that track already; device.move moves it along the chain".into());
    }
    let copy = match params.get("copy") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("copy must be true or false")?,
    };
    let channel_of = |p: &suhina_model::Project, track| {
        p.track(track).and_then(|t| t.channel).ok_or("the track has no mixer channel")
    };
    let (source, target) = (channel_of(&session.project, from)?, channel_of(&session.project, to)?);
    let (heard, _) = session.heard();
    let heard_devices = heard.channel(source).map(|c| c.devices.clone());
    let devices = &session.project.channel(source).ok_or("the track's channel is missing")?.devices;
    let path = device_path(devices, params, "device")?;
    let device = heard_devices
        .as_deref()
        .and_then(|h| device_at(h, &path))
        .or_else(|| device_at(devices, &path))
        .cloned()
        .ok_or("the device is missing")?;
    // A group is what its branches start with (`kind_of`); a single device is its own role, or a
    // plug-in's in the catalog.
    let grouped = crate::device_groups::kind_of(session, &device);
    let instrument = grouped == crate::device_groups::Kind::Instrument
        || match chain_role(&device) {
            Some(role) => role == Role::Instrument,
            None => session
                .plugins
                .env()
                .and_then(|env| crate::device_plugin(&env.catalog, &device))
                .is_some_and(|(found, _)| crate::is_instrument(found.plugin)),
        };
    let midi_effect = !instrument
        && (grouped == crate::device_groups::Kind::MidiEffect || chain_role(&device) == Some(Role::MidiEffect));
    let plays_notes = session.project.track(to).is_some_and(|t| t.content.notes);
    if (instrument || midi_effect) && !plays_notes {
        let what = if instrument { "an instrument" } else { "a MIDI effect" };
        return Err(format!("{what} goes on a MIDI track").into());
    }
    let replaced = if instrument { instrument_in(session, &json!({ "track": to.0 }))? } else { None };
    let chain = &session.project.channel(target).ok_or("the track's channel is missing")?.devices;
    let leaving = replaced
        .as_ref()
        .and_then(|(at, _)| chain.get(*at))
        .map_or(0, |d| suhina_model::count_devices(std::slice::from_ref(d)));
    if suhina_model::count_devices(chain) + suhina_model::count_devices(std::slice::from_ref(&device)) - leaving
        > MAX_DEVICES
    {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let position = if instrument || midi_effect { midi_prefix(chain) } else { chain.len() };
    if !copy {
        let devices = &mut session
            .project
            .channels
            .iter_mut()
            .find(|c| c.id == source)
            .ok_or("the track's channel is missing")?
            .devices;
        remove_at(devices, &path).ok_or("the device is missing")?;
    }
    let chain = &mut session
        .project
        .channels
        .iter_mut()
        .find(|c| c.id == target)
        .ok_or("the track's channel is missing")?
        .devices;
    let position = take_out(chain, replaced.as_ref(), position);
    chain.insert(position, device);
    Ok(json!({ "track": to.0, "device": position, "copied": copy, "replaced": replaced.map(|(_, name)| name) }))
}

/// `device.paste {track, position?}`: puts the copied devices into a track's chain at
/// `position` (the end when left out), in the order they were copied, as one undo step.
pub(crate) fn paste(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if session.clipboards.devices.is_empty() {
        return Err("no devices have been copied".into());
    }
    let copied = session.clipboards.devices.clone();
    let count = copied.len();
    crate::device_groups::edit_chain(session, params, |devices| {
        let (parent, position) = crate::device_groups::position_path(devices, params, "position")?
            .unwrap_or_else(|| (Vec::new(), devices.len()));
        let list = crate::device_groups::container_mut(devices, &parent).ok_or("the position is missing")?;
        for (offset, device) in copied.into_iter().enumerate() {
            list.insert(position + offset, device);
        }
        let pasted: Vec<Value> = (position..position + count)
            .map(|i| {
                let mut path = parent.clone();
                path.push(i);
                path_value(&path)
            })
            .collect();
        Ok(json!({ "devices": pasted }))
    })
}

pub(crate) fn set_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let channel = channel_param(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = device_at_mut(&mut channel.devices, &path).ok_or("the device is missing")?;
    device.enabled = enabled;
    Ok(json!({ "device": path_value(&path), "enabled": enabled }))
}

/// `device.setNamFile` ([docs/spec/nam-amp-and-cab.md]): the file a NAM Amp Modeler or a NAM
/// Cab Loader runs. The device's state becomes the file's absolute path; the parsed model or
/// decoded IR goes into the session's assets with this command, so the playing engine hears it
/// with the next edit. One undo step, like the other device edits.
pub(crate) fn set_file(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .ok_or("path must be a .nam model or a .wav IR file's path")?;
    // The device's kind first, from a read; no mutation until the file has loaded.
    let kind = {
        let track = track_id(&session.project, params)?;
        let channel = session
            .project
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.channel)
            .ok_or("the track has no mixer channel")?;
        let channel = session.project.channel(channel).ok_or("the track's channel is missing")?;
        let path = device_path(&channel.devices, params, "device")?;
        match device_at(&channel.devices, &path) {
            Some(device) => match &device.kind {
                DeviceKind::Builtin(id) if suhina_device_registry::is_file_device(id) => id.clone(),
                _ => return Err("the device does not run a file".into()),
            },
            None => return Err("the device is missing".into()),
        }
    };
    let want_extension = match kind.as_str() {
        "nam-amp-modeler" => "nam",
        "nam-cab-loader" => "wav",
        _ => return Err("the device does not run a file".into()),
    };
    if !path.to_lowercase().ends_with(&format!(".{want_extension}")) {
        return Err(format!("a {kind} runs a .{want_extension} file").into());
    }
    let absolute = crate::files::absolute(std::path::Path::new(path));
    let absolute_text = absolute.display().to_string();
    let asset = session.load_device_file(&kind, &absolute_text).map_err(CommandError)?;
    let name = match &asset {
        suhina_device_registry::DeviceAsset::Nam(model) => {
            suhina_device_nam_amp_modeler::model::display_name(model).unwrap_or_default()
        }
        suhina_device_registry::DeviceAsset::Ir(_) => absolute
            .parent()
            .and_then(|folder| folder.file_name())
            .map_or_else(String::new, |folder| folder.to_string_lossy().into_owned()),
    };
    let channel = channel_param(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = device_at_mut(&mut channel.devices, &path).ok_or("the device is missing")?;
    device.state = Some(absolute_text.as_bytes().to_vec());
    Ok(json!({ "device": path_value(&path), "file": absolute_text, "name": name }))
}

/// `device.file`: the file a NAM device runs — its absolute path (empty when none is set) and
/// the model's own name or the cab's (the IR's parent folder's name), for a device panel.
pub(crate) fn device_file(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let channel = channel_read(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = device_at(&channel.devices, &path).ok_or("the device is missing")?;
    let kind = match &device.kind {
        DeviceKind::Builtin(id) => id.clone(),
        _ => return Err("the device does not run a file".into()),
    };
    if !suhina_device_registry::is_file_device(&kind) {
        return Err("the device does not run a file".into());
    }
    let file = device.state.as_deref().and_then(|state| std::str::from_utf8(state).ok()).unwrap_or_default();
    let name = if file.is_empty() { String::new() } else { session.device_asset_name(file).unwrap_or_default() };
    Ok(json!({ "file": file, "name": name }))
}

pub(crate) fn set_param(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id, from device.list")?;
    let mut value = number(params, "value")?;
    let source_tempo =
        if param == "warp" && value > 0. { initial_sampler_tempo(session, params, false)? } else { None };
    if param == "source_bpm" && value == 0. {
        value = initial_sampler_tempo(session, params, true)?.unwrap_or(0.);
    }
    let track = track_id(&session.project, params)?;
    let channel = channel_param(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    if device_at(&channel.devices, &path).is_some_and(|d| is_plugin(&d.kind)) {
        return set_plugin_param(session, params, track, &path, param, value);
    }
    let device = device_at_mut(&mut channel.devices, &path).ok_or("the device is missing")?;
    let mut edits = Vec::new();
    let (param_index, value) = match &device.kind {
        DeviceKind::Builtin(id) => {
            let info =
                suhina_device_registry::info(id).ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
            let index = info
                .param_index(param)
                .ok_or_else(|| format!("{} has no parameter {param:?}; device.list lists them", info.name))?;
            let spec = info.params.get(index).ok_or("the parameter is missing")?;
            (index, spec.clamp(value))
        }
        DeviceKind::Sd => {
            let bytes = device.state.as_deref().ok_or("the .sd graph is missing")?;
            let text = std::str::from_utf8(bytes).map_err(|_| "the .sd graph is invalid UTF-8")?;
            let graph = suhina_device_sdk::DeviceGraph::from_sd(text).map_err(|e| e.to_string())?;
            let (index, parameter) = graph
                .parameters
                .iter()
                .enumerate()
                .find(|(_, p)| p.id == param)
                .ok_or_else(|| format!("{} has no parameter {param:?}", graph.name))?;
            edits = graph
                .parameter_edits(param, value, |id| {
                    device.params.iter().find(|p| p.id == id).map_or_else(
                        || graph.parameters.iter().find(|p| p.id == id).map_or(0.0, |p| p.default),
                        |p| p.value,
                    )
                })
                .into_iter()
                .filter_map(|(i, value)| graph.parameters.get(i).map(|p| (i, p.id.clone(), value)))
                .collect();
            (index, parameter.clamp(value))
        }
        DeviceKind::Group(_) => {
            let index = suhina_engine::group_param_index(param).ok_or_else(|| {
                format!("a device group has no parameter {param:?}; it has mix, gain and macro.1 to macro.8")
            })?;
            let &(_, min, max, _) = suhina_engine::GROUP_PARAMS.get(index).ok_or("the parameter is missing")?;
            (index, value.clamp(min, max))
        }
        _ => return Err("this device has no parameters Suhina can set (it is from another program)".into()),
    };
    if edits.is_empty() {
        edits.push((param_index, param.to_string(), value));
    }
    if matches!(&device.kind, DeviceKind::Builtin(id) if id == "sampler")
        && let Some(value) = source_tempo.or((param == "warp_beats").then_some(0.))
        && let Some(index) = suhina_device_sdk::sampler::INFO.param_index("source_bpm")
    {
        edits.push((index, "source_bpm".into(), value));
    }
    let mut messages = Vec::new();
    let mut off = false;
    for (param_index, id, value) in edits {
        match device.params.iter_mut().find(|p| p.id == id) {
            Some(stored) => stored.value = value,
            None => device.params.push(DeviceParam::new(id.clone(), value)),
        }
        off |= crate::automation::override_lane(&mut device.automation, |l| l.param == id);
        if let [index] = path.as_slice() {
            messages.push(ParamEdit { track, device: *index, param: param_index, value });
        }
    }
    // A playing engine takes this as messages, without a new graph; one that turned a lane
    // off needs the new graph, which holds the values too, and so does a device inside a group.
    if !off && path.len() == 1 {
        session.sync.param_edits = messages;
    }
    Ok(with_override(json!({ "device": path_value(&path), "param": param, "value": value }), off))
}

/// Sets parameter `param` (its id) of the plug-in device at `index` on `track`: kept in the
/// project, and heard from the next block of a playing engine.
fn set_plugin_param(
    session: &mut Session,
    params: &Value,
    track: suhina_model::TrackId,
    path: &[usize],
    param: &str,
    value: f64,
) -> Result<Value, CommandError> {
    let id = plugin_param_id(param)?;
    let value = plugin_value(session, track, path, id, value)?;
    let channel = channel_param(session, params)?;
    let device = device_at_mut(&mut channel.devices, path).ok_or("the device is missing")?;
    match device.params.iter_mut().find(|p| param_id(&p.id) == Some(id)) {
        Some(stored) => {
            stored.id = id.to_string();
            stored.value = value;
        }
        None => device.params.push(DeviceParam::new(id.to_string(), value)),
    }
    let off = crate::automation::override_lane(&mut device.automation, |l| param_id(&l.param) == Some(id));
    // A plug-in's parameter goes to the engine by its id.
    let param = usize::try_from(id).map_err(|_| "the parameter id is too large")?;
    if !off && let [index] = path {
        session.sync.param_edits = vec![ParamEdit { track, device: *index, param, value }];
    }
    Ok(with_override(json!({ "device": path_value(path), "param": id, "value": value }), off))
}

/// Most points one automation lane holds.
const MAX_POINTS: usize = 65_536;

/// Sets or removes the automation lane of one parameter of a device: a plug-in's by its id, and
/// a built-in or `.sd` device's by its stable id (#395).
pub(crate) fn set_automation(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    const POINTS: &str = "points must be a list of numbers, a beat then a value for each point: [0, 0.2, 4, 0.8]";
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id")?;
    let numbers = params.get("points").and_then(Value::as_array).ok_or(POINTS)?;
    if numbers.len() % 2 != 0 {
        return Err(POINTS.into());
    }
    if numbers.len() / 2 > MAX_POINTS {
        return Err(format!("a lane holds at most {MAX_POINTS} points").into());
    }
    let track = track_id(&session.project, params)?;
    let channel = channel_param(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let [index] = path.as_slice() else {
        return Err("automation of devices inside a device group does not play yet".into());
    };
    let index = *index;
    // A built-in device's parameter: its spec, to keep values as the device takes them.
    let device = channel.devices.get(index).ok_or("the device is missing")?;
    let (builtin, sd_graph) = match &device.kind {
        kind if is_plugin(kind) => (None, None),
        DeviceKind::Builtin(id) => {
            let info =
                suhina_device_registry::info(id).ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
            let spec = info
                .param_index(param)
                .and_then(|i| info.params.get(i))
                .ok_or_else(|| format!("{} has no parameter {param:?}; device.list lists them", info.name))?;
            (Some(spec), None)
        }
        DeviceKind::Sd => {
            let graph = sd_graph(device)?;
            if !graph.parameters.iter().any(|p| p.id == param) {
                return Err(format!("{} has no parameter {param:?}", graph.name).into());
            }
            (None, Some(graph))
        }
        _ => return Err("this device has no parameters Suhina can automate (it is from another program)".into()),
    };
    let plugin = builtin.is_none() && sd_graph.is_none();
    let id = if plugin { plugin_param_id(param)? } else { 0 };
    let mut points = Vec::with_capacity(numbers.len() / 2);
    for [beat, value] in numbers.as_chunks::<2>().0 {
        let number = |v: &Value| v.as_f64().filter(|n| n.is_finite());
        let (Some(beat), Some(value)) = (number(beat), number(value)) else {
            return Err(POINTS.into());
        };
        if !(0.0..=suhina_engine::MAX_START_BEAT).contains(&beat) {
            return Err(format!("a point's beat must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
        }
        let value = match builtin {
            Some(spec) => spec.clamp(value),
            None if let Some(graph) = &sd_graph => {
                graph.parameters.iter().find(|p| p.id == param).map_or(value, |p| p.clamp(value))
            }
            None => plugin_value(session, track, &[index], id, value)?,
        };
        points.push(AutomationPoint { beat, value, curve: 0.0 });
    }
    // In beat order; points given at one beat keep their order (a jump).
    points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let count = points.len();
    let channel = channel_param(session, params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    let name = if plugin { id.to_string() } else { param.to_string() };
    device.automation.retain(|lane| if plugin { param_id(&lane.param) != Some(id) } else { lane.param != name });
    if !points.is_empty() {
        device.automation.push(AutomationLane { param: name.clone(), points, enabled: true });
    }
    let param = if plugin { json!(id) } else { json!(name) };
    Ok(json!({ "device": index, "param": param, "points": count }))
}

/// The parameters of a running plug-in device, as its plug-in lists them.
pub(crate) fn plugin_params(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    let list = session.plugins.params(key)?;
    Ok(Value::Array(
        list.iter()
            .filter(|p| !p.hidden)
            .map(|p| {
                json!({
                    "id": p.id, "name": p.name, "group": p.group, "min": p.min, "max": p.max,
                    "default": p.default, "value": p.value, "text": p.text, "steps": p.steps,
                    "automatable": p.automatable, "read_only": p.read_only,
                })
            })
            .collect(),
    ))
}

pub(crate) fn plugin_param_id(param: &str) -> Result<u32, CommandError> {
    param_id(param).ok_or_else(|| {
        format!("{param:?} is not a plug-in parameter id (a whole number); device.pluginParams lists them").into()
    })
}

/// `value` for parameter `id` of the plug-in device at `index` on `track`, limited to its range
/// when its parameters have been listed (`device.pluginParams`); as given otherwise.
pub(crate) fn plugin_value(
    session: &Session,
    track: suhina_model::TrackId,
    path: &[usize],
    id: u32,
    value: f64,
) -> Result<f64, CommandError> {
    let key = crate::plugin_key_at_path(&session.project, track, path);
    match key.and_then(|key| session.plugins.param_range(key, id)) {
        Some(Some((min, max))) => Ok(value.clamp(min, max)),
        Some(None) => Err(format!("the plug-in has no parameter {id}; device.pluginParams lists them").into()),
        None => Ok(value),
    }
}

/// The key of the plug-in device the `track` and `device` parameters name.
fn plugin_key(session: &Session, params: &Value) -> Result<u64, CommandError> {
    let track = track_id(&session.project, params)?;
    let path = device_path(&channel_read(session, params)?.devices, params, "device")?;
    crate::plugin_key_at_path(&session.project, track, &path)
        .ok_or_else(|| format!("device {} on the track is not a plug-in", path_value(&path)).into())
}

/// Adds an installed plug-in from the catalog to a track.
pub(crate) fn add_plugin(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("plugin").and_then(Value::as_str).ok_or("plugin must be a plug-in name, id or file")?;
    let env = session.plugins.env().ok_or("no plug-in catalog is open; plugin.scan looks for installed ones")?;
    let plugin = env
        .catalog
        .find(name)
        .or_else(|| env.catalog.plugins().find(|p| p.spec() == name))
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugin.list lists them"))?
        .clone();
    let instrument = crate::is_instrument(&plugin);
    let device = Device::new(plugin.name.clone(), crate::rack::device_kind(&plugin));
    if let Some(value) = crate::device_groups::insert_into_group(session, params, device.clone())? {
        return Ok(json!({
            "device": value, "name": plugin.name, "vendor": plugin.vendor, "format": plugin.format(), "instrument": instrument,
        }));
    }
    if instrument {
        let track = track_id(&session.project, params)?;
        if session.project.track(track).is_some_and(|t| !t.content.notes) {
            return Err("an instrument goes on a MIDI track".into());
        }
    }
    let replaced = if instrument { instrument_in(session, params)? } else { None };
    let first_effect = first_effect_position(session, params)?;
    let channel = channel_param(session, params)?;
    if suhina_model::count_devices(&channel.devices) >= MAX_DEVICES && replaced.is_none() {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    // An instrument goes right after the MIDI effects that lead the chain, first when there are
    // none.
    let midi_prefix = midi_prefix(&channel.devices);
    let position = match params.get("position") {
        None if instrument => midi_prefix,
        None => channel.devices.len(),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    if instrument && position != midi_prefix {
        return Err(format!(
            "an instrument goes after the track's {midi_prefix} MIDI effects (position {midi_prefix})"
        )
        .into());
    }
    if !instrument {
        check_effect_position(first_effect, channel.devices.len(), position)?;
    }
    // A plug-in that edits clips (ARA, ADR 0028) plays the clips from their files, so a device
    // before it would not be heard: it goes first, wherever it was asked for.
    let moved_first = plugin.ara && params.get("position").is_some() && position != 0;
    let position = if plugin.ara { 0 } else { position };
    let position = take_out(&mut channel.devices, replaced.as_ref(), position);
    channel.devices.insert(position, device);
    Ok(json!({
        "device": position,
        "name": plugin.name,
        "vendor": plugin.vendor,
        "format": plugin.format(),
        "instrument": instrument,
        "ara": plugin.ara,
        "moved_first": moved_first,
        "replaced": replaced.map(|(_, name)| name),
    }))
}

/// A plug-in device's state as JSON.
pub(crate) fn plugin_json(report: Option<crate::PluginReport>) -> Value {
    let Some(report) = report else { return json!({ "status": "stopped" }) };
    json!({
        "status": report.status.name(),
        "reason": report.status.reason(),
        "plugin": report.plugin.as_ref().map(|(name, vendor, how)| json!({ "name": name, "vendor": vendor, "found": how })),
        "instrument": report.instrument,
        "latency": report.latency,
        "editor_open": report.editor_open,
        "blocks": report.blocks,
        "late_blocks": report.late_blocks,
        "longest_us": report.longest_us,
        "no_time": report.no_time,
        "state_refused": report.state_refused,
        "ara": report.ara,
    })
}

pub(crate) fn plugin_status(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    Ok(plugin_json(session.plugins.report(key)))
}

pub(crate) fn open_editor(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    // Over the app's window the editor opens in the background; `device.plugin` says when.
    Ok(match session.plugins.open_editor(key)? {
        Some((width, height)) => json!({ "editor_open": true, "width": width, "height": height }),
        None => json!({ "editor_open": "opening" }),
    })
}

pub(crate) fn close_editor(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    session.plugins.close_editor(key)?;
    Ok(json!({ "editor_open": false }))
}

pub(crate) fn restart(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    session.plugins.restart(key)?;
    Ok(json!({ "status": "loading" }))
}
