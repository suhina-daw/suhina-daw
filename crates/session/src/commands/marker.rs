// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `marker.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "marker.add",
        label: "Add a named marker on the arrangement's timeline",
        params: &[
            param("beat", ParamType::Number, "where, in beats from the beginning, 0 … 120000"),
            optional("name", ParamType::Text, "the marker's name (default \"Marker n\")"),
        ],
        kind: CommandKind::Edit,
        run: add_marker,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "marker.move",
        label: "Move a marker to another beat; markers stay in beat order, so its index can change",
        params: &[MARKER, param("beat", ParamType::Number, "the new beat, 0 … 120000")],
        kind: CommandKind::Edit,
        run: move_marker,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "marker.rename",
        label: "Rename a marker",
        params: &[MARKER, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: rename_marker,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "marker.delete",
        label: "Delete a marker",
        params: &[MARKER],
        kind: CommandKind::Edit,
        run: delete_marker,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "marker.jump",
        label: "Move the play start marker to a marker; while playing, playback jumps there at once",
        params: &[MARKER],
        kind: CommandKind::Transport,
        run: jump_to_marker,
        policy: Policy::DEFAULT,
    },
];

/// `beat`, checked to be a place in the arrangement.
fn marker_beat(args: Args) -> Result<f64, CommandError> {
    let beat = args.f64("beat")?;
    if !(0.0..=MAX_START_BEAT).contains(&beat) {
        return Err(format!("beat must be between 0 and {MAX_START_BEAT}, got {beat}").into());
    }
    Ok(beat)
}

/// `index`, checked to name a marker.
fn marker_index(p: &Project, args: Args) -> Result<usize, CommandError> {
    let index = args.index("index", "from project.markers")?;
    let count = p.arrangement.markers.len();
    (index < count)
        .then_some(index)
        .ok_or_else(|| format!("marker {index} does not exist; the arrangement has {count} markers").into())
}

fn add_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let beat = marker_beat(args)?;
    let markers = &mut session.project.arrangement;
    let name = match args.opt_name("name")? {
        Some(name) => name,
        None => {
            // The first "Marker n" not in use, counting from how many there are.
            let mut n = markers.markers.len() + 1;
            while markers.markers.iter().any(|m| m.name == format!("Marker {n}")) {
                n += 1;
            }
            format!("Marker {n}")
        }
    };
    let index = markers.insert_marker(Marker::new(beat, name.clone()));
    Ok(json!({ "index": index, "beat": beat, "name": name }))
}

fn move_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let index = marker_index(&session.project, args)?;
    let beat = marker_beat(args)?;
    let arrangement = &mut session.project.arrangement;
    let mut marker = arrangement.markers.remove(index);
    marker.beat = beat;
    let name = marker.name.clone();
    let index = arrangement.insert_marker(marker);
    Ok(json!({ "index": index, "beat": beat, "name": name }))
}

fn rename_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let name = args.name("name")?;
    let index = marker_index(&session.project, args)?;
    session.project.arrangement.markers[index].name.clone_from(&name);
    Ok(json!({ "index": index, "name": name }))
}

fn delete_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = marker_index(&session.project, Args::new(params))?;
    let marker = session.project.arrangement.markers.remove(index);
    Ok(json!({ "deleted": index, "name": marker.name }))
}

fn jump_to_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // Without an engine there is nothing to jump; say so before looking at the marker.
    session.player_mut()?;
    let index = marker_index(session.project(), Args::new(params))?;
    let beat = session.project().arrangement.markers[index].beat.clamp(0.0, MAX_START_BEAT);
    send(session, LaunchCommand::SetPosition { beat })?;
    Ok(json!({ "index": index, "start_beat": beat }))
}
