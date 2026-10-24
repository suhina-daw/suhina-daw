// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `arrangement.*`, `clip.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "arrangement.audioOverlaps",
        label: "List the pairs of audio clips that sound together on one track because their arrangement spans overlap; clips that only touch do not",
        params: &[optional("track", ParamType::Track, "only overlaps on this track (id or name)")],
        kind: CommandKind::Read,
        run: audio_overlaps,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.delete",
        label: "Delete arrangement clips as one undo step, or the clip in a track's slot of a scene",
        params: &[CLIP, DELETE_CLIPS, DELETE_TRACK, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Edit,
        run: delete_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.rename",
        label: "Rename a clip: by its id, or (deprecated) an arrangement clip by index; a slot's clip by scene; clips that play the same content share the name",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("name", ParamType::Text, "the new name (empty clears it)"),
        ],
        kind: CommandKind::Edit,
        run: rename_clip,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.renameMany",
        label: "Give several arrangement clips the same name, as one undo step; clips that play the same content share the name",
        params: &[CLIPS, param("name", ParamType::Text, "the new name (empty clears it)")],
        kind: CommandKind::Edit,
        run: rename_clips,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.setColorMany",
        label: "Give several arrangement clips a colour of their own, or their tracks' colour again, as one undo step",
        params: &[CLIPS, param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for each track's colour")],
        kind: CommandKind::Edit,
        run: set_clips_color,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.setGain",
        label: "Set an audio clip's gain (every clip that plays the same audio content follows)",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("db", ParamType::Decibels, "gain in dB, at most +24; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: set_clip_gain,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setFades",
        label: "Set an audio clip's fade-in and fade-out: lengths in beats (together at most the clip's length), shapes and curves; an arrangement clip by index, a launcher clip by scene",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "arrangement clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "instead of index: the clip in the track's slot of this scene, from 1",
            ),
            optional("in", ParamType::Number, "fade-in length in beats from the clip's start, at least 0; 0 is none"),
            optional("out", ParamType::Number, "fade-out length in beats before the clip's end, at least 0; 0 is none"),
            optional("inShape", ParamType::Text, "fade-in shape: linear, equal_power, fast, slow or s_curve"),
            optional("outShape", ParamType::Text, "fade-out shape: linear, equal_power, fast, slow or s_curve"),
            optional(
                "inCurve",
                ParamType::Number,
                "fade-in curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
            optional(
                "outCurve",
                ParamType::Number,
                "fade-out curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_clip_fades,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.shiftFades",
        label: "Lengthen or shorten the fade-in or fade-out of several arrangement clips by the same beats, each from its own, kept from 0 to what the clip's other fade leaves; clips with no audio are left as they are; as one undo step",
        params: &[
            CLIPS,
            param("edge", ParamType::Text, "in or out"),
            param("beats", ParamType::Number, "how much longer (or, below 0, shorter) each fade gets"),
        ],
        kind: CommandKind::Edit,
        run: shift_fades,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.copy",
        label: "Copy clips, with what they play, for clip.paste: arrangement clips by clips, or a clip slot's by track and scene",
        params: &[
            CLIP,
            optional(
                "clips",
                ParamType::Clips,
                "the arrangement clips to copy, each { \"clip\": id } as project.clips lists them (or, deprecated, { \"track\": id or name, \"index\": n })",
            ),
            optional(
                "track",
                ParamType::Track,
                "instead of clip or clips, with scene: the track whose clip slot to copy",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "with track: the clip in this scene's slot, from 1, as in project.launcher",
            ),
        ],
        kind: CommandKind::Read,
        run: copy_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.cut",
        label: "Copy arrangement clips for clip.paste and delete them, as one undo step",
        params: &[CLIPS],
        kind: CommandKind::Edit,
        run: cut_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.paste",
        label: "Place the copied clip on a track's arrangement at start, replacing what it covers unless stacked, or in its clip slot at scene, replacing the slot's clip",
        params: &[
            optional(
                "track",
                ParamType::Track,
                "track id or name, from project.tracks; a copied range pastes back on its own tracks without one",
            ),
            optional(
                "start",
                ParamType::Number,
                "where it starts in the arrangement, in beats from the beginning, at least 0",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "instead of start: the scene, from 1, whose slot on the track takes the clip",
            ),
            optional("stack", ParamType::Bool, "with start, true leaves clips under it as they are"),
        ],
        kind: CommandKind::Edit,
        run: paste_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.duplicate",
        label: "Place a copy of each arrangement clip right after it, replacing what the copies cover unless stacked",
        params: &[
            CLIPS,
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copies as they are; false (default) shortens, splits or removes them",
            ),
        ],
        kind: CommandKind::Edit,
        run: duplicate_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setEnabled",
        label: "Turn arrangement clips on or off: a clip that is off is drawn dimmed and plays nothing",
        params: &[CLIPS, param("enabled", ParamType::Bool, "true plays the clips, false silences them")],
        kind: CommandKind::Edit,
        run: set_clip_enabled,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setColor",
        label: "Give a clip a colour of its own (by index), or every clip of a track one when no index is given; \"none\" makes them show the track's colour again",
        params: &[
            CLIP_TRACK,
            param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the track's colour"),
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip); every clip of the track when neither is given",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_color::set_clip_color,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.trim",
        label: "Move an arrangement clip's edges (song beats), the content moving with the left one; an unlooped clip ends at most where its audio does, a looping one repeats as far as it goes, and loop turns the loop on first",
        params: &[
            CLIP,
            CLIP_TRACK,
            CLIP_INDEX,
            optional("start", ParamType::Number, "where the clip's left edge goes, in beats from the beginning"),
            optional("end", ParamType::Number, "where the clip's right edge goes, in beats from the beginning"),
            optional(
                "loop",
                ParamType::Bool,
                "true turns the clip's loop on (over what it plays now) before the edges move",
            ),
            optional("stack", ParamType::Bool, "true leaves clips under it as they are; false (default) replaces them"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_edges::trim_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.stretch",
        label: "Move an audio clip's edges with its audio time-stretched to fill the new length, its pitch kept",
        params: &[
            CLIP,
            CLIP_TRACK,
            CLIP_INDEX,
            optional("start", ParamType::Number, "where the clip's left edge goes, in beats from the beginning"),
            optional("end", ParamType::Number, "where the clip's right edge goes, in beats from the beginning"),
            optional("stack", ParamType::Bool, "true leaves clips under it as they are; false (default) replaces them"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_edges::stretch_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.moveMany",
        label: "Move several arrangement clips by the same number of beats at once, so none replaces another on the way; toTrack takes clips of one track to another, tracksBy moves each by as many tracks, toNewTrack makes a track for them",
        params: &[
            param("clips", ParamType::Clips, "the clips to move, each {track, index} from project.clips"),
            param("beats", ParamType::Number, "how far to move them, in beats; negative is earlier"),
            optional("toTrack", ParamType::Track, "track to move them to, when they are all on one track"),
            optional(
                "tracksBy",
                ParamType::Integer,
                "instead of toTrack: move each that many tracks down (negative: up) from its own, among the tracks that hold clips",
            ),
            optional(
                "toNewTrack",
                ParamType::Bool,
                "instead of toTrack: true makes a track of the clips' kind after the last and moves them there, in the same undo step",
            ),
            optional(
                "araEdits",
                ParamType::Text,
                "for clips a plug-in edits (Melodyne) going to another track: keep takes their edits along and puts the plug-in on that track when it has none; new (default) starts them there without",
            ),
            optional(
                "color",
                ParamType::Text,
                "\"#rrggbb\": the colour they show now, kept on the other track by those with none of their own",
            ),
            optional(
                "copy",
                ParamType::Bool,
                "true leaves the clips where they are and puts copies (each with a content of its own) where they would have gone",
            ),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under them as they are; false (default) replaces them",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::range::move_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.split",
        label: "Cut an arrangement clip in two at a song position; the second part plays on from where the clip was",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("at", ParamType::Number, "where to cut, in beats from the beginning of the song, inside the clip"),
        ],
        kind: CommandKind::Edit,
        run: editing::split_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.consolidate",
        label: "Turn a range of one track into one clip: MIDI clips by joining their notes, audio clips by rendering them (no devices) to a new file next to the project",
        params: &[
            TRACK,
            param("start", ParamType::Number, "where the range starts, in beats from the beginning, at least 0"),
            param("end", ParamType::Number, "where the range ends, in beats, after start"),
            optional(
                "clips",
                ParamType::Integers,
                "only these clips (ids from project.clips), the others in the range left as they are: one row of a stack",
            ),
        ],
        kind: CommandKind::Edit,
        run: editing::consolidate,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.move",
        label: "Move an arrangement clip to a new start position, and to another track if asked; it replaces what it covers unless stacked",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("start", ParamType::Number, "new start in beats from the beginning, at least 0"),
            optional(
                "toTrack",
                ParamType::Track,
                "track to move the clip to (id or name); it must take the clip's kind: notes or audio",
            ),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under it as they are (they all play); false (default) shortens, splits or removes them",
            ),
            optional(
                "color",
                ParamType::Text,
                "\"#rrggbb\": the colour the clip shows now, which it keeps on another track when it has none of its own",
            ),
        ],
        kind: CommandKind::Edit,
        run: move_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.moveSlot",
        label: "Move a slot's clip to another track's slot; the two slots swap when the target has a clip",
        params: &[
            TRACK,
            SCENE,
            optional(
                "toTrack",
                ParamType::Track,
                "track to move the clip to (id or name); it must take the clip's kind: notes or audio (default: the same track)",
            ),
            optional("toScene", ParamType::Integer, "the scene to move it to, from 1 (default: the same scene)"),
        ],
        kind: CommandKind::Edit,
        run: move_slot,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.record",
        label: "Record into an empty clip slot of an armed track from the next launch boundary (after the count-in when the transport is stopped); clip.launch on the slot ends the take at the next boundary and plays it, looped",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: crate::recording::record_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.launch",
        label: "Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track)",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLoop",
        label: "Switch a clip's loop (an arrangement clip by index, or the clip in a track's slot by scene): on, it repeats its whole length, or the loop it has; off, it plays once. With start and end, the loop is set to that part of the content and turned on",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional(
                "loop",
                ParamType::Bool,
                "true loops the clip, false plays it once; needed unless start or end is given",
            ),
            optional("start", ParamType::Number, "the loop's start in content beats, 0 to 120000"),
            optional("end", ParamType::Number, "the loop's end in content beats, after start, at most 120000"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_loop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setAudio",
        label: "Set an audio clip's warp switch, warp mode or transpose (an arrangement clip, or the clip in a track's slot by scene); every clip that plays the same audio follows, unless warp turned on would fit its markers to another tempo than theirs",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional("warp", ParamType::Bool, "true warps the audio to the tempo, false plays it at its own speed"),
            optional(
                "mode",
                ParamType::Text,
                "the warp mode: repitch, rhythmic, tonal, texture, smooth or high-quality",
            ),
            optional("transpose", ParamType::Number, "pitch shift in semitones, -48 to 48 (fractions are cents)"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_audio,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLength",
        label: "Set how long a clip plays, in beats (an arrangement clip by index, or a slot's clip by scene); an arrangement clip keeps its start and may not run into the next clip on its lane",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("length", ParamType::Number, "the new length in beats, above 0 and at most 120000"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_length,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setStart",
        label: "Set a clip's start marker: where in its content it starts playing. A clip that does not loop keeps its end marker, so its length changes by as much; a looping clip keeps its length",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param(
                "offset",
                ParamType::Number,
                "the start marker in content beats, 0 to 120000; before the loop's end, or the clip's end",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_start,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setFollowAction",
        label: "Set what a slot clip does after it has played for a while: action A or B, by chance",
        params: &[
            TRACK,
            SCENE,
            optional("enabled", ParamType::Bool, "whether the clip runs its follow action"),
            optional(
                "time",
                ParamType::Number,
                "beats from the launch to the action, more than 0; used when not linked",
            ),
            optional("linked", ParamType::Bool, "true: the time is the clip's length"),
            optional(
                "actionA",
                ParamType::Text,
                "nothing, stop, again, previous, next, first, last, any, other, or \"jump N\" for the slot of scene N",
            ),
            optional("actionB", ParamType::Text, "like actionA"),
            optional("chanceA", ParamType::Integer, "weight of action A, 0 to 100"),
            optional("chanceB", ParamType::Integer, "weight of action B, 0 to 100"),
        ],
        kind: CommandKind::Edit,
        run: follow::set_follow_action,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.release",
        label: "Release a slot's launch button (clip.launch is the press): stops a clip in gate mode, ends the repeats of one in repeat mode",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch::release_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLaunchMode",
        label: "Set what launching a slot's clip does: trigger (plays until stopped), gate (plays while held), toggle (a second launch stops it) or repeat (relaunches at its quantisation while held)",
        params: &[TRACK, SCENE, param("mode", ParamType::Text, "trigger, gate, toggle or repeat")],
        kind: CommandKind::Edit,
        run: launch::set_launch_mode,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLegato",
        label: "Set whether a slot's clip starts at the position of the clip it replaces on its track (legato) instead of at its start",
        params: &[TRACK, SCENE, param("on", ParamType::Bool, "true for legato")],
        kind: CommandKind::Edit,
        run: launch::set_legato,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setQuantization",
        label: "Set the grid a slot's clip waits for when launched: global (the launcher's), none, or its own",
        params: &[
            TRACK,
            SCENE,
            param(
                "value",
                ParamType::Text,
                "global, none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32",
            ),
        ],
        kind: CommandKind::Edit,
        run: launch::set_quantization,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.addFiles",
        label: "Place samples and MIDI files as clips: on a track (or new tracks when none is given), one after another from a beat in the arrangement or down the clip slots from a scene; separate gives each clip a track of its own",
        params: &[
            param(
                "paths",
                ParamType::Path(PathKind::Files),
                "the files: samples (wav, aiff, flac, mp3, ogg) and MIDI files (.mid); at most 256",
            ),
            optional(
                "track",
                ParamType::Track,
                "the track to place them on; files it cannot hold go on a new track after it (default: new tracks)",
            ),
            optional("start", ParamType::Number, "arrangement beat the first clip starts at"),
            optional("scene", ParamType::Integer, "scene number, from 1, of the first clip slot"),
            optional(
                "separate",
                ParamType::Bool,
                "one track per clip, side by side, instead of one after another (default false)",
            ),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new ones cover (default false)"),
            optional(
                "warp",
                ParamType::Text,
                "stretch mode for samples with a known tempo: repitch, rhythmic (default), tonal, texture, smooth, high-quality",
            ),
        ],
        kind: CommandKind::Edit,
        run: files::add_files,
        policy: Policy::CRITICAL,
    },
    CommandSpec {
        id: "clip.create",
        label: "Make an empty clip on a track that holds clips: MIDI on a notes track, audio with no file yet on an audio track; in an empty clip slot, or at a beat in the arrangement, replacing what it covers unless stacked",
        params: &[
            TRACK,
            optional("scene", ParamType::Integer, "scene number, from 1, of an empty clip slot"),
            optional("start", ParamType::Number, "arrangement beat it starts at, at least 0"),
            optional("length", ParamType::Number, "length in beats (default one bar of the time signature)"),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new one covers (default false)"),
        ],
        kind: CommandKind::Edit,
        run: files::create_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.scale",
        label: "The scale of a MIDI clip (root and mode, or null for none) and the project's default scale for new clips",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Read,
        run: crate::scales::clip_scale,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setScale",
        label: "Set the scale of a MIDI clip (the piano roll marks its keys), or clear it with off; a content other clips also play is copied for this clip first",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            SCALE_ROOT,
            SCALE_MODE,
            optional("off", ParamType::Bool, "true clears the scale; not with root or mode"),
        ],
        kind: CommandKind::Edit,
        run: crate::scales::set_scale,
        policy: Policy::DEFAULT,
    },
];

/// `arrangement.audioOverlaps`: [`Project::audio_overlaps`], each clip named as `clip.move`
/// addresses it (ADR 0028).
fn audio_overlaps(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let only = match params.get("track") {
        Some(_) => Some(track_id(p, params)?),
        None => None,
    };
    let track_name = |id: TrackId| p.track(id).map(|t| t.name.as_str()).unwrap_or_default();
    let clip = |place: suhina_model::OverlapClip| {
        let found = p.arrangement.lanes.get(place.lane).and_then(|l| l.clips.get(place.index));
        let name = found.and_then(|c| p.content(c.content)).map(|c| c.name.as_str());
        json!({
            "id": found.map(|c| c.id.0),
            "track": place.track.0,
            "track_name": track_name(place.track),
            "index": place.index,
            "name": name,
        })
    };
    let found = p.audio_overlaps();
    let overlaps: Vec<Value> = found
        .pairs
        .iter()
        .filter(|o| only.is_none_or(|t| t == o.track))
        .map(|o| {
            json!({
                "track": o.track.0,
                "track_name": track_name(o.track),
                "start": o.start,
                "end": o.end,
                "kind": if o.contained { "contained" } else { "partial" },
                "clips": [clip(o.first), clip(o.second)],
            })
        })
        .collect();
    Ok(json!({ "overlaps": overlaps, "truncated": found.truncated }))
}

/// The clip `params` names: an arrangement clip by `index`, or the clip in a launcher slot by
/// `scene`.
fn clip_at(p: &Project, params: &Value) -> Result<ClipAt, CommandError> {
    match (params.get("index"), params.get("scene")) {
        (Some(_), Some(_)) => Err("give index or scene, not both".into()),
        (None, Some(_)) => {
            let (column, scene) = (column_index(p, params)?, scene_index(p, params)?);
            match p.launcher.columns.get(column).and_then(|c| c.slots.get(scene)) {
                Some(slot) if slot.clip.is_some() => Ok(ClipAt::Slot { column, scene }),
                _ => Err(format!("the slot of scene {} is empty", scene + 1).into()),
            }
        }
        (Some(_), None) => lane_and_index(p, params).map(|(lane, index)| ClipAt::Arrangement { lane, index }),
        (None, None) => Err("give index (an arrangement clip) or scene (a launcher clip)".into()),
    }
}

fn set_clip_gain(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > 24.0 {
        return Err(format!("db must be at most +24, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let (lane, index) = lane_and_index(&session.project, params)?;
    let content = session
        .project
        .arrangement
        .lanes
        .get(lane)
        .and_then(|l| l.clips.get(index))
        .map(|c| c.content)
        .ok_or("the clip is missing")?;
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    let mut audio = false;
    for lane in &mut content.lanes {
        if let LaneData::Audio(region) = &mut lane.data {
            region.gain = gain;
            audio = true;
        }
    }
    if !audio {
        return Err("only an audio clip has a gain".into());
    }
    Ok(json!({ "gain_db": to_db(gain) }))
}

fn set_clip_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let length_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let beats = number(params, name)?;
                if beats < 0.0 {
                    return Err(format!("{name} must be at least 0 beats, got {beats}").into());
                }
                Ok(Some(beats))
            }
        }
    };
    let shape_of = |name: &str| -> Result<Option<FadeShape>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(Value::String(s)) => FadeShape::from_name(s).map(Some).ok_or_else(|| {
                let names: Vec<&str> = FadeShape::ALL.iter().map(|s| s.name()).collect();
                format!("{name} must be one of {}, got {s:?}", names.join(", ")).into()
            }),
            Some(_) => Err(format!("{name} must be a shape name").into()),
        }
    };
    let curve_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let curve = number(params, name)?;
                if !(-1.0..=1.0).contains(&curve) {
                    return Err(format!("{name} must be from -1 to +1, got {curve}").into());
                }
                Ok(Some(curve))
            }
        }
    };
    let (fade_in, fade_out) = (length_of("in")?, length_of("out")?);
    let (in_shape, out_shape) = (shape_of("inShape")?, shape_of("outShape")?);
    let (in_curve, out_curve) = (curve_of("inCurve")?, curve_of("outCurve")?);
    if fade_in.is_none()
        && fade_out.is_none()
        && in_shape.is_none()
        && out_shape.is_none()
        && in_curve.is_none()
        && out_curve.is_none()
    {
        return Err("give in, out (beats), inShape, outShape, inCurve or outCurve".into());
    }
    let at = clip_at(&session.project, params)?;
    let p = &mut session.project;
    let clip = at.get(p).ok_or("the clip is missing")?;
    let audio = p.content(clip.content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
    if !audio {
        return Err("only an audio clip has fades".into());
    }
    let changed = Clip {
        fade_in: fade_in.unwrap_or(clip.fade_in),
        fade_out: fade_out.unwrap_or(clip.fade_out),
        fade_in_curve: FadeCurve {
            shape: in_shape.unwrap_or(clip.fade_in_curve.shape),
            curve: in_curve.unwrap_or(clip.fade_in_curve.curve),
        },
        fade_out_curve: FadeCurve {
            shape: out_shape.unwrap_or(clip.fade_out_curve.shape),
            curve: out_curve.unwrap_or(clip.fade_out_curve.curve),
        },
        ..clip.clone()
    };
    if !changed.fades_valid() {
        if !(changed.fade_in_curve.is_valid() && changed.fade_out_curve.is_valid()) {
            return Err("the clip's fade curves are outside -1 to +1; set inCurve and outCurve".into());
        }
        return Err(format!(
            "the fades ({} + {} beats) are longer than the clip ({} beats)",
            changed.fade_in, changed.fade_out, changed.length
        )
        .into());
    }
    let result = json!({
        "fade_in": changed.fade_in,
        "fade_out": changed.fade_out,
        "fade_in_shape": changed.fade_in_curve.shape.name(),
        "fade_in_curve": changed.fade_in_curve.curve,
        "fade_out_shape": changed.fade_out_curve.shape.name(),
        "fade_out_curve": changed.fade_out_curve.curve,
    });
    if let Some(target) = at.get_mut(p) {
        *target = changed;
    }
    Ok(result)
}

