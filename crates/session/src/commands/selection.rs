// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The selection's commands (`docs/spec/selection.md`, #1679): what the window, controllers,
//! mappings and agents point at — the selected track and the selected device of its chain.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "track.select",
        label: "Select a track — what controllers, mappings and agents point at — and its first device; the already-selected track changes nothing",
        params: &[DEVICE_TRACK],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_track,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.selectNext",
        label: "Select the next track in the project's order; at the end nothing happens, and with nothing selected the first track is selected",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_next,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.selectPrevious",
        label: "Select the previous track in the project's order; at the beginning nothing happens, and with nothing selected the last track is selected",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_previous,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "device.select",
        label: "Select a device of the selected track's chain, or of the named track (selecting it too), and answer with it and its parameters",
        params: &[
            DEVICE,
            optional("track", ParamType::Track, "the track to select it on (id or name), when not the selected track"),
        ],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_device,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "device.selectNext",
        label: "Select the next device of the branch the selected device sits in; nothing at its end, nothing with no device selected",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_device_next,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "device.selectPrevious",
        label: "Select the previous device of the branch the selected device sits in; nothing at its beginning, nothing with no device selected",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: select_device_previous,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.selection",
        label: "Read the selection: the selected track and the selected device, its parameters with name, range and unit — what a controller's rings draw",
        params: &[],
        kind: CommandKind::READ,
        run: selection,
        policy: Policy::DEFAULT,
    },
];

/// The selection as the commands answer it: nothing selected is null fields, and the device is
/// its `track.devices` entry — parameters with `name`, `min`, `max` and `unit` included, so a
/// knob can scale onto the unit and a ring draw the position. A running plug-in's entry is its
/// generated panel's parameter list, in the panel's order, so a controller's knobs turn the
/// sliders the window shows (#1679).
fn selected_json(session: &mut Session) -> Value {
    let mut value = json!({ "track": Value::Null, "track_name": Value::Null, "device": Value::Null });
    let Some(track) = session.selection.track() else { return value };
    value["track"] = json!(track.0);
    let name = session
        .project
        .track(track)
        .map(|_| session.project.shown_track_name(track))
        .unwrap_or_else(|| "Main".to_string());
    value["track_name"] = json!(name);
    let Some(path) = session.selection.device_path(&session.project) else { return value };
    // Its panel follows the parameters it lists; a selection read keeps that list fresh, as the
    // detail area does, so a controller maps the sliders the window shows.
    if let Some(entry) = crate::devices::device_entry(session, track, &path) {
        value["device"] = entry;
    }
    value
}

fn select_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_or_main_id(&session.project, params)?;
    session.selection.prune(&session.project);
    session.selection.select_track(&session.project, track);
    Ok(selected_json(session))
}

fn select_next(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.selection.prune(&session.project);
    session.selection.step_track(&session.project, true);
    Ok(selected_json(session))
}

fn select_previous(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.selection.prune(&session.project);
    session.selection.step_track(&session.project, false);
    Ok(selected_json(session))
}

fn select_device(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = if Args::new(params).value("track").is_some() {
        track_or_main_id(&session.project, params)?
    } else {
        session.selection.track().ok_or("no track is selected; track.select, or name the track")?
    };
    let channel = session.project.channel_of_track(track).ok_or("the track has no mixer channel")?;
    let path = crate::device_groups::device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at(&channel.devices, &path).ok_or("the device is missing")?;
    if !device.id.is_some() {
        return Err("the device has no id yet; a project gives its devices ids when it opens".into());
    }
    let id = device.id;
    session.selection.select_device(track, id);
    Ok(selected_json(session))
}

fn select_device_next(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.selection.prune(&session.project);
    session.selection.step_device(&session.project, true);
    Ok(selected_json(session))
}

fn select_device_previous(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.selection.prune(&session.project);
    session.selection.step_device(&session.project, false);
    Ok(selected_json(session))
}

fn selection(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.selection.prune(&session.project);
    Ok(selected_json(session))
}
