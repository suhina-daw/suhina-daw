// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `project.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "project.save",
        label: "Save the project as a Suhina project (.suhina) with its plug-ins' state as it is now; without a path, to the project file it was opened from or last saved to",
        params: &[
            optional("path", ParamType::Path(PathKind::WriteFile), "file to save to; .suhina is added when missing"),
            optional(
                "copy",
                ParamType::Bool,
                "true writes a copy (a template, a backup) and the project stays with its own file",
            ),
        ],
        kind: CommandKind::Read,
        run: save,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.info",
        label: "Summarise the open project: tempo, time signature, counts of tracks, scenes, clips and markers, undo state",
        params: &[],
        kind: CommandKind::Read,
        run: info,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.tracks",
        label: "List tracks with id, name, volume (dB), pan, mute, whether armed, the MIDI and audio inputs it hears, and devices",
        params: &[],
        kind: CommandKind::Read,
        run: tracks,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.clips",
        label: "List arrangement clips with their id (what clip commands take), track, index, start and length in beats, and note count",
        params: &[optional("track", ParamType::Track, "only clips on this track (id or name)")],
        kind: CommandKind::Read,
        run: clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.markers",
        label: "List the arrangement's markers (named positions on the timeline) in beat order, with index, beat and name",
        params: &[],
        kind: CommandKind::Read,
        run: markers,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.launcher",
        label: "List the clip launcher: scenes (name, tempo) and each track's slots (clip name, follow action, stop button)",
        params: &[],
        kind: CommandKind::Read,
        run: launcher_grid,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.export",
        label: "Render the mix, or the named node of the track tree, to an audio file: wav (32-bit float or 16/24-bit PCM), flac, ogg or mp3, with sample rate, depth, mp3 rate, range, tail, normalise and dither",
        params: &[
            optional(
                "path",
                ParamType::Path(PathKind::WriteFile),
                "the file to write (output is an alias); its extension decides the format when format is left out",
            ),
            optional("output", ParamType::Path(PathKind::WriteFile), "the file to write; an alias of path"),
            optional("format", ParamType::Text, "wav (default), flac, ogg or mp3"),
            optional("sampleRate", ParamType::Integer, "hertz; 48000 by default; with mp3, 32, 44.1 or 48 kHz"),
            optional("bitDepth", ParamType::Text, "32f (the default for wav), 24 or 16; an error with ogg or mp3"),
            optional(
                "mp3Rate",
                ParamType::Text,
                "the mp3's rate: 320 (default), 256, 192 or 128 kbps, or the variable qualities v0 and v2",
            ),
            optional("overwrite", ParamType::Bool, "true replaces an existing file; it is never replaced without this"),
            optional(
                "range",
                ParamType::Range,
                "the beats to render, {start, end} from the arrangement's start; the whole arrangement when left out",
            ),
            optional("tail", ParamType::Number, "extra beats rendered after the range's end"),
            optional("normalize", ParamType::Bool, "true scales the file's peak to -0.3 dBFS"),
            optional(
                "dither",
                ParamType::Bool,
                "triangular dither for a reduced depth; on there, off at 32f, by default",
            ),
            optional(
                "source",
                ParamType::Track,
                "the track or group of the track tree to render, instead of the whole mix (id or name)",
            ),
        ],
        kind: CommandKind::Read,
        run: crate::export::export,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.missingFiles",
        label: "List the audio files the project's clips name that cannot be found, with where they were and how many clips play each",
        params: &[],
        kind: CommandKind::Read,
        run: crate::relink::missing_files,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.relinkFiles",
        label: "Point every clip that plays a missing (or any) audio file at another file, as one undo step; library.findFiles finds candidates",
        params: &[
            param("files", ParamType::Texts, "the files as project.missingFiles names them"),
            param(
                "to",
                ParamType::Path(PathKind::Files),
                "for each, the file to play instead (the same order); at most 4096",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::relink::relink_files,
        policy: Policy::CRITICAL,
    },
    CommandSpec {
        id: "project.collect",
        label: "Copy the audio the project plays from outside its folder into Samples/Collected there and point the clips at the copies, as one undo step; save afterwards to keep it (Collect and save)",
        params: &[optional(
            "folder",
            ParamType::Path(PathKind::WriteFolder),
            "the folder to collect into: the project's own by default; needed before it is saved",
        )],
        kind: CommandKind::Edit,
        run: crate::relink::collect,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "project.setDefaultScale",
        label: "Set the scale new MIDI clips take (clip.create, recorded and dropped MIDI clips), or clear it with off; clips that exist keep theirs",
        params: &[
            SCALE_ROOT,
            SCALE_MODE,
            optional("off", ParamType::Bool, "true clears the default scale; not with root or mode"),
        ],
        kind: CommandKind::Edit,
        run: crate::scales::set_default_scale,
        policy: Policy::DEFAULT,
    },
];

fn save(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copy = match params.get("copy") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("copy must be true or false".into()),
    };
    let path = match params.get("path") {
        Some(Value::String(path)) if !path.trim().is_empty() => std::path::PathBuf::from(path.trim()),
        Some(Value::String(_)) | None | Some(Value::Null) => match session.source() {
            Some(source) if suhina_project_file::is_project_file(source) => source.to_path_buf(),
            _ => return Err("the project has no .suhina file yet; give a path to save it to".into()),
        },
        Some(_) => return Err("path must be text".into()),
    };
    let written = session.save(&path, copy)?;
    Ok(json!({ "path": written.display().to_string(), "copy": copy }))
}

fn launcher_grid(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let scenes: Vec<Value> = p
        .launcher
        .scenes
        .iter()
        .zip(1..)
        .map(|(scene, number): (_, u32)| {
            json!({
                "scene": number,
                "id": scene.id.0,
                "name": scene.name,
                "tempo_bpm": scene.tempo_bpm,
                "time_signature": scene.time_signature.map(signature_text),
            })
        })
        .collect();
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .map(|column| {
            let slots: Vec<Value> = column
                .slots
                .iter()
                .zip(1..)
                .map(|(slot, number): (_, u32)| match &slot.clip {
                    Some(clip) => json!({
                        "scene": number,
                        "id": clip.id.0,
                        "clip": p.content(clip.content).map(|c| c.name.as_str()),
                        "length": clip.length,
                        "loop": clip.loop_range.is_some(),
                        "enabled": clip.enabled,
                        "follow_action": follow::describe(&clip.follow_action),
                        "launch_mode": clip.launch.mode.name(),
                        "legato": clip.launch.legato,
                        "quantization": clip.launch.quantization.name(),
                    }),
                    None => json!({ "scene": number, "clip": null, "stop_button": slot.has_stop }),
                })
                .collect();
            json!({ "track": column.track.0, "track_name": track_name(p, column.track), "slots": slots })
        })
        .collect();
    Ok(json!({ "scenes": scenes, "tracks": tracks }))
}