fn copy_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let mut copied: Vec<(Clip, ClipContent)> = Vec::new();
    let mut from: Vec<TrackId> = Vec::new();
    match (params.get("clips"), params.get("scene")) {
        (Some(_), None) => {
            if params.get("track").is_some() {
                return Err("track goes with scene (a clip slot); clips names arrangement clips".into());
            }
            for (track, index) in clip_list(p, params)? {
                let clip = p
                    .arrangement
                    .lanes
                    .iter()
                    .find(|l| l.track == Some(track))
                    .and_then(|l| l.clips.get(index))
                    .cloned()
                    .ok_or("the clip is missing")?;
                let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
                copied.push((clip, content));
                from.push(track);
            }
        }
        (None, Some(_)) => {
            let (column, scene) = (column_index(p, params)?, scene_index(p, params)?);
            let slot = p.launcher.columns.get(column).and_then(|c| c.slots.get(scene));
            let clip =
                slot.and_then(|s| s.clip.clone()).ok_or_else(|| format!("the slot in scene {} is empty", scene + 1))?;
            let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
            copied.push((clip, content));
        }
        _ => return Err("give either clips (arrangement clips) or track and scene (a clip slot)".into()),
    }
    let names: Vec<String> = copied.iter().map(|(_, content)| content.name.clone()).collect();
    session.clipboards.clips = copied;
    session.clipboards.tracks = from;
    Ok(json!({ "copied": names }))
}

