// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for devices on tracks (ADR 0004): listing the built-in effects and instruments, adding, removing and
//! switching them, and setting their parameters. Each edit is one undo step, and a playing
//! engine hears it at once: a chain that keeps its devices keeps playing (held notes, tails).

use serde_json::{Value, json};
use suhina_device_api::{DeviceInfo, ParamKind, Role, Unit};
use suhina_engine::param_id;
use suhina_model::{AutomationLane, AutomationPoint, Device, DeviceKind, DeviceParam, Extensions};

use crate::commands::{channel_param, number, track_id, with_override};
use crate::{CommandError, ParamEdit, Session};

/// Devices one track may hold.
const MAX_DEVICES: usize = 64;

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
    let device = channel_param(session, params)?.devices.get_mut(index).ok_or("device is missing")?;
    device.set_sample_file(&file);
    Ok(json!({"device": index, "path": file, "frames": audio.frames()}))
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
        "role": match info.role { Role::Effect => "effect", Role::Instrument => "instrument" },
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

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::None => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Percent => "%",
        Unit::Balance => "balance",
        Unit::Octaves => "octaves",
    }
}

/// The devices on a track, in order, with every parameter's value for built-in ones.
pub(crate) fn track_devices(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let devices = channel_param(session, params)?.devices.clone();
    Ok(Value::Array(
        devices
            .iter()
            .enumerate()
            .map(|(index, d)| {
                let mut value = device_json(index, d);
                if matches!(d.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }) {
                    value["plugin"] = plugin_json(session.plugin_report(track, index));
                }
                value
            })
            .collect(),
    ))
}

