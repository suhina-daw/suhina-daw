// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time playback on the default audio output: the arrangement once, or the clip launcher
//! driven by typed commands. The device itself is `suhina-output`.

use std::io::BufRead;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use suhina_engine::model::{LaneData, Project};
use suhina_engine::{AudioPool, DeviceAssets, Plugins, Renderer};
use suhina_output::{Output, Progress, Stream};
use suhina_session::{OutputChoice, Player, Session};

/// Extra time to wait after the last frame, so the device can drain its buffer.
const TAIL: Duration = Duration::from_millis(300);

/// Opens the output `choice` names, saying what could not be had as chosen.
fn chosen_output(choice: &OutputChoice) -> Result<Output, String> {
    let output = Output::choose(choice)?;
    for notice in output.notices() {
        eprintln!("note: {notice}");
    }
    Ok(output)
}

/// Says when the render thread could not have real-time priority (once per process).
fn note_priority(stream: &Stream) {
    if let Some(notice) = stream.priority_notice() {
        eprintln!("note: {notice}");
    }
}

/// Plays the project's arrangement once on the output `choice` names and returns when it ends.
pub fn play(project: &Project, audio: &AudioPool, assets: &DeviceAssets, choice: &OutputChoice) -> Result<(), String> {
    let output = chosen_output(choice)?;
    let mut renderer =
        Renderer::with_assets(project, audio, assets, output.sample_rate()).map_err(|e| e.to_string())?;
    let length = renderer.length();
    if length == 0 {
        println!("{}", nothing_to_play(project));
        return Ok(());
    }
    let progress = Arc::new(Progress::default());
    let mut controller = renderer.controller();
    let stream = output.start(renderer, &mut controller, &progress)?;
    note_priority(&stream);

    let seconds = length as f64 / f64::from(output.sample_rate());
    println!("playing {seconds:.2} s on {}; Ctrl+C stops", output.describe());

    // Wait for the callback to reach the end, with a wall-clock limit in case the device stalls.
    let deadline = Instant::now() + Duration::from_secs_f64(seconds) + Duration::from_secs(5);
    while progress.frames() < length {
        if progress.failed() {
            return Err(format!("audio output {} reported an error during playback", output.describe()));
        }
        if Instant::now() > deadline {
            return Err(format!("audio output {} stopped advancing", output.describe()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(TAIL);
    let underruns = controller.stretch_underruns();
    drop(stream);
    drop(controller);
    if underruns > 0 {
        eprintln!("warning: {underruns} frames of stretched audio were not ready in time and played as silence");
    }
    println!("done");
    Ok(())
}

const LAUNCHER_HELP: &str = "\
commands (scenes count from 1; a track is its name or id):
  scene <n>             launch scene n
  clip <track> <n>      launch the clip in scene n of a track (a press of its launch button)
  release <track> <n>   release its launch button: stops a gate clip, ends a repeat clip's repeats
  stop <track>          stop a track's clip (a track named here: by its id)
  stopall               stop every clip
  play / stop           start the transport from the play start marker / stop everything now
                        and go back to the marker (stop again: the marker to the beginning)
  stop here             stop, and move the marker to where it stopped
  play <track> <i>      play from the start of a track's arrangement clip i (from project.clips,
                        0 first): the marker moves there
  pos <beats>           move the play start marker, e.g. pos 16 (bar 5 in 4/4); jumps while playing
  metronome on|off [db] click on every beat, e.g. metronome on -12
  quant <value>         launch quantisation: none, 8 bars ... 1 bar, 1/2 ... 1/32, triplets as 1/8T
  back                  tracks that launched clips play their arrangement again
  state                 what is playing and queued
  grid                  the scenes and each track's clips
  levels                the level meters: peak and RMS of the main output and each track, in dB
edits (heard while playing):
  tempo <bpm>           set the project tempo
  vol <track> <db>      set a track's volume, e.g. vol Bass -6
  mute / unmute <track> mute or unmute a track
  undo / redo           undo or redo the last edit
  <command.id> [json]   any command, e.g. track.setMute {\"tracks\": [\"Bass\"], \"mute\": true}
  help, quit";

/// Plays the project on the output `choice` names with the transport stopped and reads launcher
/// commands from standard input until `quit` or end of input.
pub fn launcher(
    project: Project,
    audio: &AudioPool,
    assets: &DeviceAssets,
    choice: &OutputChoice,
) -> Result<(), String> {
    if project.launcher.columns.iter().all(|c| c.slots.iter().all(|s| s.clip.is_none())) {
        println!("note: the project has no clips in its clip launcher; only `play` (the arrangement) will sound");
    }
    let output = chosen_output(choice)?;
    let mut renderer = Renderer::stopped_with_assets(&project, audio, assets, output.sample_rate(), Plugins::new())
        .map_err(|e| e.to_string())?;
    let mut controller = renderer.controller();
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &mut controller, &progress)?;
    note_priority(&stream);
    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    println!("clip launcher on {}\n{LAUNCHER_HELP}\n", output.describe());
    print_grid(&mut session);

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("reading commands: {e}"))?;
        if progress.failed() {
            return Err(format!("audio output {} reported an error during playback", output.describe()));
        }
        match parse_line(&line) {
            Ok(Line::Empty) => {}
            Ok(Line::Quit) => break,
            Ok(Line::Help) => println!("{LAUNCHER_HELP}"),
            Ok(Line::Command(id, params)) => match session.execute(&id, &params) {
                Ok(result) if id == "launcher.state" => print_state(&result),
                Ok(result) if id == "project.launcher" => print_grid_json(&result),
                Ok(result) => println!("{id}: {result}"),
                Err(e) => println!("error: {e}"),
            },
            Err(e) => println!("error: {e} (help lists the commands)"),
        }
    }
    // Stop, then let the release tails and fades finish before closing the device.
    if let Err(e) = session.execute("transport.stop", &json!({})) {
        eprintln!("warning: {e}");
    }
    std::thread::sleep(TAIL);
    drop(stream);
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Line {
    Empty,
    Quit,
    Help,
    Command(String, Value),
}

/// A track argument: a number is a track id, anything else a name.
fn track_value(words: &[&str]) -> Value {
    let text = words.join(" ");
    text.parse::<u64>().map_or_else(|_| json!(text), |id| json!(id))
}

fn scene_value(word: &str) -> Result<Value, String> {
    word.parse::<u64>().map(|n| json!(n)).map_err(|_| format!("{word:?} is not a scene number"))
}

/// Turns one line of input into a command id and parameters.
fn parse_line(line: &str) -> Result<Line, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let command = |id: &str, params: Value| Ok(Line::Command(id.to_string(), params));
    match words.as_slice() {
        [] => Ok(Line::Empty),
        ["quit" | "exit"] => Ok(Line::Quit),
        ["help" | "?"] => Ok(Line::Help),
        ["play"] => command("transport.play", json!({})),
        ["play", track @ .., index] if !track.is_empty() => {
            let index: u64 = index.parse().map_err(|_| format!("{index:?} is not a clip index"))?;
            command("transport.play", json!({ "from": "clip", "track": track_value(track), "index": index }))
        }
        ["stop"] => command("transport.stop", json!({})),
        ["stop", "here"] => command("transport.stop", json!({ "return": "here" })),
        ["pos" | "position", beats] => {
            let beats: f64 = beats.parse().map_err(|_| format!("{beats:?} is not a number of beats"))?;
            command("transport.setPosition", json!({ "beats": beats }))
        }
        ["stopall"] => command("launcher.stopAll", json!({})),
        ["back"] => command("launcher.backToArrangement", json!({})),
        ["state" | "s"] => command("launcher.state", json!({})),
        ["grid"] => command("project.launcher", json!({})),
        ["levels"] => command("mixer.levels", json!({})),
        ["scene", n] => command("scene.launch", json!({ "scene": scene_value(n)? })),
        ["clip", track @ .., n] if !track.is_empty() => {
            command("clip.launch", json!({ "track": track_value(track), "scene": scene_value(n)? }))
        }
        ["release", track @ .., n] if !track.is_empty() => {
            command("clip.release", json!({ "track": track_value(track), "scene": scene_value(n)? }))
        }
        ["stop", track @ ..] => command("track.stop", json!({ "track": track_value(track) })),
        ["quant", value @ ..] if !value.is_empty() => {
            command("launcher.setQuantization", json!({ "value": value.join(" ") }))
        }
        ["tempo", bpm] => {
            let bpm: f64 = bpm.parse().map_err(|_| format!("{bpm:?} is not a tempo"))?;
            command("transport.setTempo", json!({ "bpm": bpm }))
        }
        ["vol", track @ .., db] if !track.is_empty() => {
            let db: f64 = db.parse().map_err(|_| format!("{db:?} is not a level in dB"))?;
            command("track.setVolume", json!({ "tracks": [track_value(track)], "db": db }))
        }
        ["mute", track @ ..] if !track.is_empty() => {
            command("track.setMute", json!({ "tracks": [track_value(track)], "mute": true }))
        }
        ["unmute", track @ ..] if !track.is_empty() => {
            command("track.setMute", json!({ "tracks": [track_value(track)], "mute": false }))
        }
        ["metronome" | "click", state, db @ ..] if matches!(*state, "on" | "off") && db.len() <= 1 => {
            let mut params = json!({ "on": *state == "on" });
            if let Some(db) = db.first() {
                let db: f64 = db.parse().map_err(|_| format!("{db:?} is not a level in dB"))?;
                params["db"] = json!(db);
            }
            command("transport.setMetronome", params)
        }
        ["undo"] => command("edit.undo", json!({})),
        ["redo"] => command("edit.redo", json!({})),
        [id, ..] if id.contains('.') => {
            let rest = line.trim_start().get(id.len()..).unwrap_or_default().trim();
            let params = if rest.is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("parameters are not JSON: {e}"))?
            };
            command(id, params)
        }
        _ => Err(format!("unknown command {line:?}")),
    }
}