fn cut_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copied = copy_clips(session, params)?;
    delete_clips(session, params)?;
    Ok(copied)
}

/// Where `clip.paste` puts the copy: an arrangement beat, or a clip slot (scene from 0).
#[derive(Clone, Copy)]
enum PasteAt {
    Beat(f64),
    Slot(usize),
}

fn paste_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if session.clipboards.clips.is_empty() {
        return Err("nothing has been copied".into());
    }
    let at = match (params.get("start"), params.get("scene")) {
        (Some(_), None) => {
            let start = number(params, "start")?;
            if !(0.0..=MAX_START_BEAT).contains(&start) {
                return Err(format!("start must be from 0 to {MAX_START_BEAT}, got {start}").into());
            }
            PasteAt::Beat(start)
        }
        (None, Some(_)) => {
            if params.get("stack").is_some() {
                return Err("stack is for the arrangement (start); a slot holds one clip".into());
            }
            PasteAt::Slot(scene_index(&session.project, params)?)
        }
        _ => return Err("give either start (a beat in the arrangement) or scene (a clip slot)".into()),
    };
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    // No track: each copied clip goes back to the track it came from.
    if params.get("track").is_none()
        && let PasteAt::Beat(start) = at
    {
        return crate::range::paste_to_sources(session, start, stack);
    }
    let track = track_id(&session.project, params)?;
    {
        let p = &session.project;
        let target = p.track(track).ok_or("the track is missing")?;
        for (_, content) in &session.clipboards.clips {
            let notes = content.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_)));
            let audio = content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)));
            if (notes && !target.content.notes) || (audio && !target.content.audio) {
                let what = if audio { "an audio clip" } else { "a MIDI clip" };
                return Err(format!("track {:?} cannot hold {what}", target.name).into());
            }
        }
    }
    let column = match at {
        PasteAt::Slot(_) => Some(column_index(&session.project, params)?),
        PasteAt::Beat(_) => None,
    };
    if let (PasteAt::Slot(scene), Some(column)) = (at, column) {
        let [(clip, content)] = session
            .clipboards
            .clips
            .clone()
            .try_into()
            .map_err(|copied: Vec<_>| format!("a clip slot holds one clip, and {} are copied", copied.len()))?;
        let p = &mut session.project;
        let id = new_content_id(p)?;
        p.contents.push(suhina_model::ClipContent { id, ..content });
        // Gain, warp and the loop travel with the content and the clip; fades are fitted, as when
        // a clip is shortened, so a clip with odd fades still makes a valid project.
        // The pasted clip is a new clip, with an id of its own.
        let mut placed = Clip { id: p.new_clip_id()?, content: id, start: 0.0, ..clip };
        placed.fit_fades();
        let placed_id = placed.id;
        let slot =
            p.launcher.columns.get_mut(column).and_then(|c| c.slots.get_mut(scene)).ok_or("the slot is missing")?;
        let replaced = slot.clip.is_some();
        slot.clip = Some(placed);
        return Ok(json!({ "clip": placed_id.0, "track": track.0, "scene": scene + 1, "replaced": replaced }));
    }
    let start = match at {
        PasteAt::Beat(start) => start,
        PasteAt::Slot(_) => 0.0,
    };
    // The earliest copied clip lands on `start`; the others keep their places relative to it.
    let earliest = session.clipboards.clips.iter().map(|(clip, _)| clip.start).fold(f64::INFINITY, f64::min);
    let offset = if earliest.is_finite() { start - earliest } else { 0.0 };
    let copied = session.clipboards.clips.clone();
    let p = &mut session.project;
    let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
        Some(lane) => lane,
        None => {
            let name = p.track(track).map(|t| t.name.clone()).unwrap_or_default();
            let id = p.new_lane_id()?;
            p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(track), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let mut placed: Vec<Value> = Vec::with_capacity(copied.len());
    for (clip, content) in copied {
        // The copy plays its own content, so editing one does not change the other.
        let id = new_content_id(p)?;
        p.contents.push(suhina_model::ClipContent { id, ..content });
        let at = clip.start + offset;
        let mut copy = Clip { id: p.new_clip_id()?, content: id, start: at, ..clip };
        copy.fit_fades();
        let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
        if !stack {
            make_room(&mut lane.clips, at, at + copy.length);
        }
        let copy_id = copy.id;
        lane.clips.push(copy);
        lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = lane.clips.iter().position(|c| c.id == copy_id).unwrap_or_default();
        placed.push(json!({ "clip": copy_id.0, "track": track.0, "index": index, "start": at }));
    }
    Ok(json!({ "clips": placed }))
}