fn device_json(index: usize, device: &Device) -> Value {
    let (kind, id) = match &device.kind {
        DeviceKind::Builtin(id) => ("builtin", Some(id.clone())),
        DeviceKind::Sd => ("sd", None),
        DeviceKind::Clap { id } => ("clap", Some(id.clone())),
        DeviceKind::Vst3 { id } => ("vst3", Some(id.clone())),
        DeviceKind::Vst2 { id } => ("vst2", Some(id.to_string())),
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
        "index": index,
        "name": device.name,
        "kind": kind,
        "id": id,
        "role": graph.as_ref().map(|graph| match graph.role {
            suhina_device_sdk::DeviceRole::Effect => "effect",
            suhina_device_sdk::DeviceRole::Instrument => "instrument",
            suhina_device_sdk::DeviceRole::MidiEffect => "midi_effect",
        }),
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
        (None, None) => return Err("id must be the id of a built-in effect or instrument, from devices.list".into()),
    };
    let info = suhina_device_registry::info(id)
        .ok_or_else(|| format!("no built-in effect or instrument {id:?}; devices.list lists them"))?;
    let definition = suhina_device_bundle::find(id).ok_or("the stock .sd definition is missing")?;
    if info.role == Role::Instrument {
        let track = track_id(&session.project, params)?;
        if session.project.tracks.iter().find(|t| t.id == track).is_some_and(|t| !t.content.notes) {
            return Err("an instrument goes on a MIDI track".into());
        }
    }
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let instrument = info.role == Role::Instrument;
    let has_instrument = channel.devices.iter().any(|device| match &device.kind {
        DeviceKind::Builtin(id) => suhina_device_registry::info(id).is_some_and(|info| info.role == Role::Instrument),
        DeviceKind::Sd => sd_role(device) == Some(suhina_device_sdk::DeviceRole::Instrument),
        _ => false,
    });
    let midi_prefix = channel
        .devices
        .iter()
        .take_while(|device| sd_role(device) == Some(suhina_device_sdk::DeviceRole::MidiEffect))
        .count();
    let position = match params.get("position") {
        None => {
            if instrument {
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
    if instrument && has_instrument {
        return Err("the track has an instrument already; remove it first".into());
    }
    if instrument && position != midi_prefix {
        return Err(format!("an instrument goes after the track's {midi_prefix} MIDI effects").into());
    }
    if has_instrument && !instrument && position <= midi_prefix {
        return Err("position 0 is the track's instrument; effects go after it".into());
    }
    let mut device = Device {
        name: info.name.to_string(),
        kind: DeviceKind::Builtin(info.id.to_string()),
        enabled: true,
        state: None,
        params: preset
            .as_ref()
            .map_or_else(Vec::new, |p| p.params.iter().map(|(id, value)| DeviceParam::new(*id, *value)).collect()),
        extensions: Extensions::default(),
        automation: Vec::new(),
    };
    suhina_device_bundle::embed(&mut device, definition.sd.as_bytes().to_vec());
    channel.devices.insert(position, device);
    Ok(json!({ "device": position, "id": info.id, "name": info.name, "preset": preset.map(|p| p.name) }))
}

pub(crate) fn add_sd(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be a .sd file path")?;
    let graph = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    if let Some(id) = graph.native_device_id().map_err(|e| e.to_string())? {
        let _prepared = graph.prepare_native_at(48000., &suhina_device_registry::create).map_err(|e| e.to_string())?;
        let mut request = params.clone();
        let object = request.as_object_mut().ok_or("parameters must be an object")?;
        object.remove("path");
        object.insert("id".into(), json!(id));
        let result = add(session, &request)?;
        let index =
            result["device"].as_u64().and_then(|i| usize::try_from(i).ok()).ok_or("missing inserted device index")?;
        let device = channel_param(session, params)?.devices.get_mut(index).ok_or("inserted device is missing")?;
        device.name = graph.name.clone();
        device.params = graph
            .parameters
            .iter()
            .filter(|p| {
                suhina_device_registry::info(id)
                    .and_then(|info| info.params.iter().find(|native| native.id == p.id))
                    .is_some_and(|native| native.default != p.default)
            })
            .map(|p| DeviceParam::new(&p.id, p.default))
            .collect();
        suhina_device_bundle::embed(device, graph.to_sd().map_err(|e| e.to_string())?.into_bytes());
        return Ok(json!({"device":index,"id":graph.id,"name":graph.name,"kind":"sd","implementation":id}));
    }
    if graph.role != suhina_device_sdk::DeviceRole::Effect {
        let track = track_id(&session.project, params)?;
        if session.project.tracks.iter().find(|t| t.id == track).is_some_and(|t| !t.content.notes) {
            return Err("a MIDI effect goes on a MIDI track".into());
        }
    }
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    if graph.role == suhina_device_sdk::DeviceRole::Instrument
        && channel.devices.iter().any(|device| match &device.kind {
            DeviceKind::Builtin(id) => {
                suhina_device_registry::info(id).is_some_and(|info| info.role == Role::Instrument)
            }
            DeviceKind::Sd => sd_role(device) == Some(suhina_device_sdk::DeviceRole::Instrument),
            _ => false,
        })
    {
        return Err("the track already has an instrument; remove it first".into());
    }
    let instrument_position = channel.devices.iter().position(|device| match &device.kind {
        DeviceKind::Builtin(id) => suhina_device_registry::info(id).is_some_and(|info| info.role == Role::Instrument),
        DeviceKind::Sd => device
            .state
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
            .is_some_and(|graph| graph.role == suhina_device_sdk::DeviceRole::Instrument),
        _ => false,
    });
    let midi_prefix = channel
        .devices
        .iter()
        .take_while(|device| {
            device.kind == DeviceKind::Sd
                && device
                    .state
                    .as_deref()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
                    .is_some_and(|graph| graph.role == suhina_device_sdk::DeviceRole::MidiEffect)
        })
        .count();
    let max_position = if graph.role == suhina_device_sdk::DeviceRole::MidiEffect {
        instrument_position.unwrap_or(channel.devices.len())
    } else if graph.role == suhina_device_sdk::DeviceRole::Instrument {
        midi_prefix
    } else {
        channel.devices.len()
    };
    let position = match params.get("position") {
        None => max_position,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= max_position)
            .ok_or_else(|| format!("position must be from 0 to {max_position}"))?,
    };
    let values: Vec<DeviceParam> =
        graph.parameters.iter().map(|parameter| DeviceParam::new(parameter.id.clone(), parameter.default)).collect();
    let state = graph.to_sd().map_err(|e| e.to_string())?.into_bytes();
    let device = Device {
        name: graph.name.clone(),
        kind: DeviceKind::Sd,
        enabled: true,
        state: Some(state),
        params: values,
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    channel.devices.insert(position, device);
    Ok(json!({ "device": position, "id": graph.id, "name": graph.name, "kind": "sd" }))
}

pub(crate) fn update_sd(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be the newer .sd device file")?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    let native = matches!(device.kind, DeviceKind::Builtin(_)) && device.sd_definition().is_some();
    if device.kind != DeviceKind::Sd && !native {
        return Err("device.updateSd only updates .sd devices".into());
    }
    let bytes =
        if native { device.sd_definition() } else { device.state.as_deref() }.ok_or("the .sd graph is missing")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "the embedded .sd graph is invalid UTF-8")?;
    let current = suhina_device_sdk::DeviceGraph::from_sd(text).map_err(|e| e.to_string())?;
    let newer = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    if native {
        let _prepared = newer.prepare_native_at(48000., &suhina_device_registry::create).map_err(|e| e.to_string())?;
        if newer.native_device_id().map_err(|e| e.to_string())?
            != current.native_device_id().map_err(|e| e.to_string())?
        {
            return Err("the native implementation must stay the same".into());
        }
    }
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
    if native {
        device.set_sd_definition(state);
    } else {
        device.state = Some(state);
    }
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

pub(crate) fn remove(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let removed = channel.devices.remove(index);
    Ok(json!({ "removed": removed.name, "devices": channel.devices.len() }))
}

pub(crate) fn set_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    device.enabled = enabled;
    Ok(json!({ "device": index, "enabled": enabled }))
}

pub(crate) fn set_param(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id, from devices.list")?;
    let value = number(params, "value")?;
    let track = track_id(&session.project, params)?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    if channel.devices.get(index).is_some_and(|d| is_plugin(&d.kind)) {
        return set_plugin_param(session, params, track, index, param, value);
    }
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    let mut edits = Vec::new();
    let (param_index, value) = match &device.kind {
        DeviceKind::Builtin(id) => {
            let info =
                suhina_device_registry::info(id).ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
            let index = info
                .param_index(param)
                .ok_or_else(|| format!("{} has no parameter {param:?}; devices.list lists them", info.name))?;
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
        _ => return Err("this device has no parameters Suhina can set (it is from another program)".into()),
    };
    if edits.is_empty() {
        edits.push((param_index, param.to_string(), value));
    }
    let mut messages = Vec::new();
    let mut off = false;
    for (param_index, id, value) in edits {
        match device.params.iter_mut().find(|p| p.id == id) {
            Some(stored) => stored.value = value,
            None => device.params.push(DeviceParam::new(id.clone(), value)),
        }
        off |= crate::automation::override_lane(&mut device.automation, |l| l.param == id);
        messages.push(ParamEdit { track, device: index, param: param_index, value });
    }
    // A playing engine takes this as messages, without a new graph; one that turned a lane
    // off needs the new graph, which holds the values too.
    if !off {
        session.param_edits = messages;
    }
    Ok(with_override(json!({ "device": index, "param": param, "value": value }), off))
}

/// Sets parameter `param` (its id) of the plug-in device at `index` on `track`: kept in the
/// project, and heard from the next block of a playing engine.
fn set_plugin_param(
    session: &mut Session,
    params: &Value,
    track: suhina_model::TrackId,
    index: usize,
    param: &str,
    value: f64,
) -> Result<Value, CommandError> {
    let id = plugin_param_id(param)?;
    let value = plugin_value(session, track, index, id, value)?;
    let channel = channel_param(session, params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
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
    if !off {
        session.param_edits = vec![ParamEdit { track, device: index, param, value }];
    }
    Ok(with_override(json!({ "device": index, "param": id, "value": value }), off))
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
    let index = device_index(channel.devices.len(), params)?;
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
                .ok_or_else(|| format!("{} has no parameter {param:?}; devices.list lists them", info.name))?;
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
            None => plugin_value(session, track, index, id, value)?,
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
    index: usize,
    id: u32,
    value: f64,
) -> Result<f64, CommandError> {
    let key = crate::plugin_key_at(&session.project, track, index);
    match key.and_then(|key| session.plugins.param_range(key, id)) {
        Some(Some((min, max))) => Ok(value.clamp(min, max)),
        Some(None) => Err(format!("the plug-in has no parameter {id}; device.pluginParams lists them").into()),
        None => Ok(value),
    }
}

/// The key of the plug-in device the `track` and `device` parameters name.
fn plugin_key(session: &Session, params: &Value) -> Result<u64, CommandError> {
    let track = track_id(&session.project, params)?;
    let index = params
        .get("device")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("device must be a device index, from 0, as in track.devices")?;
    crate::plugin_key_at(&session.project, track, index)
        .ok_or_else(|| format!("device {index} on the track is not a plug-in").into())
}

/// Adds an installed plug-in from the catalog to a track.
pub(crate) fn add_plugin(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("plugin").and_then(Value::as_str).ok_or("plugin must be a plug-in name, id or file")?;
    let env = session.plugins.env().ok_or("no plug-in catalog is open; plugins.scan looks for installed ones")?;
    let plugin = env
        .catalog
        .find(name)
        .or_else(|| env.catalog.plugins().find(|p| p.spec() == name))
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugins.list lists them"))?
        .clone();
    let instrument = crate::is_instrument(&plugin);
    if instrument {
        let track = track_id(&session.project, params)?;
        if session.project.tracks.iter().find(|t| t.id == track).is_some_and(|t| !t.content.notes) {
            return Err("an instrument goes on a MIDI track".into());
        }
    }
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let position = match params.get("position") {
        None if instrument => 0,
        None => channel.devices.len(),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    if instrument && position != 0 {
        return Err("an instrument goes first in a track's chain (position 0)".into());
    }
    let device = Device {
        name: plugin.name.clone(),
        kind: crate::rack::device_kind(&plugin),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    channel.devices.insert(position, device);
    Ok(json!({
        "device": position,
        "name": plugin.name,
        "vendor": plugin.vendor,
        "format": plugin.format(),
        "instrument": instrument,
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
