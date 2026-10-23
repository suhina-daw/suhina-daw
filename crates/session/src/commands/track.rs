// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `track.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "track.setVolume",
        label: "Set the volume of each named track",
        params: &[
            TRACKS,
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
            CONTINUE,
        ],
        kind: CommandKind::Edit,
        run: set_volume,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.adjustVolume",
        label: "Move the volume of each named track by the same number of decibels, keeping their differences",
        params: &[TRACKS, param("db", ParamType::Number, "how far to move it, in dB; positive is louder")],
        kind: CommandKind::Edit,
        run: adjust_volume,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setPan",
        label: "Set the pan of each named track",
        params: &[TRACKS, param("pan", ParamType::Number, "-1 is full left, 0 centre, 1 full right"), CONTINUE],
        kind: CommandKind::Edit,
        run: set_pan,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.adjustPan",
        label: "Move the pan of each named track by the same amount, keeping their differences",
        params: &[TRACKS, param("pan", ParamType::Number, "how far to move it, -1 … +1; positive is right")],
        kind: CommandKind::Edit,
        run: adjust_pan,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setMute",
        label: "Mute or unmute each named track",
        params: &[TRACKS, param("mute", ParamType::Bool, "true mutes the tracks")],
        kind: CommandKind::Edit,
        run: set_mute,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.group",
        label: "Put tracks into a new group track, which plays into where they played",
        params: &[
            param("tracks", ParamType::Tracks, "the tracks to group (ids or names), all in the same group or none"),
            optional("name", ParamType::Text, "the group's name (\"Group\" if left out)"),
        ],
        kind: CommandKind::Edit,
        run: group_tracks,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setGroup",
        label: "Move a track (a group with its tracks) into a group track, or out of every group, and play into it",
        params: &[
            TRACK,
            param("group", ParamType::Track, "a group track (id or name) to move into, or \"none\" for the top level"),
            optional(
                "index",
                ParamType::Integer,
                "its place among the tracks directly in that group (or at the top level), from 0; the end if left out",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_group,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.ungroup",
        label: "Dissolve a group track: its tracks stay, playing into where the group played",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: ungroup_track,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.move",
        label: "Move a track (a group with its tracks) to another place in the track order",
        params: &[TRACK, param("to", ParamType::Integer, "its new position, from 0, in project.tracks order")],
        kind: CommandKind::Edit,
        run: move_track,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.rename",
        label: "Rename a track",
        params: &[TRACK, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: rename_track,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "track.setColor",
        label: "Set a track's colour",
        params: &[TRACK, param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the default")],
        kind: CommandKind::Edit,
        run: set_color,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "track.colorGroup",
        label: "Give every track inside a group, at any depth, the group's colour, and with clips their clips too, as one undo step",
        params: &[
            TRACK,
            optional(
                "color",
                ParamType::Text,
                "\"#rrggbb\": the colour the group shows, for a group given none of its own (the default: the group's own)",
            ),
            optional(
                "clips",
                ParamType::Bool,
                "true: the clips of the group and of every track inside it show their track's colour again",
            ),
        ],
        kind: CommandKind::Edit,
        run: color_group,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "track.delete",
        label: "Delete a track with its clips; a deleted group's tracks stay",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: delete_track,
        policy: Policy::CRITICAL,
    },
    CommandSpec {
        id: "track.renameClips",
        label: "Name every clip of a track, in the arrangement and in its slots, after the track",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: crate::clip_color::rename_clips,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setOutput",
        label: "Route a track's audio to the main output or into a group track",
        params: &[TRACK, param("to", ParamType::Track, "\"main\", or a group track (id or name) to play into")],
        kind: CommandKind::Edit,
        run: set_output,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setSolo",
        label: "Solo the named tracks or end their solo: while any track is soloed, only soloed tracks, the groups they play into and return tracks are heard",
        params: &[
            TRACKS,
            param("solo", ParamType::Bool, "true solos the tracks"),
            optional("exclusive", ParamType::Bool, "true also ends every other solo (false by default)"),
        ],
        kind: CommandKind::Edit,
        run: set_solo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setCue",
        label: "Turn a track's cue on or off: a cued track is also heard on the cue output (headphones, output.setCuePair), after its own volume and pan, while the main mix stays as it is",
        params: &[TRACK, param("cue", ParamType::Bool, "true cues the track")],
        kind: CommandKind::Edit,
        run: set_cue,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setMidiInput",
        label: "Choose a track's MIDI input port, channel and monitoring: auto (while armed), in (always) or off (recording only); saved with the project",
        params: &[
            TRACK,
            optional(
                "port",
                ParamType::Text,
                "\"all\" (the default), \"none\", or a port's name; left out keeps the port",
            ),
            optional("channel", ParamType::Integer, "1 to 16, or 0 for every channel; left out keeps the channel"),
            optional("monitor", ParamType::Text, "auto, in or off; left out keeps the mode"),
        ],
        kind: CommandKind::Edit,
        run: set_midi_input,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setAudioInput",
        label: "Choose which channels of the audio input device a track hears (none, one channel, or a stereo pair) and when it is heard through the track: auto (while armed), in (always) or off; saved with the project",
        params: &[
            TRACK,
            optional(
                "input",
                ParamType::Text,
                "\"none\", a channel from 1 (\"3\"), or a pair (\"1/2\"); left out keeps it",
            ),
            optional("monitor", ParamType::Text, "auto (the default), in or off; left out keeps it"),
        ],
        kind: CommandKind::Edit,
        run: set_audio_input,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setArm",
        label: "Arm a track for recording or disarm it. Auto input monitoring follows arm; In and Off are independent. Arming is not an edit: it is not undone and not saved",
        params: &[
            TRACK,
            param("armed", ParamType::Bool, "true arms the track"),
            optional("exclusive", ParamType::Bool, "true also disarms every other track (false by default)"),
        ],
        kind: CommandKind::Transport,
        run: set_arm,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.stop",
        label: "Stop the clip a track is playing, at the next quantisation boundary",
        params: &[TRACK],
        kind: CommandKind::Transport,
        run: stop_track,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.devices",
        label: "List the devices on a track in order, with each built-in and .sd device's role and parameter values",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: devices::track_devices,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.add",
        label: "Add an audio or MIDI track with its mixer channel, clip slots and arrangement lane, optionally with a device (an instrument or MIDI effect makes a MIDI track, an effect an audio track)",
        params: &[
            optional("kind", ParamType::Text, "audio or midi (default: from the device)"),
            optional("name", ParamType::Text, "the track's name (default: the device's, or Audio n / MIDI n)"),
            optional(
                "after",
                ParamType::Track,
                "put it after this track, in its group (default: after the last regular track)",
            ),
            optional(
                "device",
                ParamType::Text,
                "the id of a built-in effect or instrument, from device.list, or a plug-in from plugin.list",
            ),
            optional(
                "preset",
                ParamType::Path(PathKind::File),
                "a preset file for the device (DX7 voice data for the FM synth)",
            ),
            optional(
                "sd",
                ParamType::Path(PathKind::File),
                "a data-only .sd AudioEffect graph to add to the new audio track",
            ),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1"),
        ],
        kind: CommandKind::Edit,
        run: files::add_track,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setSend",
        label: "Set how much of a track goes to a return track (its send); the return runs its devices over what it is sent",
        params: &[
            TRACK,
            RETURN,
            param("level", ParamType::Decibels, "send level in dB, at most +6; \"-inf\" or below -150 is silence"),
            CONTINUE,
        ],
        kind: CommandKind::Edit,
        run: crate::sends::set_send,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "track.setSendMode",
        label: "Take a track's send to a return before its volume and pan (pre) or after them (post, the default)",
        params: &[TRACK, RETURN, param("mode", ParamType::Text, "\"pre\" or \"post\"")],
        kind: CommandKind::Edit,
        run: crate::sends::set_send_mode,
        policy: Policy::DEFAULT,
    },
];

fn set_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > MAX_VOLUME_DB {
        return Err(format!("db must be at most +{MAX_VOLUME_DB}, got {db}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let gain = gain_of_db(db);
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channel_mut(*id) {
            channel.volume = gain;
            off |= crate::automation::override_lane(&mut channel.automation, |l| {
                l.param == suhina_model::automation::VOLUME
            });
        }
    }
    // A fader move, nothing else: a playing engine takes it as a message where it can (a lane
    // turned off changes more than the volume). Tracks not heard (muted, outside a solo) play
    // nothing it changes.
    if !off && channels.iter().all(|(track, _)| !suhina_engine::track_heard(&session.project, *track)) {
        session.sync.unheard = true;
    } else if !off {
        session.sync.mixer_edits = channels
            .iter()
            .map(|(track, _)| crate::ParamEdit {
                track: *track,
                device: suhina_engine::MIXER_VOLUME,
                param: 0,
                value: gain,
            })
            .collect();
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "volume_db": vec![to_db(gain); channels.len()] }), off))
}