fn delete_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // A list of arrangement clips goes as one; otherwise a launcher slot's clip is named by
    // `scene`, an arrangement clip by `index`, the way the note commands name the clip they edit.
    if params.get("clips").is_some_and(|v| !v.is_null()) {
        if ["track", "index", "scene"].iter().any(|k| params.get(*k).is_some_and(|v| !v.is_null())) {
            return Err("give clips, or track with index or scene, not both".into());
        }
        let clips = clip_list(&session.project, params)?;
        let deleted = remove_clips(&mut session.project, &clips);
        return Ok(json!({ "deleted": deleted }));
    }
    let whole = |key: &str| -> Result<Option<u64>, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{key} must be a whole number").into()),
        }
    };
    match (whole("index")?, whole("scene")?) {
        (Some(_), Some(_)) => Err("give index or scene, not both".into()),
        (None, None) => Err("give clips, or track with index (an arrangement clip) or scene (a launcher clip)".into()),
        (Some(_), None) => {
            let (lane, index) = lane_and_index(&session.project, params)?;
            if let Some(lane) = session.project.arrangement.lanes.get_mut(lane) {
                lane.clips.remove(index);
            }
            Ok(json!({ "deleted": index }))
        }
        (None, Some(scene)) => {
            let column = column_index(session.project(), params)?;
            let s = scene_index(session.project(), params)?;
            let slot = session
                .project
                .launcher
                .columns
                .get_mut(column)
                .and_then(|c| c.slots.get_mut(s))
                .ok_or("the slot is missing")?;
            if slot.clip.is_none() {
                return Err(format!("the slot of scene {scene} is empty").into());
            }
            slot.clip = None;
            Ok(json!({ "deleted": scene }))
        }
    }
}

