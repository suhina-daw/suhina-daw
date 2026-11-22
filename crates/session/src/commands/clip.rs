// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `arrangement.*`, `clip.*` commands, and what runs them here.

use super::*;

/// The note starts of a warped clip's file, found already (the window finds them on a thread of
/// its own), so a warp marker command places markers on them without analysing the file again.
const TRANSIENTS: Param = optional(
    "transients",
    ParamType::Numbers,
    "the seconds in the clip's audio file where notes start, as found before (at most 100000, each 0 or more); given, the file is not analysed again",
);

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "arrangement.audioOverlaps",
        label: "List the pairs of audio clips that sound together on one track because their arrangement spans overlap; clips that only touch do not",
        params: &[optional("track", ParamType::Track, "only overlaps on this track (id or name)")],
        kind: CommandKind::READ,
        run: audio_overlaps,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.delete",
        label: "Delete arrangement clips as one undo step, or the clip in a track's slot of a scene",
        params: &[CLIP, DELETE_CLIPS, DELETE_TRACK, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
        run: rename_clip,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.trimMany",
        label: "Move the same edge of several arrangement clips by the same beats as one undo step, each stopping at its own limits as clip.trim does (the selected clips dragged by one edge)",
        params: &[
            CLIPS,
            param("edge", ParamType::Text, "start (the left edge, the content moving with it) or end"),
            param("by", ParamType::Number, "how far each edge moves, in beats; negative moves it left"),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under them as they are; false (default) replaces them",
            ),
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_edges::trim_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.renameMany",
        label: "Give several arrangement clips the same name, as one undo step; clips that play the same content share the name",
        params: &[CLIPS, param("name", ParamType::Text, "the new name (empty clears it)")],
        kind: CommandKind::EDIT,
        run: rename_clips,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.setColorMany",
        label: "Give several clips, in the arrangement or in clip slots, a colour of their own, or their tracks' colour again, as one undo step",
        params: &[CLIPS, param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for each track's color"), GESTURE],
        kind: CommandKind::EDIT,
        run: set_clips_color,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "clip.setGain",
        label: "Set an audio clip's gain, an arrangement clip's or a slot's; only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; to be removed, ADR 0025 › 4)",
            ),
            CLIP_SCENE,
            param("db", ParamType::Decibels, "gain in dB, at most +24; \"-inf\" or below -150 is silence"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
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
                "arrangement clip index on the track, from project.clips (deprecated, use clip; to be removed, ADR 0025 › 4)",
            ),
            optional(
                "scene",
                ParamType::Item,
                "instead of index: the clip in the track's slot of this scene, its number from 0 or {\"id\": n} with its scene_id",
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
        kind: CommandKind::EDIT,
        run: set_clip_fades,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.shiftFades",
        label: "Lengthen or shorten the fade-in or fade-out of several arrangement clips by the same beats, each from its own, kept from 0 to what the clip's other fade leaves, and optionally give that fade one curve; clips with no audio are left as they are; as one undo step",
        params: &[
            CLIPS,
            param("edge", ParamType::Text, "in or out"),
            optional("beats", ParamType::Number, "how much longer (or, below 0, shorter) each fade gets (default 0)"),
            optional(
                "curve",
                ParamType::Number,
                "the curve every one of these fades takes, -1 to +1 (default: as each is)",
            ),
        ],
        kind: CommandKind::EDIT,
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
                ParamType::Item,
                "with track: the clip in this scene's slot, its number from 0 or {\"id\": n} with its scene_id, as in project.launcher",
            ),
        ],
        kind: CommandKind::READ,
        run: copy_clips,
        policy: Policy { after: After::Copies, ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "clip.cut",
        label: "Copy arrangement clips for clip.paste and delete them, as one undo step",
        params: &[CLIPS],
        kind: CommandKind::EDIT,
        run: cut_clips,
        policy: Policy { after: After::Copies, ..Policy::DEFAULT },
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
                ParamType::Item,
                "instead of start: the scene, its number from 0 or {\"id\": n} with its scene_id, whose slot on the track takes the clip",
            ),
            optional("stack", ParamType::Bool, "with start, true leaves clips under it as they are"),
            optional("replace", ParamType::Clip, "with track and start, remove this destination clip before pasting"),
        ],
        kind: CommandKind::EDIT,
        run: paste_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.duplicate",
        label: "Place copies of arrangement clips right after them as one block, moved by the span from the first start to the last end, replacing what the copies cover unless stacked; linked copies keep playing one content, so an edit through any of them changes them all",
        params: &[
            CLIPS,
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copies as they are; false (default) shortens, splits or removes them",
            ),
            optional(
                "linked",
                ParamType::Bool,
                "true makes linked copies, the clips and their copies one content; false (default) independent copies",
            ),
        ],
        kind: CommandKind::EDIT,
        run: duplicate_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.unlink",
        label: "Unlink linked copies: each of these arrangement clips gets a content of its own, as it is now, and the other linked copies stay linked",
        params: &[CLIPS],
        kind: CommandKind::EDIT,
        run: unlink_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.link",
        label: "Link arrangement clips that play the same notes or audio: they play one content from now on, so editing one edits them all",
        params: &[CLIPS],
        kind: CommandKind::EDIT,
        run: link_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setEnabled",
        label: "Turn arrangement clips on or off: a clip that is off is drawn gray with a mute mark and plays nothing",
        params: &[CLIPS, param("enabled", ParamType::Bool, "true plays the clips, false silences them")],
        kind: CommandKind::EDIT,
        run: set_clip_enabled,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setAra",
        label: "Play audio clips through their track's ARA plug-in (such as Melodyne), or no longer: a track without one gets it as its clip host, first in its chain and not in its device list",
        params: &[
            CLIPS,
            param("on", ParamType::Bool, "true plays the clips through the plug-in, false plays them as they are"),
            optional(
                "plugin",
                ParamType::Text,
                "the ARA plug-in a track without one gets, as plugin.list names it (default: the installed one, Melodyne first)",
            ),
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_ara::set_ara,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setColor",
        label: "Give a clip a colour of its own (by id, an arrangement clip or a slot's, or by index), or every clip of a track one when neither is given; \"none\" makes them show the track's colour again",
        params: &[
            CLIP_TRACK,
            param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the track's colour"),
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip); every clip of the track when neither is given",
            ),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
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
                "clip index on the track, from project.clips (deprecated, use clip; to be removed, ADR 0025 › 4)",
            ),
            optional(
                "at",
                ParamType::Number,
                "where to cut, in beats from the beginning of the song, inside the clip; needed unless ats is given",
            ),
            optional(
                "ats",
                ParamType::Numbers,
                "instead of at: several song positions inside the clip, up to 64, cut in one undo step into as many parts plus one; the result lists the parts' clip ids in order",
            ),
        ],
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
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
                "clip index on the track, from project.clips (deprecated, use clip; to be removed, ADR 0025 › 4)",
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
        kind: CommandKind::EDIT,
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
            optional(
                "toScene",
                ParamType::Item,
                "the scene to move it to: its number, from 0, or {\"id\": n} with its scene_id (default: the same scene)",
            ),
        ],
        kind: CommandKind::EDIT,
        run: move_slot,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.record",
        label: "Record into an empty clip slot of an armed track from the next launch boundary (after the count-in when the transport is stopped); clip.launch on the slot ends the take at the next boundary and plays it, looped",
        params: &[TRACK, SCENE],
        kind: CommandKind::TRANSPORT,
        run: crate::recording::record_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.launch",
        label: "Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track)",
        params: &[TRACK, SCENE],
        kind: CommandKind::TRANSPORT,
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
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_shape::set_loop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setAudio",
        label: "Set an audio clip's warp switch, warp mode, transpose or warp mode settings (an arrangement clip, or the clip in a track's slot by scene); only that clip changes, another clip of the same audio keeps its own",
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
            optional(
                "slice",
                ParamType::Text,
                "rhythmic mode: where the audio is cut into slices, transients, 1/4, 1/8, 1/16 or 1/32",
            ),
            optional(
                "tail",
                ParamType::Text,
                "rhythmic mode: what fills the gap after a slice played slower than its audio, off, loop or back-and-forth",
            ),
            optional(
                "envelope",
                ParamType::Number,
                "rhythmic mode: how level a slice stays over its length, 0 to 100 percent (100 does not fade it)",
            ),
            optional("grain", ParamType::Number, "tonal and texture modes: a grain's length, 10 to 400 milliseconds"),
            optional("scatter", ParamType::Number, "texture mode: how far grains move at random, 0 to 100 percent"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_shape::set_audio,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setEnvelope",
        label: "Set an audio clip's gain envelope (an arrangement clip, or the clip in a track's slot by scene): points of a content beat and an offset in dB from the clip's gain, straight in dB between them or along each point's curve; an empty list clears it. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param(
                "points",
                ParamType::Numbers,
                "the points as pairs, a beat of the content (from 0) and then dB from the clip's gain (-70 to +24): [0, 0, 4, -12] falls 12 dB over the first four beats; two points on one beat make a step; two in one place (same beat and dB) are one, and a third on a beat replaces the second",
            ),
            optional(
                "curves",
                ParamType::Curves,
                "each point's curve to the next, in the order of points, as automation.movePoint's curve: a bend from -1 to 1 or [x1, y1, x2, y2] each from -0.5 to 1.5; null or missing for straight",
            ),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::set_envelope,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setTempo",
        label: "Set the tempo an audio clip's audio is warped from, and where that tempo came from: the audio fills the beats it holds at that tempo (a higher tempo plays slower against the song), the clip's length follows and its start stays; warp turns on. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("bpm", ParamType::Number, "the audio's tempo in beats per minute, 20 to 999"),
            optional(
                "source",
                ParamType::Text,
                "where the tempo came from: metadata, name, detected or manual (the default, a tempo typed, halved or doubled)",
            ),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::set_tempo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.addWarpMarker",
        label: "Add a warp marker to a warped audio clip (an arrangement clip, or the clip in a track's slot by scene) at a beat of its content: it pins the sound playing there to that beat, so nothing moves until the marker is moved. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param(
                "beat",
                ParamType::Number,
                "the content beat to pin the sound at (a beat that has a marker changes nothing)",
            ),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::add_warp_marker,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.addWarpMarkersAtTransients",
        label: "Add a warp marker at every note start of a warped audio clip's file (where its spectrum rises as the pitches sounding change or the level jumps, each moved onto a sharp level attack close by), each pinning the sound playing there to its beat, so nothing moves until a marker is moved. A transient near a marker the clip has keeps that marker. One undo step; only that clip changes",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, TRANSIENTS],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::add_warp_markers_at_transients,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.moveWarpMarker",
        label: "Move a warp marker of a warped audio clip from one content beat to another: the sound it is pinned to goes with it, so what plays between it and its neighbours stretches or squeezes. It stays between its neighbours. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("from", ParamType::Number, "the content beat the marker is at now"),
            param("beat", ParamType::Number, "the content beat to move it to"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::move_warp_marker,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.removeWarpMarker",
        label: "Remove the warp marker of a warped audio clip at a content beat; the audio between its neighbours then runs evenly. A warped clip keeps at least two markers. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("beat", ParamType::Number, "the content beat of the marker"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::remove_warp_marker,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.moveWarpMarkers",
        label: "Move several warp markers of a warped audio clip by the same number of beats at once, each with the sound it is pinned to. The markers not moved stay; a moved one stays between them. One undo step; only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("beats", ParamType::Numbers, "the content beats the markers are at now"),
            param("by", ParamType::Number, "how many beats to move them, later (positive) or earlier (negative)"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::move_warp_markers,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.quantizeWarpMarkers",
        label: "Quantize a warped audio clip's timing: each transient (where its sound starts) in the range gets a warp marker where it plays now, then every marker there moves towards the nearest grid line, its sound with it. One undo step; only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("grid", ParamType::Text, "the grid, as note.quantize takes it: 1/16, 1/8T, 1 bar, ..."),
            optional("amount", ParamType::Number, "how far towards the line, 0 to 100 percent (default 100: onto it)"),
            optional(
                "start",
                ParamType::Number,
                "the content beat the range starts at (default: where the clip starts playing its content, its offset or its loop's start when earlier)",
            ),
            optional(
                "end",
                ParamType::Number,
                "the content beat the range ends at (default: where the clip stops playing its content, its offset plus length or, looping, its loop's end)",
            ),
            TRANSIENTS,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::quantize_warp_markers,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.removeWarpMarkers",
        label: "Remove several warp markers of a warped audio clip at once; the audio between the markers left then runs evenly. A warped clip keeps at least two markers. One undo step; only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("beats", ParamType::Numbers, "the content beats of the markers"),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::remove_warp_markers,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.resetWarp",
        label: "Reset an audio clip's warp to two markers, one at each end of its audio, so it runs evenly: at the tempo the clip has, or with detect at the tempo found in the file's metadata, name or sound. Warp turns on; the clip's start, length and loop stay. Only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional(
                "detect",
                ParamType::Bool,
                "true reads the tempo again from the file (metadata, name, then detection) instead of keeping the clip's",
            ),
            optional(
                "transients",
                ParamType::Numbers,
                "the note starts of the clip's audio file, in seconds, as the other warp marker commands take them: checked, but not needed, as the tempo is detected from the sound's beat rather than from its note starts",
            ),
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_audio::reset_warp,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setSwing",
        label: "Set a MIDI clip's own swing, which it then plays instead of the project's, or make it follow the project's swing again (it keeps its own amount for later); only that clip changes",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional("amount", ParamType::Number, "the clip's own swing in percent, 0 to 100; the clip then plays it"),
            optional(
                "global",
                ParamType::Bool,
                "true: the clip follows the project's swing; false: it plays its own amount again",
            ),
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::swing::set_clip_swing,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.findTempo",
        label: "What each source says an audio clip's tempo is (its file's metadata, its file's name, detection in its sound; null where one finds nothing), and the tempo the clip has now with its source",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::READ,
        run: crate::clip_audio::find_tempo,
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
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_shape::set_length,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.crop",
        label: "Crop a MIDI clip to a stretch of its content (the piano roll's time selection): its start marker and length become the stretch's, its loop is cut to it (off where they do not meet), notes outside it are deleted and those across an edge cut at it. An arrangement clip moves with its start marker, so what is left plays where it played; a looping one keeps its end",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("start", ParamType::Number, "the stretch's start in content beats, 0 to 120000"),
            param("end", ParamType::Number, "the stretch's end in content beats, after start"),
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_edits::crop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setOrigin",
        label: "Make a content beat of a MIDI clip its first beat (1.1.1 on the clip's own ruler): every note, the start marker and the loop move back by it together, so the clip sounds and sits in the song as before. Refused when a note, the start marker or the loop's start lies before the beat, and on a clip that swings when the beat is not on an eighth (a multiple of 0.5)",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("beat", ParamType::Number, "the content beat that becomes the first, 0 to 120000"),
        ],
        kind: CommandKind::EDIT,
        run: crate::clip_edits::set_origin,
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
            GESTURE,
        ],
        kind: CommandKind::EDIT,
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
                "nothing, stop, again, previous, next, first, last, any, other, or \"jump N\" for the slot of scene N (N from 1, as the window shows it; the scene parameter counts from 0)",
            ),
            optional("actionB", ParamType::Text, "like actionA"),
            optional("chanceA", ParamType::Integer, "weight of action A, 0 to 100"),
            optional("chanceB", ParamType::Integer, "weight of action B, 0 to 100"),
        ],
        kind: CommandKind::EDIT,
        run: follow::set_follow_action,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.release",
        label: "Release a slot's launch button (clip.launch is the press): stops a clip in gate mode, ends the repeats of one in repeat mode",
        params: &[TRACK, SCENE],
        kind: CommandKind::TRANSPORT,
        run: launch::release_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLaunchMode",
        label: "Set what launching a slot's clip does: trigger (plays until stopped), gate (plays while held), toggle (a second launch stops it) or repeat (relaunches at its quantisation while held)",
        params: &[TRACK, SCENE, param("mode", ParamType::Text, "trigger, gate, toggle or repeat")],
        kind: CommandKind::EDIT,
        run: launch::set_launch_mode,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setLegato",
        label: "Set whether a slot's clip starts at the position of the clip it replaces on its track (legato) instead of at its start",
        params: &[TRACK, SCENE, param("on", ParamType::Bool, "true for legato")],
        kind: CommandKind::EDIT,
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
        kind: CommandKind::EDIT,
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
            optional(
                "after",
                ParamType::Track,
                "with no track: put the new tracks after this one, in its group (default: after the last regular track)",
            ),
            optional("start", ParamType::Number, "arrangement beat the first clip starts at"),
            optional(
                "scene",
                ParamType::Item,
                "the scene of the first clip slot: its number, from 0, or {\"id\": n} with its scene_id",
            ),
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
        kind: CommandKind::EDIT,
        run: files::add_files,
        policy: Policy { after: After::Recent(Recent::Files), ..Policy::CRITICAL },
    },
    CommandSpec {
        id: "clip.create",
        label: "Make an empty clip on a track that holds clips: MIDI on a notes track, audio with no file yet on an audio track; in an empty clip slot, or at a beat in the arrangement, replacing what it covers unless stacked",
        params: &[
            TRACK,
            optional(
                "scene",
                ParamType::Item,
                "the scene of an empty clip slot: its number, from 0, or {\"id\": n} with its scene_id",
            ),
            optional("start", ParamType::Number, "arrangement beat it starts at, at least 0"),
            optional("length", ParamType::Number, "length in beats (default one bar of the time signature)"),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new one covers (default false)"),
        ],
        kind: CommandKind::EDIT,
        run: files::create_clip,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.scale",
        label: "The scale of a MIDI clip (root and mode, or null for none) and the project's default scale for new clips",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::READ,
        run: crate::scales::clip_scale,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "clip.setScale",
        label: "Set the scale of a clip: a MIDI clip's marks the piano roll's keys, an audio clip's says the key of its audio; clear it with off; a content other clips also play is copied for this clip first",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            SCALE_ROOT,
            SCALE_MODE,
            optional("off", ParamType::Bool, "true clears the scale; not with root or mode"),
        ],
        kind: CommandKind::EDIT,
        run: crate::scales::set_scale,
        policy: Policy::DEFAULT,
    },
];