fn adjust_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number(params, "db")?;
    let channels = channel_ids(&session.project, params)?;
    let mut volumes = Vec::with_capacity(channels.len());
    let mut off = false;
    for (_, id) in &channels {
        let Some(channel) = session.project.channel_mut(*id) else { continue };
        let now = to_db(channel.volume).unwrap_or(f64::NEG_INFINITY);
        let next = (now + db).clamp(MIN_VOLUME_DB, MAX_VOLUME_DB);
        channel.volume = gain_of_db(next);
        off |=
            crate::automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::VOLUME);
        volumes.push(to_db(channel.volume));
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "volume_db": volumes }), off))
}

fn set_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channel_mut(*id) {
            channel.pan = pan;
            off |=
                crate::automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::PAN);
        }
    }
    // A pan knob's turn, nothing else: a playing engine takes it as a message where it can.
    if !off && channels.iter().all(|(track, _)| !suhina_engine::track_heard(&session.project, *track)) {
        session.sync.unheard = true;
    } else if !off {
        session.sync.mixer_edits = channels
            .iter()
            .map(|(track, _)| crate::ParamEdit {
                track: *track,
                device: suhina_engine::MIXER_PAN,
                param: 0,
                value: pan,
            })
            .collect();
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "pan": vec![pan; channels.len()] }), off))
}