fn print_grid(session: &mut Session) {
    if let Ok(grid) = session.execute("project.launcher", &json!({})) {
        print_grid_json(&grid);
    }
}

fn print_grid_json(grid: &Value) {
    let empty = Vec::new();
    let scenes = grid["scenes"].as_array().unwrap_or(&empty);
    println!("{} scenes", scenes.len());
    for scene in scenes {
        let name = scene["name"].as_str().unwrap_or_default();
        let tempo = scene["tempo_bpm"].as_f64().map(|t| format!(" ({t} bpm)")).unwrap_or_default();
        if !name.is_empty() || !tempo.is_empty() {
            println!("  scene {}: {name}{tempo}", scene["scene"]);
        }
    }
    println!("clips by track (scene=clip):");
    for track in grid["tracks"].as_array().unwrap_or(&empty) {
        let clips: Vec<String> = track["slots"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|s| {
                let name = s["clip"].as_str()?;
                let name = if name.is_empty() { "unnamed" } else { name };
                let off = if s["enabled"] == false { " (deactivated)" } else { "" };
                Some(format!("{}={name}{off}", s["scene"]))
            })
            .collect();
        let clips = if clips.is_empty() { "-".to_string() } else { clips.join(", ") };
        println!("  {}: {clips}", track["track_name"].as_str().unwrap_or_default());
    }
}