/// `arrangement.audioOverlaps`: [`Project::audio_overlaps`], each clip named as `clip.move`
/// addresses it (ADR 0033).
fn audio_overlaps(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let only = match Args::new(params).value("track") {
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

/// The clip `params` names: by `clip`, or an arrangement clip by `index`, or the clip in a
/// launcher slot by `scene`.
fn clip_at(p: &Project, params: &Value) -> Result<ClipAt, CommandError> {
    if let Some(named) = named_clip(p, params)? {
        return Ok(match named.at {
            crate::notes::ClipAt::Arrangement(lane, index) => ClipAt::Arrangement { lane, index },
            crate::notes::ClipAt::Slot(column, scene) => ClipAt::Slot { column, scene },
        });
    }
    match (Args::new(params).value("index"), Args::new(params).value("scene")) {
        (Some(_), Some(_)) => Err("give index or scene, not both".into()),
        (None, Some(_)) => {
            let (column, scene) = (column_index(p, params)?, scene_index(p, params)?);
            match p.launcher.columns.get(column).and_then(|c| c.slots.get(scene)) {
                Some(slot) if slot.clip.is_some() => Ok(ClipAt::Slot { column, scene }),
                _ => Err(format!("the slot of scene {} is empty", scene).into()),
            }
        }
        (Some(_), None) => lane_and_index(p, params).map(|(lane, index)| ClipAt::Arrangement { lane, index }),
        (None, None) => Err("give index (an arrangement clip) or scene (a launcher clip)".into()),
    }
}

fn set_clip_gain(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = Args::new(params).f64_or_neg_infinity("db")?;
    if db > 24.0 {
        return Err(format!("db must be at most +24, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let (at, content, _) = crate::notes::locate(&session.project, params)?;
    if crate::audio_timing::region(&session.project, content).is_none() {
        return Err("only an audio clip has a gain".into());
    }
    // Only this clip: every clip has its own gain (#742).
    crate::notes::clip_mut(&mut session.project, at).ok_or("the clip is missing")?.audio.gain = gain;
    Ok(json!({ "gain_db": to_db(gain) }))
}

fn set_clip_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let length_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match Args::new(params).value(name) {
            None => Ok(None),
            Some(_) => {
                let beats = Args::new(params).f64(name)?;
                if beats < 0.0 {
                    return Err(format!("{name} must be at least 0 beats, got {beats}").into());
                }
                Ok(Some(beats))
            }
        }
    };
    let shape_of = |name: &str| -> Result<Option<FadeShape>, CommandError> {
        match Args::new(params).value(name) {
            None => Ok(None),
            Some(Value::String(s)) => FadeShape::from_name(s).map(Some).ok_or_else(|| {
                let names: Vec<&str> = FadeShape::ALL.iter().map(|s| s.name()).collect();
                format!("{name} must be one of {}, got {s:?}", names.join(", ")).into()
            }),
            Some(_) => Err(format!("{name} must be a shape name").into()),
        }
    };
    let curve_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match Args::new(params).value(name) {
            None => Ok(None),
            Some(_) => {
                let curve = Args::new(params).f64(name)?;
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
    let named = named_clip(p, params)?;
    let source = match (named, Args::new(params).value("clips"), Args::new(params).value("scene")) {
        (Some(NamedClip { track, at: crate::notes::ClipAt::Arrangement(_, index), .. }), ..) => {
            Copied::Clips(vec![(track, index)])
        }
        (Some(NamedClip { at: crate::notes::ClipAt::Slot(column, scene), .. }), ..) => Copied::Slot(column, scene),
        (None, Some(_), None) => {
            if Args::new(params).has("track") {
                return Err("track goes with scene (a clip slot); clips names arrangement clips".into());
            }
            Copied::Clips(clip_list(p, params)?)
        }
        (None, None, Some(_)) => Copied::Slot(column_index(p, params)?, scene_index(p, params)?),
        _ => return Err("give either clips (arrangement clips) or track and scene (a clip slot)".into()),
    };
    match source {
        Copied::Clips(clips) => {
            for (track, index) in clips {
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
        Copied::Slot(column, scene) => {
            let slot = p.launcher.columns.get(column).and_then(|c| c.slots.get(scene));
            let clip =
                slot.and_then(|s| s.clip.clone()).ok_or_else(|| format!("the slot in scene {} is empty", scene))?;
            let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
            copied.push((clip, content));
        }
    }
    let names: Vec<String> = copied.iter().map(|(_, content)| content.name.clone()).collect();
    // The automation under each arrangement clip goes with it.
    let under = from
        .iter()
        .zip(&copied)
        .map(|(track, (clip, _))| crate::automation_range::under(session, *track, clip.start, clip.start + clip.length))
        .collect();
    session.clipboards.clips = copied;
    session.clipboards.tracks = from;
    session.clipboards.under = under;
    session.clipboards.span = None;
    Ok(json!({ "copied": names }))
}

/// What `clip.copy` copies: arrangement clips by track and index, or a slot's clip by column and
/// scene.
enum Copied {
    Clips(Vec<(TrackId, usize)>),
    Slot(usize, usize),
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
    // A time selection copied with only automation in it pastes onto its own tracks.
    let span_only = session.clipboards.clips.is_empty() && session.clipboards.span.is_some();
    if session.clipboards.clips.is_empty() && !span_only {
        return Err("nothing has been copied".into());
    }
    let at = match (Args::new(params).value("start"), Args::new(params).value("scene")) {
        (Some(_), None) => {
            let start = Args::new(params).f64("start")?;
            if !(0.0..=MAX_START_BEAT).contains(&start) {
                return Err(format!("start must be from 0 to {MAX_START_BEAT}, got {start}").into());
            }
            PasteAt::Beat(start)
        }
        (None, Some(_)) => {
            if Args::new(params).has("stack") {
                return Err("stack is for the arrangement (start); a slot holds one clip".into());
            }
            PasteAt::Slot(scene_index(&session.project, params)?)
        }
        _ => return Err("give either start (a beat in the arrangement) or scene (a clip slot)".into()),
    };
    let stack = match Args::new(params).value("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let replace = Args::new(params).value("replace").map(|value| clip_param(&session.project, value)).transpose()?;
    if replace.is_some() && (!Args::new(params).has("track") || !matches!(at, PasteAt::Beat(_))) {
        return Err("replace requires an arrangement track and start".into());
    }
    // No track: each copied clip goes back to the track it came from; so does a time
    // selection's automation, with or without clips.
    if (!Args::new(params).has("track") || span_only)
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
        p.contents.push(suhina_model::ClipContent { id, linked: false, ..content });
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
        let scene_id = scene_id(p, scene);
        return Ok(json!({
            "clip": placed_id.0,
            "track": track.0,
            "scene": scene,
            "scene_id": scene_id,
            "replaced": replaced,
        }));
    }
    let start = match at {
        PasteAt::Beat(start) => start,
        PasteAt::Slot(_) => 0.0,
    };
    // The earliest copied clip lands on `start`; the others keep their places relative to it.
    let earliest = session.clipboards.clips.iter().map(|(clip, _)| clip.start).fold(f64::INFINITY, f64::min);
    let offset = if earliest.is_finite() { start - earliest } else { 0.0 };
    let copied = session.clipboards.clips.clone();
    let under = session.clipboards.under.clone();
    let sources = session.clipboards.tracks.clone();
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
    if let Some(replace) = replace {
        let clips = &mut p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?.clips;
        let index = clips
            .iter()
            .position(|clip| clip.id == replace)
            .ok_or("the replacement clip is not on the destination track")?;
        clips.remove(index);
    }
    let mut placed: Vec<Value> = Vec::with_capacity(copied.len());
    let mut take_spans = Vec::new();
    for (i, (clip, content)) in copied.into_iter().enumerate() {
        // A part of a take in this project stays one, and takes its takes along (#1078); what came
        // from another project does not know them.
        let span = sources
            .get(i)
            .filter(|_| p.content(clip.content) == Some(&content))
            .and_then(|from| crate::take_carry::span_of_part(p, *from, track, &clip, offset));
        take_spans.extend(span);
        // Any other copy plays its own content, so editing one does not change the other.
        let id = if span.is_some() {
            clip.content
        } else {
            let id = new_content_id(p)?;
            p.contents.push(suhina_model::ClipContent { id, linked: false, ..content });
            id
        };
        let at = clip.start + offset;
        let mut copy = Clip { id: p.new_clip_id()?, content: id, start: at, ..clip };
        copy.fit_fades();
        let stack = stack && !p.lane_takes_cover(lane, at, at + copy.length);
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
    crate::take_carry::carry(p, &take_spans, true);
    // The automation that was under each clip, under its copy.
    for (i, (clip, _)) in session.clipboards.clips.clone().iter().enumerate() {
        if let Some(lanes) = under.get(i) {
            let same = sources.get(i) == Some(&track);
            crate::automation_range::lay_under(session, track, clip.start + offset, lanes, same)?;
        }
    }
    Ok(json!({ "clips": placed }))
}

fn delete_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // A list of arrangement clips goes as one; otherwise a launcher slot's clip is named by
    // `scene`, an arrangement clip by `index`, the way the note commands name the clip they edit;
    // any one clip by `clip`.
    match named_clip(&session.project, params)?.map(|named| named.at) {
        Some(crate::notes::ClipAt::Arrangement(lane, index)) => {
            if let Some(lane) = session.project.arrangement.lanes.get_mut(lane) {
                lane.clips.remove(index);
            }
            return Ok(json!({ "deleted": index }));
        }
        Some(crate::notes::ClipAt::Slot(column, scene)) => {
            let slot = session
                .project
                .launcher
                .columns
                .get_mut(column)
                .and_then(|c| c.slots.get_mut(scene))
                .ok_or("the slot is missing")?;
            slot.clip = None;
            return Ok(json!({ "deleted": scene }));
        }
        None => {}
    }
    if Args::new(params).has("clips") {
        if ["track", "index", "scene"].iter().any(|k| Args::new(params).has(k)) {
            return Err("give clips, or track with index or scene, not both".into());
        }
        let clips = clip_list(&session.project, params)?;
        let deleted = remove_clips(&mut session.project, &clips);
        return Ok(json!({ "deleted": deleted }));
    }
    let whole = |key: &str| -> Result<Option<u64>, CommandError> {
        match Args::new(params).value(key) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{key} must be a whole number").into()),
        }
    };
    match (whole("index")?, Args::new(params).has("scene").then_some(())) {
        (Some(_), Some(())) => Err("give index or scene, not both".into()),
        (None, None) => Err("give clips, or track with index (an arrangement clip) or scene (a launcher clip)".into()),
        (Some(_), None) => {
            let (lane, index) = lane_and_index(&session.project, params)?;
            if let Some(lane) = session.project.arrangement.lanes.get_mut(lane) {
                lane.clips.remove(index);
            }
            Ok(json!({ "deleted": index }))
        }
        (None, Some(())) => {
            let column = column_index(session.project(), params)?;
            let s = scene_index(session.project(), params)?;
            let scene = s;
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
    let name = match Args::new(params).value("name") {
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
    let fade_in = match Args::new(params).raw_str("edge") {
        Some("in") => true,
        Some("out") => false,
        _ => return Err("edge must be in or out".into()),
    };
    let beats = if Args::new(params).has("beats") { Args::new(params).f64("beats")? } else { 0.0 };
    let curve = match Args::new(params).value("curve") {
        None => None,
        Some(_) => {
            let curve = Args::new(params).f64("curve")?;
            if !(-1.0..=1.0).contains(&curve) {
                return Err(format!("curve must be from -1 to +1, got {curve}").into());
            }
            Some(curve)
        }
    };
    if !Args::new(params).has("beats") && curve.is_none() {
        return Err("give beats, curve or both".into());
    }
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
        if let Some(curve) = curve {
            if fade_in {
                clip.fade_in_curve.curve = curve;
            } else {
                clip.fade_out_curve.curve = curve;
            }
        }
        changed += 1;
    }
    Ok(json!({ "clips": changed }))
}

/// `clip.setColorMany`: the clips `clips` names take `color` (or their tracks' again), every one
/// checked before any changes. A clip named by id may be in a clip slot; the deprecated
/// `{track, index}` form names arrangement clips.
fn set_clips_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let color = crate::clip_color::parse_color(params, "color")?;
    let p = &mut session.project;
    let places = color_clip_places(p, params)?;
    for at in &places {
        crate::notes::clip_mut(p, *at).ok_or("the clip is missing")?.color = color;
    }
    Ok(json!({ "clips": places.len(), "color": Args::new(params).value("color") }))
}

/// Where each clip `params.clips` names is, arrangement clips and slots' clips alike, each once.
fn color_clip_places(p: &Project, params: &Value) -> Result<Vec<crate::notes::ClipAt>, CommandError> {
    let list =
        Args::new(params).value("clips").and_then(Value::as_array).ok_or("clips must be a list of { \"clip\": id }")?;
    let mut out: Vec<crate::notes::ClipAt> = Vec::new();
    for entry in list {
        let at = if entry.get("clip").is_some() {
            match named_clip(p, entry)? {
                Some(named) => named.at,
                None => return Err("each clip in clips is { \"clip\": id }".into()),
            }
        } else {
            let one = json!({ "clips": [entry] });
            let (track, index) = clip_list(p, &one)?.first().copied().ok_or("each clip in clips names a clip")?;
            let lane = p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the clip is missing")?;
            crate::notes::ClipAt::Arrangement(lane, index)
        };
        if !out.contains(&at) {
            out.push(at);
        }
    }
    if out.is_empty() {
        return Err("clips must name at least one clip".into());
    }
    Ok(out)
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
    let stack = match Args::new(params).value("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let linked = match Args::new(params).value("linked") {
        None => false,
        Some(v) => v.as_bool().ok_or("linked must be true or false")?,
    };
    let clips = clip_list(&session.project, params)?;
    // The clips go as one block, on every track at once: each copy keeps its place among the
    // others, moved by the span from the first start to the last end, so copies never land on the
    // clips they copy (one clip: right after it).
    let spans: Vec<(f64, f64)> = clips
        .iter()
        .filter_map(|(track, index)| {
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track))?;
            lane.clips.get(*index).map(|c| (c.start, c.start + c.length))
        })
        .collect();
    let first = spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
    let last = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
    let shift = if last > first { last - first } else { 0.0 };
    // The automation under each clip, laid under its copy once the copies are placed.
    let carried: Vec<(TrackId, f64, Vec<crate::automation_range::Under>)> = clips
        .iter()
        .filter_map(|(track, index)| {
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track))?;
            let clip = lane.clips.get(*index)?;
            let end = clip.start + clip.length;
            Some((*track, clip.start + shift, crate::automation_range::under(session, *track, clip.start, end)))
        })
        .collect();
    let p = &mut session.project;
    let mut made: Vec<Value> = Vec::new();
    for (track, indexes) in clips_by_track(&clips) {
        // The copies, worked out before the lane changes under them.
        let copies: Vec<Clip> = match p.arrangement.lanes.iter().find(|l| l.track == Some(track)) {
            Some(lane) => indexes
                .iter()
                .filter_map(|i| lane.clips.get(*i))
                .map(|c| Clip { start: c.start + shift, ..c.clone() })
                .collect(),
            None => continue,
        };
        // Each copy is a new clip with an id of its own. A linked copy plays its clip's content and
        // links it; an independent copy of a linked clip gets a content of its own, so it is not
        // one of the linked copies (`docs/spec/arrangement-editing.md` › Linked copies).
        let mut copies = copies;
        for copy in &mut copies {
            copy.id = p.new_clip_id()?;
            if linked {
                if let Some(content) = p.contents.iter_mut().find(|c| c.id == copy.content) {
                    content.linked = true;
                }
            } else if p.content(copy.content).is_some_and(|c| c.linked) {
                let id = new_content_id(p)?;
                let content = p.content(copy.content).cloned().ok_or("the clip's content is missing")?;
                p.contents.push(suhina_model::ClipContent { id, linked: false, ..content });
                copy.content = id;
            }
        }
        // A part of a take takes what the takes hold under it along (#1078).
        let take_spans: Vec<_> = copies
            .iter()
            .filter_map(|c| {
                let original = Clip { start: c.start - shift, ..c.clone() };
                crate::take_carry::span_of_part(p, track, track, &original, c.length)
            })
            .collect();
        let covered: Vec<bool> = copies.iter().map(|c| p.takes_cover(track, c.start, c.start + c.length)).collect();
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        for (copy, covered) in copies.into_iter().zip(covered) {
            if !stack || covered {
                make_room(&mut lane.clips, copy.start, copy.start + copy.length);
            }
            let (copy_id, copy_start) = (copy.id, copy.start);
            lane.clips.push(copy);
            lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let index = lane.clips.iter().position(|c| c.id == copy_id).unwrap_or_default();
            made.push(json!({ "clip": copy_id.0, "track": track.0, "index": index, "start": copy_start }));
        }
        crate::take_carry::carry(p, &take_spans, true);
    }
    for (track, at, lanes) in &carried {
        crate::automation_range::lay_under(session, *track, *at, lanes, true)?;
    }
    Ok(json!({ "clips": made }))
}

/// `clip.unlink {clips}` (#1414): each linked clip named gets its own copy of the content; a
/// content left with one clip is no longer linked. A clip that is not linked is left as it is.
fn unlink_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let mut unlinked = Vec::new();
    let mut left = Vec::new();
    for (track, indexes) in clips_by_track(&clips) {
        for index in indexes {
            let Some(content) = p
                .arrangement
                .lanes
                .iter()
                .find(|l| l.track == Some(track))
                .and_then(|l| l.clips.get(index))
                .map(|c| c.content)
            else {
                continue;
            };
            if !p.content(content).is_some_and(|c| c.linked) {
                continue;
            }
            let id = new_content_id(p)?;
            let copy = p.content(content).cloned().ok_or("the clip's content is missing")?;
            p.contents.push(suhina_model::ClipContent { id, linked: false, ..copy });
            let clip = p
                .arrangement
                .lanes
                .iter_mut()
                .find(|l| l.track == Some(track))
                .and_then(|l| l.clips.get_mut(index))
                .ok_or("the clip is missing")?;
            clip.content = id;
            unlinked.push(clip.id.0);
            left.push(content);
        }
    }
    // A content that one clip plays is no longer a linked copy of anything.
    for content in left {
        if crate::notes::placements(p, content) <= 1
            && let Some(c) = p.contents.iter_mut().find(|c| c.id == content)
        {
            c.linked = false;
        }
    }
    Ok(json!({ "unlinked": unlinked }))
}

/// `clip.link {clips}`: arrangement clips whose contents hold the same notes or audio (every
/// lane, its length) play the first one's content from now on, a linked content; the contents
/// no clip, slot or take plays any more go. Clips that play different things are refused: no
/// edit is lost.
fn link_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let content_of = |p: &suhina_model::Project, (track, index): (suhina_model::TrackId, usize)| {
        p.arrangement.lanes.iter().find(|l| l.track == Some(track)).and_then(|l| l.clips.get(index)).map(|c| c.content)
    };
    let contents: Vec<suhina_model::ContentId> =
        clips.iter().map(|&at| content_of(p, at).ok_or("the clip is missing")).collect::<Result<_, _>>()?;
    let Some(&first) = contents.first() else { return Err("clips must name at least two clips".into()) };
    if contents.iter().all(|c| *c == first) {
        return Err("the clips already play one content".into());
    }
    let kept = p.content(first).cloned().ok_or("the clip's content is missing")?;
    for content in &contents {
        let other = p.content(*content).ok_or("the clip's content is missing")?;
        if !same_material(&kept, other) {
            return Err("only clips that play the same notes or audio can be linked".into());
        }
    }
    let mut linked = Vec::new();
    for &(track, index) in &clips {
        if let Some(clip) =
            p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)).and_then(|l| l.clips.get_mut(index))
        {
            clip.content = first;
            linked.push(clip.id.0);
        }
    }
    if let Some(content) = p.contents.iter_mut().find(|c| c.id == first) {
        content.linked = true;
    }
    // A content nothing plays any more is dropped, so it is not left in the file.
    let gone: Vec<suhina_model::ContentId> = contents
        .iter()
        .copied()
        .filter(|c| *c != first)
        .filter(|c| crate::notes::placements(p, *c) == 0)
        .filter(|c| !p.arrangement.takes.iter().flat_map(|t| &t.clips).any(|clip| clip.content == *c))
        .collect();
    p.contents.retain(|c| !gone.contains(&c.id));
    Ok(json!({ "linked": linked, "content": first.0 }))
}