fn adjust_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let mut pans = Vec::with_capacity(channels.len());
    let mut off = false;
    for (_, id) in &channels {
        let Some(channel) = session.project.channel_mut(*id) else { continue };
        channel.pan = (channel.pan + pan).clamp(-1.0, 1.0);
        off |= crate::automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::PAN);
        pans.push(channel.pan);
    }
    if !off {
        session.sync.mixer_edits = channels
            .iter()
            .zip(&pans)
            .map(|((track, _), pan)| crate::ParamEdit {
                track: *track,
                device: suhina_engine::MIXER_PAN,
                param: 0,
                value: *pan,
            })
            .collect();
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "pan": pans }), off))
}

fn set_mute(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let mute = params.get("mute").and_then(Value::as_bool).ok_or("mute must be true or false")?;
    let channels = channel_ids(&session.project, params)?;
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channel_mut(*id) {
            channel.mute = mute;
            off |= crate::automation::override_lane(&mut channel.automation, |l| {
                l.param == suhina_model::automation::MUTE
            });
        }
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "mute": mute }), off))
}

/// The mixer channel of every track `params.tracks` names, in that order, checked.
fn channel_ids(p: &Project, params: &Value) -> Result<Vec<(TrackId, ChannelId)>, CommandError> {
    let mut out = Vec::new();
    for track in track_list(p, params)? {
        let found = p.track(track).ok_or("the track is missing")?;
        let channel = found.channel.ok_or_else(|| format!("track {:?} has no mixer channel", found.name))?;
        out.push((track, channel));
    }
    Ok(out)
}

/// The track ids of [`channel_ids`], for a command's result.
fn track_ids(channels: &[(TrackId, ChannelId)]) -> Vec<u32> {
    channels.iter().map(|(track, _)| track.0).collect()
}

fn ungroup_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let group = track_id(p, params)?;
    let channel = channel_id_of(p, group).and_then(|id| p.channel(id));
    let Some(channel) = channel.filter(|c| c.role == ChannelRole::Submix) else {
        return Err("only a group track can be ungrouped".into());
    };
    let (group_channel, output) = (channel.id, channel.output);
    let parent = p.track(group).and_then(|t| t.parent);
    for track in p.tracks.iter_mut().filter(|t| t.parent == Some(group)) {
        track.parent = parent;
    }
    for c in p.channels.iter_mut().filter(|c| c.output == Some(group_channel)) {
        c.output = output;
    }
    remove_track(p, group, group_channel);
    Ok(json!({ "ungrouped": group.0 }))
}