/// `clip.rename`: the clip's name is its content's, so clips that play the same content share
/// the new name (a pasted copy has a content of its own and keeps its name). An empty name
/// clears it, and the launcher's slots then show "clip".
fn rename_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    let (_, content, _) = crate::notes::locate(&session.project, params)?;
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    content.name.clone_from(&name);
    Ok(json!({ "name": name }))
}

/// `clip.shiftFades {clips, edge, beats}` (#715). Only an audio clip has fades, as in
/// `clip.setFades`: a clip with no audio lane is left as it is and not counted in `clips`.
fn shift_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let fade_in = match params.get("edge").and_then(Value::as_str) {
        Some("in") => true,
        Some("out") => false,
        _ => return Err("edge must be in or out".into()),
    };
    let beats = number(params, "beats")?;
    let p = &mut session.project;
    let chosen = clip_list(p, params)?;
    let mut changed = 0;
    for (track, index) in chosen {
        let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(track));
        let content = lane.and_then(|l| l.clips.get(index)).map(|c| c.content).ok_or("the clip is missing")?;
        // Only an audio clip has fades (`clip.setFades`): a mixed selection leaves the others.
        let audio = p.content(content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
        if !audio {
            continue;
        }
        let clip = p
            .arrangement
            .lanes
            .iter_mut()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.get_mut(index))
            .ok_or("the clip is missing")?;
        let (own, other) = if fade_in { (clip.fade_in, clip.fade_out) } else { (clip.fade_out, clip.fade_in) };
        let most = (clip.length - other).max(0.0);
        let now = (own + beats).clamp(0.0, most);
        if fade_in {
            clip.fade_in = now;
        } else {
            clip.fade_out = now;
        }
        changed += 1;
    }
    Ok(json!({ "clips": changed }))
}

