// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Offline rendering to WAV files, shared by the CLI and the MCP server.

use std::path::Path;
use suhina_engine::{RenderInputs, RenderOptions, render_offline_from};

use serde_json::{Value, json};
use suhina_engine::model::{Arrangement, ArrangementLane, Clip, ClipId, Project};
use suhina_engine::{AudioPool, Plugins, Renderer, render_offline_with_cue};
use suhina_session::{CommandKind, OfflinePlugins, Player, PluginEnv, Session, commands, start_offline};

use crate::workspace::Workspace;

/// Longest launch-script render, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = 600.0;
/// Most steps in one launch script.
pub const MAX_SCRIPT_STEPS: usize = 1024;

/// Commands of the `Read` kind that write a file of their own: the export its audio file, the
/// save the project's file. A launch script is the model of one live pass — transport commands
/// drive it, read commands look at the project — so a file-writing command never rides in it:
/// its destination is not the render's own output at `path`, and the read whitelist would hand
/// it a write it never earned. Every file a script's render makes is the render's own.
const WRITE_EVEN_WHEN_READ: [&str; 2] = ["project.export", "project.save"];

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
        None => render_offline_from(
            project,
            RenderInputs::from_audio(audio).with_assets(assets).with_plugins(running),
            sample_rate,
            512,
            None,
        )
        .map_err(|e| e.to_string())?,
        Some(cue_path) => {
            let (main, cued) = render_offline_with_cue(
                project,
                RenderInputs::from_audio(audio).with_assets(assets).with_plugins(running),
                sample_rate,
                512,
                1.0,
                true,
            )
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

/// Writes interleaved stereo `samples` to a 32-bit float WAV file at `path`, through the one
/// writer every export writes with (`media::write`). The file `project_render` was checked
/// against the workspace before the render, and is replaced like any render the same caller
/// made before.
fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    use suhina_media::write::WriteMode;
    suhina_media::write::write_audio_file(
        path,
        samples,
        &suhina_media::write::AudioFileSpec::float_wav(sample_rate),
        WriteMode::Replace,
    )
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
/// `workspace`, when the caller has one, confines every step's paths the way a tool call's are
/// confined, so the script's commands read and write only where its caller may. Returns the
/// summary and each step's result.
#[allow(clippy::too_many_arguments)]
pub fn render_launch_script(
    project: &Project,
    audio: &AudioPool,
    path: &Path,
    sample_rate: u32,
    steps: &[LaunchStep],
    seconds: f64,
    plugins: Option<&PluginEnv>,
    workspace: Option<&Workspace>,
) -> Result<(RenderSummary, Vec<Value>), String> {
    if !(seconds.is_finite() && seconds > 0.0 && seconds <= MAX_SCRIPT_SECONDS) {
        return Err(format!("seconds must be more than 0 and at most {MAX_SCRIPT_SECONDS}, got {seconds}"));
    }
    if steps.len() > MAX_SCRIPT_STEPS {
        return Err(format!("a launch script may have at most {MAX_SCRIPT_STEPS} steps, got {}", steps.len()));
    }
    let mut prepared: Vec<LaunchStep> = Vec::with_capacity(steps.len());
    for step in steps {
        if WRITE_EVEN_WHEN_READ.contains(&step.id.as_str()) {
            return Err(format!(
                "{id} writes a file, and a launch script may only run transport and read commands that do not; the render's own output is written at the path the render was given",
                id = step.id
            ));
        }
        let kind = commands().iter().find(|c| c.id == step.id).map(|c| c.kind);
        match kind {
            Some(CommandKind::Transport | CommandKind::Read) => {}
            Some(_) => return Err(format!("{}: a launch script may only run transport and read commands", step.id)),
            None => return Err(format!("unknown command {}; command_list shows them", step.id)),
        }
        // The confined parameters are the ones the step runs: confinement resolves the paths a
        // caller named relatively against the permitted root, while a command resolves what it
        // is given against the process's working directory — executing the originals would
        // authorize one file and read another.
        let params = if let Some(workspace) = workspace {
            crate::confine::confine(workspace, &step.id, step.params.clone(), None)
                .map_err(|why| format!("{}: {why}", step.id))?
        } else {
            step.params.clone()
        };
        if !(step.at.is_finite() && step.at >= 0.0) {
            return Err(format!("{}: at must be a time in seconds, at least 0", step.id));
        }
        prepared.push(LaunchStep { at: step.at, id: step.id.clone(), params });
    }
    let mut steps: Vec<&LaunchStep> = prepared.iter().collect();
    steps.sort_by(|a, b| a.at.total_cmp(&b.at));

    let mut started = offline_plugins(project, audio, plugins, sample_rate);
    let running: Plugins = std::mem::take(&mut started.plugins);
    let mut renderer = Renderer::build(
        project,
        RenderInputs::from_audio(audio).with_plugins(running),
        sample_rate,
        RenderOptions::STOPPED,
    )
    .map_err(|e| e.to_string())?;
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
                Err(e) => json!({ "at": step.at, "id": step.id, "ok": false, "error": e.message().to_owned() }),
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
#[path = "render_tests.rs"]
mod tests;