/// Moves a track, with everything inside it, into a group or to the top level: it becomes the
/// group's track at `index` among the group's own tracks, and its channel plays into the group.
fn set_group(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let role_of = |p: &Project, t: TrackId| channel_id_of(p, t).and_then(|id| p.channel(id)).map(|c| c.role);
    if matches!(role_of(p, track), Some(ChannelRole::Effect | ChannelRole::Master)) {
        return Err("a return track cannot go into a group".into());
    }
    let group = match params.get("group") {
        Some(Value::String(none)) if none.eq_ignore_ascii_case("none") => None,
        Some(_) => Some(track_at(p, params, "group")?),
        None => return Err("group must be a group track or \"none\"".into()),
    };
    let index = match params.get("index") {
        None => None,
        Some(v) => Some(v.as_u64().ok_or("index must be a whole number from 0")?),
    };
    let family = p.with_descendants(track);
    let output = match group {
        None => None,
        Some(g) => {
            let name = p.track(g).map(|t| t.name.clone()).unwrap_or_default();
            if role_of(p, g) != Some(ChannelRole::Submix) {
                return Err(format!("{name:?} is not a group track").into());
            }
            if family.contains(&g) {
                return Err(format!("{name:?} is inside the track being moved, or is that track").into());
            }
            let depth = p.group_depth(g).unwrap_or(0) + 1 + p.nesting_below(track);
            if depth > MAX_GROUP_DEPTH {
                return Err(format!(
                    "groups nest at most {MAX_GROUP_DEPTH} deep; moving into {name:?} would go deeper"
                )
                .into());
            }
            channel_id_of(p, g)
        }
    };

    let block: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    // The group's own tracks, top-level returns aside, and where the block goes among them.
    let is_return =
        |p: &Project, t: &Track| t.channel.and_then(|id| p.channel(id)).is_some_and(|c| c.role == ChannelRole::Effect);
    let siblings: Vec<TrackId> =
        p.tracks.iter().filter(|t| t.parent == group && !is_return(p, t)).map(|t| t.id).collect();
    let index = index.map_or(siblings.len(), |i| usize::try_from(i).unwrap_or(usize::MAX).min(siblings.len()));
    let position_of = |p: &Project, id: TrackId| p.tracks.iter().position(|t| t.id == id);
    let at = match (siblings.get(index), index.checked_sub(1).and_then(|i| siblings.get(i)), group) {
        (Some(&next), _, _) => position_of(p, next),
        // After the last sibling and everything inside it.
        (None, Some(&last), _) => p.with_descendants(last).last().and_then(|&t| position_of(p, t)).map(|i| i + 1),
        (None, None, Some(g)) => position_of(p, g).map(|i| i + 1),
        (None, None, None) => None,
    }
    .unwrap_or(p.tracks.len())
    .min(p.tracks.len());
    p.tracks.splice(at..at, block);
    if let Some(t) = p.track_mut(track) {
        t.parent = group;
    }
    if let Some(channel) = channel_id_of(p, track)
        && let Some(c) = p.channel_mut(channel)
    {
        c.output = output;
    }
    Ok(json!({ "track": track.0, "group": group.map(|g| g.0), "position": at }))
}

fn move_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let to = params.get("to").and_then(Value::as_u64).ok_or("to must be a whole number from 0")?;
    let family = p.with_descendants(track);
    let block: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    let at = usize::try_from(to).unwrap_or(usize::MAX).min(p.tracks.len());
    p.tracks.splice(at..at, block);
    Ok(json!({ "track": track.0, "position": at }))
}

fn rename_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let name = name.ok_or("name must be a text, not empty")?.to_string();
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.track_mut(track) {
        t.name.clone_from(&name);
    }
    if let Some(channel) = channel_id_of(&session.project, track)
        && let Some(c) = session.project.channel_mut(channel)
    {
        c.name.clone_from(&name);
    }
    Ok(json!({ "name": name }))
}

