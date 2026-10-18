// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Offline rendering to WAV files, shared by the CLI and the MCP server.

use std::path::Path;

use serde_json::{Value, json};
use suhina_engine::model::{Arrangement, ArrangementLane, Clip, ClipId, Project};
use suhina_engine::{AudioPool, Plugins, Renderer, render_offline_with_assets, render_offline_with_cue};
use suhina_session::{CommandKind, OfflinePlugins, Player, PluginEnv, Session, commands, start_offline};

/// Longest launch-script render, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = 600.0;
/// Most steps in one launch script.
pub const MAX_SCRIPT_STEPS: usize = 1024;

/// What a render produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSummary {
    pub seconds: f64,
    pub sample_rate: u32,
    /// Largest absolute sample value; above 1.0 means the file clips.
    pub peak: f32,
    /// About the project's plug-ins: missing ones (passed through), ones that could not start
    /// or did not take their saved settings.
    pub plugins: Vec<String>,
}

/// The project's plug-ins started for a render with `plugins` (ADR 0006), or none.
fn offline_plugins(
    project: &Project,
    audio: &AudioPool,
    plugins: Option<&PluginEnv>,
    sample_rate: u32,
) -> OfflinePlugins {
    match plugins {
        Some(env) => start_offline(project, audio, env, sample_rate),
        None => OfflinePlugins::none(),
    }
}

/// Renders the project's arrangement, with the audio files in `audio` and its plug-ins found in
/// `plugins` (none are run without it), to a 32-bit float stereo WAV file at `path`. Plug-ins
/// run through the same engine as playback, each block waited for.
pub fn render_wav(
    project: &Project,
    audio: &AudioPool,
    assets: &suhina_engine::DeviceAssets,
    path: &Path,
    sample_rate: u32,
    plugins: Option<&PluginEnv>,
) -> Result<RenderSummary, String> {
    render_wavs(project, audio, assets, path, None, sample_rate, plugins)
}

/// Like [`render_wav`], and, with `cue`, the cue bus beside the main mix to a second WAV file
/// of the same length: the tracks whose cue is on, after their own volume and pan, at unity
/// cue volume. The main mix is the same either way.
pub fn render_wavs(
    project: &Project,
    audio: &AudioPool,
    assets: &suhina_engine::DeviceAssets,
    path: &Path,
    cue: Option<&Path>,
    sample_rate: u32,
    plugins: Option<&PluginEnv>,
) -> Result<RenderSummary, String> {
    let mut started = offline_plugins(project, audio, plugins, sample_rate);
    let running = std::mem::take(&mut started.plugins);
    let samples = match cue {
        None => {
            render_offline_with_assets(project, audio, assets, sample_rate, 512, running).map_err(|e| e.to_string())?
        }
        Some(cue_path) => {
            let (main, cued) = render_offline_with_cue(project, audio, assets, sample_rate, 512, running, 1.0, true)
                .map_err(|e| e.to_string())?;
            write_wav(cue_path, &cued, sample_rate)?;
            main
        }
    };
    write_wav(path, &samples, sample_rate)?;
    Ok(RenderSummary {
        seconds: samples.len() as f64 / 2.0 / f64::from(sample_rate),
        sample_rate,
        peak: samples.iter().fold(0.0f32, |m, s| m.max(s.abs())),
        plugins: started.notices,
    })
}

/// Writes interleaved stereo `samples` to a 32-bit float WAV file at `path`.
fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec =
        hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    for sample in samples {
        writer.write_sample(*sample).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    writer.finalize().map_err(|e| format!("{}: {e}", path.display()))
}

/// One step of a launch script: the command `id` with `params`, run when the render reaches
/// `at` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchStep {
    pub at: f64,
    pub id: String,
    pub params: Value,
}