/// Whether two contents hold the same material: the same lanes (notes or audio, for the same
/// targets) and the same length. Their names and ids may differ.
pub(crate) fn same_material(a: &suhina_model::ClipContent, b: &suhina_model::ClipContent) -> bool {
    a.length == b.length && a.lanes == b.lanes && a.scale == b.scale
}

fn set_clip_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = Args::new(params).bool("enabled")?;
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
    let (track, index) = match named_arrangement_clip(&session.project, params)? {
        Some((track, _, index)) => (track, u64::try_from(index).map_err(|_| "the clip index is too large")?),
        None => (
            track_id(&session.project, params)?,
            Args::new(params)
                .value("index")
                .and_then(Value::as_u64)
                .ok_or("index must be a whole number from project.clips")?,
        ),
    };
    let start = Args::new(params).f64("start")?;
    if start < 0.0 {
        return Err(format!("start must be at least 0, got {start}").into());
    }
    let stack = match Args::new(params).value("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let to = match Args::new(params).value("toTrack") {
        Some(_) => track_at(&session.project, params, "toTrack")?,
        None => track,
    };
    // The colour the clip shows on its track, kept when it goes to another (`color`).
    let keep = match Args::new(params).value("color") {
        None => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    // An empty track of the other kind takes the clip's (#945).
    if to != track
        && let Some(content) = session
            .project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(track))
            .and_then(|l| usize::try_from(index).ok().and_then(|i| l.clips.get(i)))
            .map(|c| c.content)
    {
        crate::track_kind::fit_empty_for(session, to, content)?;
    }
    // The automation under the clip, taken along (#916).
    let carried = session
        .project
        .arrangement
        .lanes
        .iter()
        .find(|l| l.track == Some(track))
        .and_then(|l| usize::try_from(index).ok().and_then(|i| l.clips.get(i)))
        .map(|c| (c.start, c.start + c.length))
        .map(|(from, end)| (from, end, crate::automation_range::under(session, track, from, end)));
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
    // A part of a comp takes what its takes hold under it along (#1078).
    let spans: Vec<_> = crate::take_carry::span_of_part(p, track, to, &clip, start - clip.start).into_iter().collect();
    clip.start = start;
    if to != track && clip.color.is_none() {
        clip.color = keep;
    }
    let stack = stack && !p.takes_cover(to, start, start + clip.length);
    let lane = p.arrangement.lanes.get_mut(to_lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, start, start + clip.length);
    }
    // A moved clip is the same clip: it keeps its id, on whichever track it lands.
    let clip_id = clip.id;
    lane.clips.push(clip);
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let new_index = lane.clips.iter().position(|c| c.id == clip_id).unwrap_or_default();
    crate::take_carry::carry(p, &spans, false);
    if let Some((from, end, lanes)) = carried {
        crate::automation_range::clear_under(session, track, from, end, &lanes)?;
        crate::automation_range::lay_under(session, to, start, &lanes, to == track)?;
    }
    Ok(json!({ "clip": clip_id.0, "start": start, "track": to.0, "index": new_index }))
}

/// `clip.moveSlot`: the dragged slot's clip goes to the target's slot; an occupied target's
/// clip comes back to the source's slot (the two swap, nothing lost). The target must take the
/// clip's kind: notes or audio.
fn move_slot(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from_column = column_index(session.project(), params)?;
    let from_scene = scene_index(session.project(), params)?;
    let to_track = match Args::new(params).value("toTrack") {
        None => None,
        Some(_) => Some(track_at(&session.project, params, "toTrack")?),
    };
    let to_scene = opt_scene_at(&session.project, params, "toScene")?;

    // An empty track of the other kind takes the clip's (#945).
    let from = session.project.launcher.columns.get(from_column).map(|c| c.track);
    if let Some(to) = to_track.filter(|t| Some(*t) != from)
        && let Some(content) = session
            .project
            .launcher
            .columns
            .get(from_column)
            .and_then(|c| c.slots.get(from_scene))
            .and_then(|s| s.clip.as_ref())
            .map(|c| c.content)
    {
        crate::track_kind::fit_empty_for(session, to, content)?;
    }
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
        "scene": to_scene,
        "scene_id": scene_id(project, to_scene),
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
    Ok(json!({ "scene": scene, "scene_id": scene_id(session.project(), scene), "queued": queued }))
}
