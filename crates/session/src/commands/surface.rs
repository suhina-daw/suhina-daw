// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Launcher window a control surface shows. A surface program reports it, and the window
//! draws its outline over the slots, as a controller's session ring does.

use super::*;
use crate::ControlSurfaceWindow;

pub(super) static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "surface.setWindow",
    label: "Report the Launcher window a control surface shows: its first track and scene and how many of each, for the window to outline the slots the surface sees",
    params: &[
        param("id", ParamType::Text, "the surface's extension id, such as apc40-mkii"),
        optional("name", ParamType::Text, "the surface's name for a window's label; the id when left out"),
        param("track", ParamType::Integer, "the first track shown, from 0"),
        param("scene", ParamType::Integer, "the first scene shown, from 0"),
        optional("tracks", ParamType::Integer, "how many track columns, 1 to 64 (default 8)"),
        optional("scenes", ParamType::Integer, "how many scene rows, 1 to 64 (default 5)"),
    ],
    kind: CommandKind::on(Target::Project, Effect::Change),
    run: set_window,
    policy: Policy::DEFAULT,
}];

fn count(args: Args<'_>, name: &str, default: usize) -> Result<usize, CommandError> {
    let value = args.opt_index(name, "from 1")?.unwrap_or(default);
    if !(1..=64).contains(&value) {
        return Err(format!("{name} must be from 1 to 64, got {value}").into());
    }
    Ok(value)
}

fn set_window(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let id = args.text("id")?;
    let name = args.opt_text("name")?.unwrap_or(id);
    let track = args.index("track", "from 0")?;
    let scene = args.index("scene", "from 0")?;
    let tracks = count(args, "tracks", 8)?;
    let scenes = count(args, "scenes", 5)?;
    if track > 100_000 || scene > 100_000 {
        return Err("track and scene must be at most 100000".into());
    }
    session.set_control_surface_window(ControlSurfaceWindow {
        id: id.to_string(),
        name: name.to_string(),
        track,
        scene,
        tracks,
        scenes,
    });
    Ok(json!({
        "id": id,
        "name": name,
        "track": track,
        "scene": scene,
        "tracks": tracks,
        "scenes": scenes,
    }))
}