/// `track.colorGroup`: the tracks inside the group `track`, at any depth, take its colour.
fn color_group(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let group = track_id(&session.project, params)?;
    let p = &mut session.project;
    let own = p.track(group).map(|t| t.color).ok_or("the track is missing")?;
    let colour = match params.get("color") {
        None | Some(Value::Null) => own,
        Some(_) => crate::clip_color::parse_color(params, "color")?.or(own),
    };
    let parent_of =
        |id: TrackId, tracks: &[suhina_model::Track]| tracks.iter().find(|t| t.id == id).and_then(|t| t.parent);
    let inside: Vec<TrackId> = p
        .tracks
        .iter()
        .filter(|t| {
            let mut parent = t.parent;
            let mut hops = 0;
            while let Some(id) = parent {
                if id == group {
                    return true;
                }
                hops += 1;
                if hops > 64 {
                    return false;
                }
                parent = parent_of(id, &p.tracks);
            }
            false
        })
        .map(|t| t.id)
        .collect();
    if inside.is_empty() {
        return Err("the track holds no tracks".into());
    }
    for track in p.tracks.iter_mut().filter(|t| inside.contains(&t.id)) {
        track.color = colour;
    }
    // Their clips follow the tracks' colour again, the group's own too.
    let mut clips = 0usize;
    if params.get("clips").and_then(Value::as_bool) == Some(true) {
        let held = |track: Option<TrackId>| track.is_some_and(|t| t == group || inside.contains(&t));
        for lane in p.arrangement.lanes.iter_mut().filter(|l| held(l.track)) {
            for clip in &mut lane.clips {
                clip.color = None;
                clips += 1;
            }
        }
        for column in p.launcher.columns.iter_mut().filter(|c| held(Some(c.track))) {
            for clip in column.slots.iter_mut().filter_map(|s| s.clip.as_mut()) {
                clip.color = None;
                clips += 1;
            }
        }
    }
    Ok(json!({ "tracks": inside.iter().map(|t| t.0).collect::<Vec<_>>(), "clips": clips }))
}

fn set_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("color").and_then(Value::as_str).ok_or("color must be \"#rrggbb\" or \"none\"")?;
    let color = if text.eq_ignore_ascii_case("none") {
        None
    } else {
        let hex = text.strip_prefix('#').filter(|h| h.len() == 6).ok_or("color must be \"#rrggbb\" or \"none\"")?;
        let part = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (part(0), part(2), part(4)) {
            (Some(r), Some(g), Some(b)) => Some(suhina_model::Rgb(r, g, b)),
            _ => return Err("color must be \"#rrggbb\" or \"none\"".into()),
        }
    };
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.track_mut(track) {
        t.color = color;
    }
    Ok(json!({ "color": text }))
}

fn delete_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let p = &session.project;
    let is_group = channel_id_of(p, track).and_then(|id| p.channel(id)).is_some_and(|c| c.role == ChannelRole::Submix);
    if is_group {
        ungroup_track(session, params)?;
        return Ok(json!({ "deleted": track.0 }));
    }
    let channel = channel_id_of(&session.project, track);
    let p = &mut session.project;
    match channel {
        Some(channel) => remove_track(p, track, channel),
        None => {
            p.tracks.retain(|t| t.id != track);
            p.arrangement.lanes.retain(|l| l.track != Some(track));
            p.launcher.columns.retain(|c| c.track != track);
        }
    }
    Ok(json!({ "deleted": track.0 }))
}

fn set_output(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from = channel_param(session, params)?.id;
    let p = &session.project;
    let to = match params.get("to") {
        Some(Value::String(main)) if main.eq_ignore_ascii_case("main") => None,
        Some(_) => {
            let track = track_at(p, params, "to")?;
            let target = p
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|id| p.channel(id))
                .ok_or("the target track has no mixer channel")?;
            if target.role != ChannelRole::Submix {
                return Err(format!("{:?} is not a group track; route to \"main\" or a group", target.name).into());
            }
            // The group must not play, directly or through other groups, into the track itself.
            let mut next = Some(target.id);
            for _ in 0..=p.channels.len() {
                match next {
                    Some(id) if id == from => {
                        return Err(format!("routing into {:?} would make a loop", target.name).into());
                    }
                    Some(id) => next = p.channel(id).and_then(|c| c.output),
                    None => break,
                }
            }
            Some(target.id)
        }
        None => return Err("to must be \"main\" or a group track".into()),
    };
    let channel = channel_param(session, params)?;
    channel.output = to;
    let name = to
        .and_then(|id| session.project.tracks.iter().find(|t| t.channel == Some(id)))
        .map_or_else(|| "main".to_string(), |t| t.name.clone());
    Ok(json!({ "to": name }))
}

fn set_solo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let solo = params.get("solo").and_then(Value::as_bool).ok_or("solo must be true or false")?;
    let exclusive = match params.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
    };
    let channels = channel_ids(&session.project, params)?;
    let ids: Vec<ChannelId> = channels.iter().map(|(_, id)| *id).collect();
    for channel in &mut session.project.channels {
        if ids.contains(&channel.id) {
            channel.solo = solo;
        } else if exclusive {
            channel.solo = false;
        }
    }
    Ok(json!({ "tracks": track_ids(&channels), "solo": solo }))
}