fn print_state(state: &Value) {
    let transport = if state["running"] == true { "playing" } else { "stopped" };
    println!("{transport} at {} ({} bpm)", state["position"].as_str().unwrap_or("?"), state["tempo_bpm"]);
    let empty = Vec::new();
    for track in state["tracks"].as_array().unwrap_or(&empty) {
        let describe = |v: &Value| match v {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            v => Some(format!("{} (scene {})", v["clip"].as_str().unwrap_or("?"), v["scene"])),
        };
        let playing = describe(&track["playing"]).unwrap_or_else(|| "-".into());
        let queued = describe(&track["queued"]).map(|q| format!(", next: {q}")).unwrap_or_default();
        println!("  {}: {playing}{queued}", track["track_name"].as_str().unwrap_or_default());
    }
}

/// Explains why a project produced no sound: no clips at all, clips without notes, or audio
/// clips whose files are missing.
fn nothing_to_play(project: &Project) -> String {
    let clips: Vec<_> = project.arrangement.lanes.iter().flat_map(|l| &l.clips).collect();
    let audio = clips
        .iter()
        .filter(|c| {
            project
                .content(c.content)
                .is_some_and(|content| content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))))
        })
        .count();
    match (clips.len(), audio) {
        (0, _) => {
            "nothing to play: the arrangement has no clips (try --scene <n>, or --launcher to launch clips)".to_string()
        }
        (_, 0) => "nothing to play: the arrangement's clips have no notes".to_string(),
        (total, audio) => format!(
            "nothing to play: {audio} of {total} arrangement clips are audio clips whose files could not be \
             loaded (see the warnings above)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_lines_become_commands() {
        let cmd = |id: &str, params: Value| Ok(Line::Command(id.into(), params));
        assert_eq!(parse_line("  "), Ok(Line::Empty));
        assert_eq!(parse_line("quit"), Ok(Line::Quit));
        assert_eq!(parse_line("scene 2"), cmd("scene.launch", json!({ "scene": 2 })));
        assert_eq!(parse_line("clip Lead Vox 3"), cmd("clip.launch", json!({ "track": "Lead Vox", "scene": 3 })));
        assert_eq!(parse_line("clip 4 1"), cmd("clip.launch", json!({ "track": 4, "scene": 1 })));
        assert_eq!(parse_line("release Lead Vox 3"), cmd("clip.release", json!({ "track": "Lead Vox", "scene": 3 })));
        assert_eq!(parse_line("stop"), cmd("transport.stop", json!({})));
        assert_eq!(parse_line("stop Drums"), cmd("track.stop", json!({ "track": "Drums" })));
        assert_eq!(parse_line("stop here"), cmd("transport.stop", json!({ "return": "here" })));
        assert_eq!(
            parse_line("play Lead Vox 2"),
            cmd("transport.play", json!({ "from": "clip", "track": "Lead Vox", "index": 2 }))
        );
        assert!(parse_line("play Drums first").is_err());
        assert_eq!(parse_line("pos 16"), cmd("transport.setPosition", json!({ "beats": 16.0 })));
        assert_eq!(parse_line("levels"), cmd("mixer.levels", json!({})));
        assert_eq!(parse_line("position 2.5"), cmd("transport.setPosition", json!({ "beats": 2.5 })));
        assert!(parse_line("pos bar5").is_err());
        assert_eq!(parse_line("quant 2 bars"), cmd("launcher.setQuantization", json!({ "value": "2 bars" })));
        assert_eq!(parse_line("tempo 100"), cmd("transport.setTempo", json!({ "bpm": 100.0 })));
        assert_eq!(
            parse_line("vol Lead Vox -6"),
            cmd("track.setVolume", json!({ "tracks": ["Lead Vox"], "db": -6.0 }))
        );
        assert_eq!(parse_line("mute 2"), cmd("track.setMute", json!({ "tracks": [2], "mute": true })));
        assert_eq!(parse_line("unmute Bass"), cmd("track.setMute", json!({ "tracks": ["Bass"], "mute": false })));
        assert_eq!(parse_line("undo"), cmd("edit.undo", json!({})));
        assert_eq!(parse_line("metronome on"), cmd("transport.setMetronome", json!({ "on": true })));
        assert_eq!(parse_line("click off"), cmd("transport.setMetronome", json!({ "on": false })));
        assert_eq!(parse_line("metronome on -12"), cmd("transport.setMetronome", json!({ "on": true, "db": -12.0 })));
        assert!(parse_line("metronome loud").is_err());
        assert!(parse_line("tempo fast").is_err());
        assert_eq!(
            parse_line("track.setMute {\"tracks\": [1], \"mute\": true}"),
            cmd("track.setMute", json!({ "tracks": [1], "mute": true }))
        );
        assert_eq!(parse_line("project.info"), cmd("project.info", json!({})));
        assert!(parse_line("scene two").is_err());
        assert!(parse_line("clip 3").is_err());
        assert!(parse_line("track.setMute {nope").is_err());
        assert!(parse_line("dance").is_err());
    }
}