/// `clip.setColorMany`: the clips `clips` names take `color` (or their tracks' again), every one
/// checked before any changes.
fn set_clips_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let color = crate::clip_color::parse_color(params, "color")?;
    let p = &mut session.project;
    let clips = clip_list(p, params)?;
    for (track, index) in &clips {
        let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(*track));
        let clip = lane.and_then(|l| l.clips.get_mut(*index)).ok_or("the clip is missing")?;
        clip.color = color;
    }
    Ok(json!({ "clips": clips.len(), "color": params.get("color") }))
}

/// Removes the clips named by `clips`, which must exist; returns how many went. Indices are read
/// against the project as it was, so a lane loses all its clips at once.
fn remove_clips(p: &mut Project, clips: &[(TrackId, usize)]) -> usize {
    let mut deleted = 0;
    for (track, indexes) in clips_by_track(clips) {
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        let before = lane.clips.len();
        let mut at = 0usize;
        lane.clips.retain(|_| {
            let keep = !indexes.contains(&at);
            at += 1;
            keep
        });
        deleted += before - lane.clips.len();
    }
    deleted
}

/// The clip indexes of `clips`, gathered by the track they are on.
fn clips_by_track(clips: &[(TrackId, usize)]) -> Vec<(TrackId, Vec<usize>)> {
    let mut out: Vec<(TrackId, Vec<usize>)> = Vec::new();
    for (track, index) in clips {
        match out.iter_mut().find(|(t, _)| t == track) {
            Some((_, list)) => list.push(*index),
            None => out.push((*track, vec![*index])),
        }
    }
    out
}

