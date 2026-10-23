// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `launcher.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "launcher.state",
        label: "What the audio engine is playing: transport, tempo, position, each track's playing and queued clip, and the engine's CPU load",
        params: &[],
        kind: CommandKind::Transport,
        run: launcher_state,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.stopAll",
        label: "Stop every launched clip at the next quantisation boundary",
        params: &[],
        kind: CommandKind::Transport,
        run: stop_all,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.setQuantization",
        label: "Set the grid launches wait for and the Arrangement view snaps to (default 1 bar)",
        params: &[param(
            "value",
            ParamType::Text,
            "none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32",
        )],
        kind: CommandKind::Transport,
        run: set_quantization,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.backToArrangement",
        label: "Return every track that launched a clip to playing its arrangement clips",
        params: &[],
        kind: CommandKind::Transport,
        run: back_to_arrangement,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.addMapping",
        label: "Map a computer key or a MIDI note to a slot, a scene or a track's stop button: its press launches it (and its release releases a gate or repeat clip); a source already mapped moves to the new target",
        params: &[
            param(
                "source",
                ParamType::Text,
                "key:<name> (key:Q, key:1, key:Space) or note:<channel 1-16>:<note 0-127>",
            ),
            param("target", ParamType::Text, "slot, scene or stop"),
            optional("track", ParamType::Track, "track id or name, for slot and stop"),
            optional("scene", ParamType::Integer, "scene number, from 1, for slot and scene"),
        ],
        kind: CommandKind::Edit,
        run: mapping::add_mapping,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.removeMapping",
        label: "Remove a key or note mapping: the one of a source, or every mapping of a target",
        params: &[
            optional("source", ParamType::Text, "key:<name> or note:<channel>:<note>"),
            optional("target", ParamType::Text, "slot, scene or stop, instead of source"),
            optional("track", ParamType::Track, "track id or name, for slot and stop"),
            optional("scene", ParamType::Integer, "scene number, from 1, for slot and scene"),
        ],
        kind: CommandKind::Edit,
        run: mapping::remove_mapping,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "launcher.mappings",
        label: "List the keys and MIDI notes mapped to slots, scenes and stop buttons (source, target, track, scene, whether the target is gone)",
        params: &[],
        kind: CommandKind::Read,
        run: mapping::list_mappings,
        policy: Policy::DEFAULT,
    },
];

fn launcher_state(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let preview_state = session.player().map(|player| {
        let preview = player.preview_monitor();
        json!({ "playing": preview.playing(), "seconds": (preview.seconds() * 1000.0).round() / 1000.0 })
    });
    let cpu = session.player().map(|player| player.cpu_monitor().load_percent());
    let monitor = session.player_mut()?.monitor();
    let (running, beat, tempo) = (monitor.running(), monitor.beat(), monitor.tempo_bpm());
    let start_beat = monitor.start_beat();
    let monitor_metronome = monitor.metronome();
    let meter = monitor.meter();
    let statuses: Vec<Option<ColumnStatus>> = (0..monitor.columns()).map(|i| monitor.column(i)).collect();
    let p = session.project();
    let project_meter = p.transport.meter();
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .zip(statuses)
        .map(|(column, status)| {
            let status = status.unwrap_or(ColumnStatus { playing: None, queued: None, overrides_arrangement: false });
            let clip_name = |slot: usize| {
                column
                    .slots
                    .get(slot)
                    .and_then(|s| s.clip.as_ref())
                    .and_then(|c| p.content(c.content))
                    .map(|c| c.name.clone())
            };
            json!({
                "track": column.track.0,
                "track_name": track_name(p, column.track),
                "playing": status.playing.map(|slot| json!({ "scene": slot + 1, "clip": clip_name(slot) })),
                "queued": match status.queued {
                    None => Value::Null,
                    Some(Queued::Stop) => json!("stop"),
                    Some(Queued::Launch(slot)) => json!({ "scene": slot + 1, "clip": clip_name(slot) }),
                },
                "plays_arrangement": !status.overrides_arrangement,
            })
        })
        .collect();
    // Rounded once, so the beat and the position agree.
    let beat = if beat.is_finite() { (beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 };
    // Bars through the time-signature changes while the clock follows the tempo map, or
    // through the time signature a scene set (`docs/spec/tempo-map.md`).
    let (bar, into_bar) = meter.locate(beat, &project_meter);
    let position = format!("{bar}.{}", into_bar.floor() as u64 + 1);
    let signature = if meter.follows_map { project_meter.position(beat).time_signature } else { meter.time_signature };
    Ok(json!({
        "running": running,
        "metronome": monitor_metronome,
        "tempo_bpm": tempo,
        "beat": beat,
        "time_signature": signature_text(signature),
        "position": position,
        "follows_tempo_map": meter.follows_map,
        "start_beat": if start_beat.is_finite() { (start_beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 },
        "tracks": tracks,
        "preview": preview_state,
        "cpu": cpu,
    }))
}

fn stop_all(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::StopAll)?;
    Ok(json!({ "queued": "stop" }))
}

fn back_to_arrangement(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::BackToArrangement)?;
    Ok(json!({ "arrangement": true }))
}
