// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `record.*`, `transport.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "transport.setTempo",
        label: "Set the project tempo",
        params: &[param("bpm", ParamType::Number, "beats per minute, 20 to 999")],
        kind: CommandKind::Edit,
        run: set_tempo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.setLoop",
        label: "Set the arrangement's loop region, the span playback repeats while looping is on",
        params: &[
            param("start", ParamType::Number, "where the loop starts, in beats from the beginning, 0 … 120000"),
            param("end", ParamType::Number, "where it ends, in beats, after start and at most 120000"),
            optional("enabled", ParamType::Bool, "true turns looping on, false off (default: as it is)"),
        ],
        kind: CommandKind::Edit,
        run: set_loop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.setLoopEnabled",
        label: "Turn looping on or off; on without a loop region first makes one over the first 4 bars",
        params: &[optional("enabled", ParamType::Bool, "true turns looping on, false off (default: the other way)")],
        kind: CommandKind::Edit,
        run: set_loop_enabled,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.play",
        label: "Start the transport from the play start marker (beat 0 unless moved), or from an arrangement clip's start: the arrangement plays, launches follow the beat",
        params: &[
            optional(
                "from",
                ParamType::Text,
                "marker (default): from the play start marker; clip: the marker moves to the start of the clip track and index name, and playback starts there",
            ),
            optional(
                "clip",
                ParamType::Clip,
                "with from: clip, the clip's id from project.clips, or a name only one clip has",
            ),
            optional("track", ParamType::Track, "with from: clip and index, the clip's track (id or name)"),
            optional(
                "index",
                ParamType::Integer,
                "with from: clip and track, the clip's index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
        ],
        kind: CommandKind::Transport,
        run: play,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.stop",
        label: "Stop the transport, the arrangement and every launched clip at once; stopping while stopped moves the play start marker to beat 0",
        params: &[optional(
            "return",
            ParamType::Text,
            "marker (default): the playhead goes back to the play start marker; here: the marker moves to where the playhead stopped",
        )],
        kind: CommandKind::Transport,
        run: stop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.setPosition",
        label: "Move the play start marker, where the transport starts; while playing, playback jumps there at once",
        params: &[param("beats", ParamType::Number, "beats from the beginning, 0 … 120000")],
        kind: CommandKind::Transport,
        run: set_position,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.followTempoMap",
        label: "Play the arrangement's tempo and time signature again after a launched scene's tempo or time signature took over; starting the transport does the same",
        params: &[],
        kind: CommandKind::Transport,
        run: follow_tempo_map,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.setMetronome",
        label: "Turn the metronome on or off: a click on every beat while playing, higher on the bar",
        params: &[
            param("on", ParamType::Bool, "true turns the click on"),
            optional(
                "db",
                ParamType::Decibels,
                "click level in dB, at most +6; keeps the current level (-6 at first) if left out",
            ),
            optional(
                "sound",
                ParamType::Text,
                "the click's sound: beep (the first), wood or soft; keeps it if left out",
            ),
        ],
        kind: CommandKind::Transport,
        run: set_metronome,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.record",
        label: "Record every armed track into the arrangement from the playhead (from the play start marker after the count-in when stopped), or end every take; each take is one undo step",
        params: &[param("on", ParamType::Bool, "true starts recording, false ends every take")],
        kind: CommandKind::Transport,
        run: crate::recording::record_arrangement,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "transport.setCountIn",
        label: "Set the count-in recording waits for when it starts the transport: 0 (none), 1, 2 or 4 bars of clicks",
        params: &[param("bars", ParamType::Integer, "0, 1, 2 or 4")],
        kind: CommandKind::Transport,
        run: crate::recording::set_count_in,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "record.state",
        label: "What is being recorded: whether the arrangement records, each take (track, slot or arrangement, start and end beat, notes, frames, file), the count-in setting and the beats it has left",
        params: &[],
        kind: CommandKind::Transport,
        run: crate::recording::record_state,
        policy: Policy::DEFAULT,
    },
];

fn set_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bpm = number(params, "bpm")?;
    if !(20.0..=999.0).contains(&bpm) {
        return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
    }
    session.project.transport.tempo_bpm = bpm;
    Ok(json!({ "tempo_bpm": bpm }))
}

fn set_loop_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let transport = &mut session.project.transport;
    let enabled = optional_enabled(params)?.unwrap_or(!transport.loop_enabled);
    if enabled && transport.loop_range.is_none() {
        let signature = transport.time_signature;
        let bar = f64::from(signature.numerator) * 4.0 / f64::from(signature.denominator.max(1));
        let end = if bar.is_finite() && bar > 0.0 { (4.0 * bar).min(MAX_START_BEAT) } else { 16.0 };
        transport.loop_range = Some(LoopRange { start: 0.0, end });
    }
    transport.loop_enabled = enabled;
    Ok(loop_value(&session.project))
}

fn set_metronome(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    // Without a level the metronome keeps the one it has (-6 dB to begin with).
    let gain = match params.get("db") {
        None => None,
        Some(_) => {
            let db = number_or_neg_infinity(params, "db")?;
            if db > 6.0 {
                return Err(format!("db must be at most +6, got {db}").into());
            }
            Some(if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) })
        }
    };
    let sound = match params.get("sound") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(
            suhina_engine::ClickSound::from_name(name)
                .ok_or_else(|| CommandError(format!("sound must be beep, wood or soft, got {name}")))?,
        ),
        Some(other) => return Err(format!("sound must be beep, wood or soft, got {other}").into()),
    };
    send(session, LaunchCommand::SetMetronome { on, gain: gain.map(|g| g as f32) })?;
    if let Some(sound) = sound {
        send(session, LaunchCommand::SetMetronomeSound(sound))?;
    }
    session.live.metronome.0 = on;
    if let Some(gain) = gain {
        session.live.metronome.1 = to_db(gain).unwrap_or(f64::NEG_INFINITY);
    }
    if let Some(sound) = sound {
        session.live.metronome.2 = sound;
    }
    let (_, db, sound) = session.live.metronome;
    Ok(json!({ "metronome": on, "db": if db.is_finite() { json!(db) } else { json!("-inf") }, "sound": sound.name() }))
}

fn set_position(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = number(params, "beats")?;
    if !(0.0..=MAX_START_BEAT).contains(&beats) {
        return Err(format!("beats must be between 0 and {MAX_START_BEAT}, got {beats}").into());
    }
    send(session, LaunchCommand::SetPosition { beat: beats })?;
    Ok(json!({ "start_beat": beats }))
}

fn follow_tempo_map(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::FollowTempoMap)?;
    Ok(json!({ "follows_tempo_map": true }))
}