fn duplicate_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let mut made: Vec<Value> = Vec::new();
    for (track, indexes) in clips_by_track(&clips) {
        // The copies, worked out before the lane changes under them.
        let copies: Vec<Clip> = match p.arrangement.lanes.iter().find(|l| l.track == Some(track)) {
            Some(lane) => indexes
                .iter()
                .filter_map(|i| lane.clips.get(*i))
                .map(|c| Clip { start: c.start + c.length, ..c.clone() })
                .collect(),
            None => continue,
        };
        // Each copy is a new clip with an id of its own.
        let mut copies = copies;
        for copy in &mut copies {
            copy.id = p.new_clip_id()?;
        }
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        for copy in copies {
            if !stack {
                make_room(&mut lane.clips, copy.start, copy.start + copy.length);
            }
            let (copy_id, copy_start) = (copy.id, copy.start);
            lane.clips.push(copy);
            lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let index = lane.clips.iter().position(|c| c.id == copy_id).unwrap_or_default();
            made.push(json!({ "clip": copy_id.0, "track": track.0, "index": index, "start": copy_start }));
        }
    }
    Ok(json!({ "clips": made }))
}

fn set_clip_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let mut changed = 0;
    for (track, indexes) in clips_by_track(&clips) {
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        for index in indexes {
            if let Some(clip) = lane.clips.get_mut(index) {
                clip.enabled = enabled;
                changed += 1;
            }
        }
    }
    Ok(json!({ "enabled": enabled, "clips": changed }))
}

fn move_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let index = params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.clips")?;
    let start = number(params, "start")?;
    if start < 0.0 {
        return Err(format!("start must be at least 0, got {start}").into());
    }
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let to = match params.get("toTrack") {
        Some(_) => track_at(&session.project, params, "toTrack")?,
        None => track,
    };
    // The colour the clip shows on its track, kept when it goes to another (`color`).
    let keep = match params.get("color") {
        None | Some(Value::Null) => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    let p = &mut session.project;
    let from_lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let count = p.arrangement.lanes.get(from_lane).map_or(0, |l| l.clips.len());
    let index = usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
    if to != track {
        let clip = p.arrangement.lanes.get(from_lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?;
        let kinds = p.content(clip.content).map_or((false, false), |c| {
            (
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
            )
        });
        let target = p.track(to).ok_or("the target track is missing")?;
        let fits = (!kinds.0 || target.content.notes) && (!kinds.1 || target.content.audio) && (kinds.0 || kinds.1);
        if !fits {
            let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("track {:?} cannot hold {what}", target.name).into());
        }
    }
    let to_lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(to)) {
        Some(lane) => lane,
        None => {
            let name = p.track(to).map(|t| t.name.clone()).unwrap_or_default();
            let id = p.new_lane_id()?;
            p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(to), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let mut clip = p.arrangement.lanes.get_mut(from_lane).ok_or("the lane is missing")?.clips.remove(index);
    clip.start = start;
    if to != track && clip.color.is_none() {
        clip.color = keep;
    }
    let lane = p.arrangement.lanes.get_mut(to_lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, start, start + clip.length);
    }
    // A moved clip is the same clip: it keeps its id, on whichever track it lands.
    let clip_id = clip.id;
    lane.clips.push(clip);
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let new_index = lane.clips.iter().position(|c| c.id == clip_id).unwrap_or_default();
    Ok(json!({ "clip": clip_id.0, "start": start, "track": to.0, "index": new_index }))
}