fn set_cue(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let cue = params.get("cue").and_then(Value::as_bool).ok_or("cue must be true or false")?;
    channel_param(session, params)?.cue = cue;
    Ok(json!({ "cue": cue }))
}

fn set_midi_input(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = track_id(&session.project, params)?;
    let port = match params.get("port") {
        None => None,
        Some(Value::String(s)) => Some(match s.trim() {
            "" => return Err("port must be \"all\", \"none\" or a port's name".into()),
            "all" => MidiPort::All,
            "none" => MidiPort::Nothing,
            name => MidiPort::Named(name.to_string()),
        }),
        Some(_) => return Err("port must be a text".into()),
    };
    let channel = match params.get("channel") {
        None => None,
        Some(v) => match v.as_u64() {
            Some(0) => Some(None),
            Some(c @ 1..=16) => Some(u8::try_from(c).ok()),
            _ => return Err("channel must be 0 (every channel) or 1 to 16".into()),
        },
    };
    let monitor = match params.get("monitor") {
        None => None,
        Some(v) => Some(match v.as_str() {
            Some("auto") => suhina_model::Monitor::Auto,
            Some("in") => suhina_model::Monitor::In,
            Some("off") => suhina_model::Monitor::Off,
            _ => return Err("monitor must be auto, in or off".into()),
        }),
    };
    let track = session.project.track_mut(id).ok_or("the track is missing")?;
    if let Some(port) = port {
        track.midi_input.port = port;
    }
    if let Some(channel) = channel {
        track.midi_input.channel = channel;
    }
    if let Some(monitor) = monitor {
        track.midi_input.monitor = monitor;
    }
    Ok(json!({ "midi_input": midi_input_json(&track.midi_input) }))
}

fn set_audio_input(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    use suhina_model::{InputChannels, MAX_INPUT_CHANNEL, Monitor};
    let id = track_id(&session.project, params)?;
    let bad = || {
        CommandError(format!("input must be \"none\", a channel from 1 to {MAX_INPUT_CHANNEL} or a pair like \"1/2\""))
    };
    let channel = |text: &str| text.trim().parse::<u16>().ok().filter(|c| (1..=MAX_INPUT_CHANNEL).contains(c));
    let channels = match params.get("input") {
        None => None,
        Some(Value::String(text)) => Some(match text.trim() {
            "none" => InputChannels::None,
            pair if pair.contains('/') => {
                let (left, right) = pair.split_once('/').ok_or_else(bad)?;
                let (left, right) = (channel(left).ok_or_else(bad)?, channel(right).ok_or_else(bad)?);
                if left.checked_add(1) != Some(right) {
                    return Err("a stereo pair is two neighbouring channels, like \"1/2\" or \"3/4\"".into());
                }
                InputChannels::Stereo(left)
            }
            one => InputChannels::Mono(channel(one).ok_or_else(bad)?),
        }),
        Some(_) => return Err(bad()),
    };
    let monitor = match params.get("monitor") {
        None => None,
        Some(v) => Some(match v.as_str() {
            Some("auto") => Monitor::Auto,
            Some("in") => Monitor::In,
            Some("off") => Monitor::Off,
            _ => return Err("monitor must be auto, in or off".into()),
        }),
    };
    let track = session.project.track_mut(id).ok_or("the track is missing")?;
    if let Some(channels) = channels {
        track.audio_input.channels = channels;
    }
    if let Some(monitor) = monitor {
        track.audio_input.monitor = monitor;
    }
    Ok(json!({ "audio_input": audio_input_json(&track.audio_input) }))
}

fn set_arm(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let armed = params.get("armed").and_then(Value::as_bool).ok_or("armed must be true or false")?;
    let exclusive = match params.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
    };
    let track = track_id(&session.project, params)?;
    session.set_armed(track, armed, exclusive);
    let tracks: Vec<u32> = session.armed().iter().map(|t| t.0).collect();
    Ok(json!({ "armed": armed, "armed_tracks": tracks, "playing": session.player().is_some() }))
}

fn stop_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    send(session, LaunchCommand::StopColumn { column })?;
    Ok(json!({ "queued": "stop" }))
}