/// Renders `seconds` of the project to a WAV file at `path`, starting with the transport
/// stopped and running each step's command when the render reaches its time, as if it were
/// typed into a live session at that moment. Only transport and read commands may be used.
/// Returns the summary and each step's result.
pub fn render_launch_script(
    project: &Project,
    audio: &AudioPool,
    path: &Path,
    sample_rate: u32,
    steps: &[LaunchStep],
    seconds: f64,
    plugins: Option<&PluginEnv>,
) -> Result<(RenderSummary, Vec<Value>), String> {
    if !(seconds.is_finite() && seconds > 0.0 && seconds <= MAX_SCRIPT_SECONDS) {
        return Err(format!("seconds must be more than 0 and at most {MAX_SCRIPT_SECONDS}, got {seconds}"));
    }
    if steps.len() > MAX_SCRIPT_STEPS {
        return Err(format!("a launch script may have at most {MAX_SCRIPT_STEPS} steps, got {}", steps.len()));
    }
    for step in steps {
        let kind = commands().iter().find(|c| c.id == step.id).map(|c| c.kind);
        match kind {
            Some(CommandKind::Transport | CommandKind::Read) => {}
            Some(_) => return Err(format!("{}: a launch script may only run transport and read commands", step.id)),
            None => return Err(format!("unknown command {}; command_list shows them", step.id)),
        }
        if !(step.at.is_finite() && step.at >= 0.0) {
            return Err(format!("{}: at must be a time in seconds, at least 0", step.id));
        }
    }
    let mut steps: Vec<&LaunchStep> = steps.iter().collect();
    steps.sort_by(|a, b| a.at.total_cmp(&b.at));

    let mut started = offline_plugins(project, audio, plugins, sample_rate);
    let running: Plugins = std::mem::take(&mut started.plugins);
    let mut renderer =
        Renderer::stopped_with_plugins(project, audio, sample_rate, running).map_err(|e| e.to_string())?;
    renderer.set_offline(true);
    let mut session = Session::new(project.clone());
    session.attach_player(Player::new(renderer.controller(), None));

    let spec =
        hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    let frame_of = |at: f64| (at * f64::from(sample_rate)).round() as u64;
    let total = frame_of(seconds);
    let mut block = vec![0.0f32; 2 * 512];
    let mut done = 0u64;
    let mut peak = 0.0f32;
    let mut results = Vec::new();
    let mut next = steps.into_iter().peekable();
    while done < total {
        while let Some(step) = next.next_if(|s| frame_of(s.at) <= done) {
            results.push(match session.execute(&step.id, &step.params) {
                Ok(result) => json!({ "at": step.at, "id": step.id, "ok": true, "result": result }),
                Err(e) => json!({ "at": step.at, "id": step.id, "ok": false, "error": e.0 }),
            });
        }
        let until = next.peek().map_or(total, |s| frame_of(s.at)).clamp(done + 1, total).min(done + 512);
        let frames = usize::try_from(until - done).unwrap_or(512);
        let Some(out) = block.get_mut(..2 * frames) else { break };
        renderer.process(out);
        for sample in out.iter() {
            peak = peak.max(sample.abs());
            writer.write_sample(*sample).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        done = until;
    }
    for step in next {
        results.push(json!({ "at": step.at, "id": step.id, "ok": false, "error": "after the end of the render" }));
    }
    writer.finalize().map_err(|e| format!("{}: {e}", path.display()))?;
    let summary =
        RenderSummary { seconds: done as f64 / f64::from(sample_rate), sample_rate, peak, plugins: started.notices };
    Ok((summary, results))
}

/// Replaces the arrangement with the clips of one launcher scene (0-based), each played once
/// from the start, as if the scene were launched at time zero.
pub fn scene_as_arrangement(project: &Project, scene: usize) -> Result<Project, String> {
    let count = project.launcher.scenes.len();
    if scene >= count {
        return Err(format!("scene {} does not exist; the project has {count} scenes", scene.saturating_add(1)));
    }
    let lanes = project
        .launcher
        .columns
        .iter()
        .map(|column| ArrangementLane {
            name: String::new(),
            track: Some(column.track),
            // A copy of the slot's clip: a clip of its own, given an id below.
            clips: column
                .slots
                .get(scene)
                .and_then(|s| s.clip.clone())
                .map(|c| Clip { id: ClipId::NONE, ..c })
                .into_iter()
                .collect(),
            ..ArrangementLane::default()
        })
        .collect();
    let mut project = project.clone();
    project.arrangement = Arrangement { lanes, markers: Vec::new() };
    project.assign_ids().map_err(|e| e.to_string())?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use suhina_engine::model::*;

    use super::*;

    const SR: u32 = 48_000;

    /// One track whose only slot loops a short note on every beat, at 120 bpm.
    fn project() -> Project {
        Project {
            tracks: vec![Track {
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "Drums")
            }],
            channels: vec![Channel::new(ChannelId(1), "Drums")],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "Beat".into(),
                length: 1.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 0.25, 36, 1.0)]) }],
                extensions: Extensions::default(),
                scale: None,
            }],
            launcher: Launcher {
                mappings: Vec::new(),
                scenes: vec![Scene::default()],
                columns: vec![LauncherColumn {
                    track: TrackId(1),
                    slots: vec![Slot {
                        clip: Some(Clip {
                            loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
                            ..Clip::new(ContentId(1), 0.0, 1.0)
                        }),
                        has_stop: true,
                    }],
                }],
            },
            ..Project::default()
        }
    }

    fn step(at: f64, id: &str, params: Value) -> LaunchStep {
        LaunchStep { at, id: id.into(), params }
    }

    fn temp_wav(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("suhina-render-{name}-{}.wav", std::process::id()))
    }

    /// Frames where sound starts after more than 100 silent frames, in the left channel.
    fn onsets(path: &Path) -> Vec<usize> {
        let mut reader = hound::WavReader::open(path).unwrap();
        let left: Vec<f32> = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
        let mut result = Vec::new();
        let mut silent_run = usize::MAX;
        for (i, s) in left.iter().enumerate() {
            let loud = s.abs() > 1e-6;
            if loud && silent_run > 100 {
                result.push(i);
            }
            silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
        }
        result
    }

    fn samples(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path).unwrap().samples::<f32>().map(Result::unwrap).collect()
    }

    #[test]
    fn the_cue_renders_beside_the_main_mix() {
        let (main_path, cue_path) = (temp_wav("cue-main"), temp_wav("cue-bus"));
        let mut song = scene_as_arrangement(&project(), 0).unwrap();
        let alone = temp_wav("cue-alone");
        render_wav(&song, &AudioPool::new(), &suhina_engine::DeviceAssets::new(), &alone, SR, None).unwrap();
        // Nothing cued: the cue file is silent and the main mix the same.
        render_wavs(
            &song,
            &AudioPool::new(),
            &suhina_engine::DeviceAssets::new(),
            &main_path,
            Some(&cue_path),
            SR,
            None,
        )
        .unwrap();
        assert_eq!(samples(&main_path), samples(&alone));
        assert!(samples(&cue_path).iter().all(|s| *s == 0.0));
        assert_eq!(samples(&cue_path).len(), samples(&main_path).len());
        // The track cued: on the cue as in the mix.
        song.channels[0].cue = true;
        render_wavs(
            &song,
            &AudioPool::new(),
            &suhina_engine::DeviceAssets::new(),
            &main_path,
            Some(&cue_path),
            SR,
            None,
        )
        .unwrap();
        let same = samples(&main_path).iter().zip(samples(&alone)).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(same && samples(&main_path).len() == samples(&alone).len());
        let (main, cue) = (samples(&main_path), samples(&cue_path));
        assert!(cue.iter().any(|s| s.abs() > 0.1));
        assert!(main.iter().zip(&cue).all(|(m, c)| (m - c).abs() < 1e-5));
        for path in [main_path, cue_path, alone] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn a_launch_script_renders_launches_at_their_times() {
        let path = temp_wav("script");
        let steps = [
            step(0.1, "clip.launch", json!({ "track": "Drums", "scene": 1 })),
            step(0.3, "track.stop", json!({ "track": "Drums" })),
            step(0.31, "launcher.state", json!({})),
            step(9.0, "transport.stop", json!({})),
        ];
        let (summary, results) =
            render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 3.0, None).unwrap();
        assert!((summary.seconds - 3.0).abs() < 1e-9);
        assert!(summary.peak > 0.1);
        // Launched at once (the transport was stopped), so the bar starts at 0.1 s; the stop waits
        // for the next bar, 2.1 s.
        let start = SR as usize / 10;
        let beat = SR as usize / 2;
        let expected = [start, start + beat, start + 2 * beat, start + 3 * beat];
        let actual = onsets(&path);
        assert_eq!(actual.len(), 4, "{actual:?}");
        assert!(actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2), "{actual:?}");
        assert_eq!(results.len(), 4);
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[2]["result"]["tracks"][0]["queued"], "stop");
        assert_eq!(results[3]["ok"], false);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_launch_script_can_move_the_play_start_marker() {
        let path = temp_wav("position");
        let steps = [
            step(0.0, "transport.setPosition", json!({ "beats": 8 })),
            step(0.0, "transport.play", json!({})),
            step(0.5, "launcher.state", json!({})),
            step(0.6, "transport.setPosition", json!({ "beats": -1 })),
        ];
        let (_, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 1.0, None).unwrap();
        assert_eq!(results[2]["result"]["start_beat"], 8.0);
        // Half a second at 120 bpm: about a beat on from the marker (read as the last block began).
        let beat = results[2]["result"]["beat"].as_f64().unwrap_or_default();
        assert!((8.9..=9.0).contains(&beat), "{beat}");
        assert_eq!(results[3]["ok"], false);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn launch_scripts_take_only_transport_and_read_commands() {
        let path = temp_wav("bad");
        let run = |steps: &[LaunchStep], seconds: f64| {
            render_launch_script(&project(), &AudioPool::new(), &path, SR, steps, seconds, None).map(|_| ())
        };
        assert!(run(&[step(0.0, "transport.setTempo", json!({ "bpm": 90 }))], 1.0).is_err());
        assert!(run(&[step(0.0, "edit.undo", json!({}))], 1.0).is_err());
        assert!(run(&[step(0.0, "nope", json!({}))], 1.0).is_err());
        assert!(run(&[step(f64::NAN, "transport.play", json!({}))], 1.0).is_err());
        assert!(run(&[step(-1.0, "transport.play", json!({}))], 1.0).is_err());
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_SCRIPT_SECONDS + 1.0] {
            assert!(run(&[], seconds).is_err(), "{seconds}");
        }
        // A failing command is reported in its step, not as a failed render.
        let steps = [step(0.0, "clip.launch", json!({ "track": "Drums", "scene": 5 }))];
        let (_, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 0.1, None).unwrap();
        assert_eq!(results[0]["ok"], false);
        let _ = std::fs::remove_file(&path);
    }
}