/// `clip.moveSlot`: the dragged slot's clip goes to the target's slot; an occupied target's
/// clip comes back to the source's slot (the two swap, nothing lost). The target must take the
/// clip's kind: notes or audio.
fn move_slot(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from_column = column_index(session.project(), params)?;
    let from_scene = scene_index(session.project(), params)?;
    let to_track = match params.get("toTrack") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "toTrack")?),
    };
    let to_scene: Option<usize> = match params.get("toScene") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or("toScene must be a whole number from 1")?,
        ),
    };

    let project = &mut session.project;
    let from_track = project.launcher.columns.get(from_column).map(|c| c.track).ok_or("the track has no clip slots")?;
    let to_track = to_track.unwrap_or(from_track);
    let to_scene = to_scene.unwrap_or(from_scene);
    if (to_track, to_scene) == (from_track, from_scene) {
        return Ok(json!({ "moved": false }));
    }
    // The clip's kind must be one the target's track plays: notes to a notes track, audio to
    // an audio track, as `clip.move` checks for the arrangement.
    let clip = project
        .launcher
        .columns
        .get(from_column)
        .and_then(|c| c.slots.get(from_scene))
        .and_then(|s| s.clip.as_ref())
        .ok_or("the slot has no clip to move")?;
    let kinds = project.content(clip.content).map_or((false, false), |c| {
        (
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
        )
    });
    let target = project.track(to_track).ok_or("the target track is missing")?;
    let source_track = project.track(from_track).ok_or("the source track is missing")?;
    let fits = (!kinds.0 || target.content.notes) && (!kinds.1 || target.content.audio) && (kinds.0 || kinds.1);
    if !fits {
        let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
        return Err(format!("track {:?} cannot hold {what}", target.name).into());
    }
    let Some(to_column) = project.launcher.columns.iter().position(|c| c.track == to_track) else {
        return Err(format!("track {:?} has no clip slots", target.name).into());
    };
    // The swap's other direction: a clip coming back to the source's slot must be one the
    // source's track plays, checked before anything moves, so a refusal leaves both as they are.
    let displaced =
        project.launcher.columns.get(to_column).and_then(|c| c.slots.get(to_scene)).and_then(|s| s.clip.as_ref());
    if let Some(displaced) = displaced {
        let kinds = project.content(displaced.content).map_or((false, false), |c| {
            (
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
            )
        });
        let fits = (!kinds.0 || source_track.content.notes)
            && (!kinds.1 || source_track.content.audio)
            && (kinds.0 || kinds.1);
        if !fits {
            let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("track {:?} cannot hold {what}", source_track.name).into());
        }
    }
    let from_clip = project
        .launcher
        .columns
        .get_mut(from_column)
        .and_then(|c| c.slots.get_mut(from_scene))
        .ok_or("the slot is missing")?
        .clip
        .take();
    let from_clip = match from_clip {
        Some(clip) => clip,
        None => return Err("the slot has no clip to move".into()),
    };
    let back = project
        .launcher
        .columns
        .get_mut(to_column)
        .and_then(|c| c.slots.get_mut(to_scene))
        .ok_or("the slot is missing")?
        .clip
        .replace(from_clip);
    let swapped = back.is_some();
    project
        .launcher
        .columns
        .get_mut(from_column)
        .and_then(|c| c.slots.get_mut(from_scene))
        .ok_or("the slot is missing")?
        .clip = back;
    Ok(json!({
        "moved": true,
        "track": to_track.0,
        "scene": to_scene + 1,
        "swapped": swapped,
    }))
}

fn launch_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    let scene = scene_index(session.project(), params)?;
    // A slot that records ends its take instead (docs/spec/recording.md).
    if let Some(ended) = session.end_slot_take(column, scene) {
        return Ok(ended);
    }
    let slot = session.project().launcher.columns.get(column).and_then(|c| c.slots.get(scene));
    let queued = match slot {
        Some(slot) if slot.clip.as_ref().is_some_and(|c| c.enabled) => "launch",
        Some(slot) if slot.clip.is_some() => return Err("the clip in that slot is deactivated".into()),
        Some(slot) if slot.has_stop => "stop",
        _ => return Err("that slot is empty and has no stop button".into()),
    };
    send(session, LaunchCommand::LaunchClip { column, slot: scene })?;
    Ok(json!({ "scene": scene + 1, "queued": queued }))
}
