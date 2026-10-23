// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.export`: rendering the mix, or one node of the track tree, to the audio file the
//! user chose ([`docs/spec/export.md`](../../docs/spec/export.md)). One command carries the
//! whole feature, so the app, `suhina-cli` and the MCP server export the same way. It renders
//! with the same offline render playback uses, changes no project state (no undo step), and
//! encodes through the one writer `media::write` is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use suhina_engine::{RenderInputs, RenderOptions};

use serde_json::{Value, json};
use suhina_engine::Renderer;
use suhina_engine::model::{Project, TrackId};
use suhina_media::write::{self, AudioFileSpec, AudioFormat, BitDepth};
use suhina_mp3::Mp3Rate;

use crate::commands::{number, track_at};
use crate::{CommandError, Session};

/// The render's frames per call, and the export's defaults.
const BLOCK_FRAMES: usize = 512;
/// Both a range and the whole arrangement are capped at what the engine itself takes for one
/// render, an hour of audio, checked before the first block against the render's own seconds.
const MAX_RANGE_SECONDS: f64 = suhina_engine::MAX_RENDER_SECONDS;

/// Renders the project's arrangement — or the sum at the node `source` names — and writes it
/// to the file `path` (or `output`, the same thing) says. Returns the summary scripts read:
/// `{path, format, sample_rate, bit_depth, seconds, peak, misses, warnings}`.
pub(crate) fn export(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    export_from(&session.export_data(), params)
}

/// Everything an export renders from: what [`export_from`] reads from the session, cloned for
/// a worker thread to run the render off the window's (`docs/spec/export.md` › In the app: the
/// export runs while playback keeps playing). The pool's and the device assets' samples are
/// shared through their `Arc`s, and the project is pure data, so the clone is cheap.
pub struct ExportData {
    /// State preparation warnings, retained with this job across project switches.
    pub warnings: Vec<String>,
    pub project: Project,
    pub audio: suhina_engine::AudioPool,
    pub assets: suhina_engine::DeviceAssets,
    /// The plug-ins' environment, shared; `None` where plug-ins are not hosted.
    pub plugins: Option<Arc<crate::PluginEnv>>,
    /// The project's folder, for the missing files' warnings.
    pub folder: Option<PathBuf>,
    pub trusted: suhina_media::Trusted,
    /// The project's own name: the stem of the file it was opened from, what the MP3 export
    /// writes as the ID3 title; empty when the project was never saved.
    pub title: String,
}

/// Runs an export against data taken off the session ([`Session::export_data`]): what a worker
/// thread calls, so the app's window keeps living while the render runs. The command
/// ([`export`]) and this are the same render; the errors are the command's texts.
pub fn export_from(data: &ExportData, params: &Value) -> Result<Value, CommandError> {
    // Worker callers bypass Session::execute, but keep the command's parameter contract.
    let spec =
        crate::commands().iter().find(|spec| spec.id == "project.export").ok_or("export command is unavailable")?;
    crate::commands::check_params(spec.id, spec.params, params)?;
    // The file: `path` or `output`, the same thing.
    let path_text = params
        .get("path")
        .filter(|value| !value.is_null())
        .or_else(|| params.get("output").filter(|value| !value.is_null()))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or("pass the file to write as path")?;
    let path = PathBuf::from(path_text);

    // The format: the parameter's, else the file name's extension's; they agree when both name
    // one. A format the command does not know is refused, never passed over for the extension.
    let asked = match params.get("format").filter(|value| !value.is_null()).and_then(Value::as_str) {
        None => None,
        Some(name) => Some(
            AudioFormat::from_extension(name)
                .ok_or_else(|| format!("{name} is not a format; wav, flac, ogg or mp3"))?,
        ),
    };
    let said = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    let format = match (asked, said.as_deref().and_then(AudioFormat::from_extension)) {
        (Some(a), Some(s)) if a != s => {
            return Err(format!("the file name says .{}, and the format says {}", s.name(), a.name()).into());
        }
        (Some(a), _) | (None, Some(a)) => a,
        (None, None) => {
            return Err("name the file .wav, .flac, .ogg or .mp3, or pass format".into());
        }
    };

    let sample_rate = match params.get("sampleRate") {
        None | Some(Value::Null) => 48_000u64,
        Some(n) => {
            n.as_u64().filter(|rate| (8_000..=384_000).contains(rate)).ok_or("sampleRate must be 8000 to 384000")?
        }
    } as u32;

    let depth = match params.get("bitDepth").filter(|value| !value.is_null()).and_then(Value::as_str) {
        Some(name) => BitDepth::from_name(name).ok_or_else(|| format!("{name} is not a bit depth; 32f, 24 or 16"))?,
        None => match format {
            AudioFormat::Wav => BitDepth::Float32,
            AudioFormat::Flac => BitDepth::Pcm24,
            AudioFormat::Ogg | AudioFormat::Mp3 => BitDepth::Float32,
        },
    };
    if !format.has_depth() && params.get("bitDepth").filter(|value| !value.is_null()).is_some() {
        return Err(format!("bitDepth does not apply to {}", format.name()).into());
    }
    if format == AudioFormat::Flac && depth == BitDepth::Float32 {
        return Err("flac is written at 24 or 16 bits, not 32f".into());
    }

    // The MP3 rate, the constant kbps or the variable quality; asked for only with mp3.
    let mp3_rate = match params.get("mp3Rate").filter(|value| !value.is_null()).and_then(Value::as_str) {
        Some(name) => {
            if format != AudioFormat::Mp3 {
                return Err("mp3Rate goes with mp3".into());
            }
            Mp3Rate::from_name(name)
                .ok_or_else(|| format!("{name} is not an mp3 rate; 128, 192, 256, 320, v0 or v2"))?
        }
        None => Mp3Rate::default(),
    };

    let overwrite = params.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    // Dither: on for a reduced depth unless the job says otherwise; not a thing at 32f.
    let dither = match params.get("dither") {
        Some(Value::Bool(d)) => *d,
        None => depth.is_reduced(),
        Some(_) => return Err("dither must be a bool".into()),
    };
    if !depth.is_reduced() && dither {
        return Err("dither does not apply at 32f".into());
    }
    let normalize = params.get("normalize").and_then(Value::as_bool).unwrap_or(false);

    // The tail, in beats: extra rendered after the range's end. It goes with a range, which
    // is also where a negative one is refused.
    let tail = tail_beats(params)?;

    // The range, in beats from the arrangement's start; left out, the whole arrangement.
    let range = match params.get("range").filter(|value| !value.is_null()) {
        None => {
            if tail != 0.0 {
                return Err("tail goes with range".into());
            }
            None
        }
        Some(object) => {
            let start = number(object, "start")?;
            let end = number(object, "end")?;
            if start < 0.0 || end > suhina_engine::MAX_START_BEAT {
                return Err(format!("start and end must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
            }
            if end - start < 1.0 {
                return Err("end must be at least one beat after start".into());
            }
            let beats_to_frames = |beat: f64| suhina_engine::frame_of_beat(&data.project, beat, sample_rate) as usize;
            let seconds = (beats_to_frames(end) - beats_to_frames(start)) as f64 / f64::from(sample_rate);
            if seconds > MAX_RANGE_SECONDS {
                return Err(
                    format!("the range would be {seconds:.0} s long; the limit is {MAX_RANGE_SECONDS:.0} s").into()
                );
            }
            Some((start, end))
        }
    };

    // An empty arrangement refuses to export.
    if !data.project.arrangement.lanes.iter().any(|lane| lane.clips.iter().any(|clip| clip.enabled)) {
        return Err("the arrangement is empty; add a clip before exporting".into());
    }

    // The node to render, when the job names one: a track or a group of the track tree, by id
    // or name. Left out, the whole mix is rendered.
    let source = match params.get("source").filter(|value| !value.is_null()) {
        None => None,
        Some(_) => Some(track_at(&data.project, params, "source")?),
    };

    // An existing file is replaced only when asked; a symlink is never written through.
    if let Some(error) = blocked_target(&path, overwrite) {
        return Err(error);
    }

    // The mix, of the arrangement's whole length so the stems line up with it: the whole mix,
    // or the sum at the named node. The job's plug-in devices are started as a scene render
    // starts them, and what did not start or did not take its state is said in the warnings.
    // `started` stays whole (its controls end the host processes), with the fields taken out
    // one by one: dropping it here would end the hosts and every block would miss its
    // plug-in's answer.
    let started = match data.plugins.as_deref() {
        Some(env) => crate::rack::start_offline(&data.project, &data.audio, env, sample_rate),
        None => crate::rack::OfflinePlugins::none(),
    };
    let notices = started.notices.clone();
    let mut renderer = Renderer::build(
        &data.project,
        RenderInputs::from_audio(&data.audio).with_assets(&data.assets).with_plugins(started.plugins),
        sample_rate,
        RenderOptions { source, ..RenderOptions::PLAYING },
    )
    .map_err(|e| e.to_string())?;
    renderer.set_offline(true);
    // The whole arrangement, once: a render does not repeat the loop region.
    renderer.set_follow_loop(false);
    let total = match range {
        None => usize::try_from(renderer.length()).unwrap_or(usize::MAX),
        Some((_, end)) => suhina_engine::frame_of_beat(&data.project, end + tail, sample_rate) as usize,
    };
    let seconds = total as f64 / f64::from(sample_rate);
    if seconds > MAX_RANGE_SECONDS {
        return Err(format!("the export would be {seconds:.0} s long; the limit is {MAX_RANGE_SECONDS:.0} s").into());
    }
    // The samples are two f32 channels per frame, capped by bytes so a valid-looking ask at a
    // high sample rate cannot exhaust memory.
    const MAX_RENDER_BYTES: usize = 2 * 1024 * 1024 * 1024;
    let bytes = total.saturating_mul(2).saturating_mul(size_of::<f32>());
    if bytes > MAX_RENDER_BYTES {
        return Err(format!(
            "the export would be {:.1} GiB of samples; the cap is 2.0 GiB",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        )
        .into());
    }
    let mut out = vec![0.0f32; total * 2];
    for block in out.chunks_mut(BLOCK_FRAMES * 2) {
        renderer.process(block);
    }

    // The job's share of the render: the whole of it, or the samples from its start beat,
    // dropped in place so no second buffer of the render's size is ever made.
    let start_frame =
        range.map_or(0, |(start, _)| suhina_engine::frame_of_beat(&data.project, start, sample_rate) as usize);
    let skip = start_frame.saturating_mul(2).min(out.len());
    out.drain(..skip);
    let mut written = out;
    if written.is_empty() {
        return Err("the range holds no audio between its beats".into());
    }

    // The file's peak, scaled to -0.3 dBFS when the job says so.
    let mut peak = written.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if normalize && peak > 0.0 {
        let scale = 10f32.powf(-0.3 / 20.0) / peak;
        for sample in &mut written {
            *sample *= scale;
        }
        peak = 10f32.powf(-0.3 / 20.0);
    }

    write::write_audio_file(
        &path,
        &written,
        &AudioFileSpec {
            format,
            depth,
            sample_rate,
            dither,
            seed: export_seed(params),
            mp3_rate,
            title: data.title.clone(),
        },
        write::WriteMode::Export { overwrite },
    )?;

    // What the user should hear about: the plug-ins that did not start or take their state,
    // the returns a node render leaves out, and the files the clips name that cannot be found.
    let warnings =
        [data.warnings.clone(), notices, send_warnings(&data.project, source), missing_warnings(data)].concat();
    Ok(json!({
        "path": path.display().to_string(),
        "format": format.name(),
        "sample_rate": sample_rate,
        // A format without a depth (ogg, mp3) reports none: the float the encoder is fed is
        // not a property of the file.
        "bit_depth": if format.has_depth() { json!(depth.name()) } else { Value::Null },
        "seconds": written.len() as f64 / 2.0 / f64::from(sample_rate),
        "peak": peak,
        "misses": renderer.plugin_misses(),
        "warnings": warnings,
    }))
}

/// The seed the dither's noise is made from: the export's `seed` parameter, or one hashed
/// from the render's own parameters — the file's name and place are not among them, so the
/// same render makes the same noise in any file.
fn export_seed(params: &Value) -> u64 {
    if let Some(seed) = params.get("seed").filter(|value| !value.is_null()).and_then(Value::as_u64) {
        return seed;
    }
    let mut render = params.clone();
    if let Some(object) = render.as_object_mut() {
        for key in ["path", "output", "overwrite", "seed"] {
            object.remove(key);
        }
    }
    // FNV-1a over the parameters' text: stable for the same command.
    let text = render.to_string();
    let mut seed = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        seed ^= u64::from(byte);
        seed = seed.wrapping_mul(0x0100_0000_01b3);
    }
    seed
}

/// The tail, in beats, the job names; 0 when it does not.
fn tail_beats(params: &Value) -> Result<f64, CommandError> {
    match params.get("tail").filter(|value| !value.is_null()) {
        None => Ok(0.0),
        Some(_) => {
            let beats = number(params, "tail")?;
            if beats < 0.0 {
                return Err("tail must be at least 0 beats".into());
            }
            Ok(beats)
        }
    }
}

/// What the user should hear about: the audio files the project's clips name that cannot be
/// found.
fn missing_warnings(data: &ExportData) -> Vec<String> {
    crate::relink::missing_of(&data.project, data.folder.as_deref(), &data.audio, &data.trusted)
        .into_iter()
        .map(|missing| format!("{} is missing", missing.file))
        .collect()
}

/// The returns a node render's source subtree sends to, named for the result's warnings: a
/// node render leaves the return chains out (`docs/spec/export.md` › Sources). The whole mix
/// renders its sends, so without a source there is nothing to say.
fn send_warnings(project: &suhina_engine::model::Project, source: Option<TrackId>) -> Vec<String> {
    use suhina_engine::model::ChannelRole;

    let Some(source) = source else { return Vec::new() };
    let Some(own) = project.track(source).and_then(|t| t.channel) else {
        return Vec::new();
    };
    // The channels that play into the node: their output chains reach the source's channel.
    // Each channel joins at most once, so the walk ends however the project routes.
    let mut under = vec![own];
    let mut grew = true;
    while grew {
        grew = false;
        for channel in &project.channels {
            if let Some(out) = channel.output
                && under.contains(&out)
                && !under.contains(&channel.id)
            {
                under.push(channel.id);
                grew = true;
            }
        }
    }
    // The distinct returns the subtree sends to; other targets are the render matrix's say.
    let mut said = Vec::new();
    let mut warnings = Vec::new();
    for channel in project.channels.iter().filter(|c| under.contains(&c.id)) {
        for send in &channel.sends {
            if !said.contains(&send.target)
                && let Some(target) = project.channel(send.target)
                && target.role == ChannelRole::Effect
            {
                said.push(send.target);
                warnings.push(format!("sends to {} are not in this render", target.name));
            }
        }
    }
    warnings
}

/// Nothing blocks the target: a symlink is never written through, a folder is never a file,
/// and an existing file is replaced only when `overwrite` says so.
fn blocked_target(path: &Path, overwrite: bool) -> Option<CommandError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Some(format!("{} is a symlink; refusing to write through it", path.display()).into())
        }
        Ok(meta) if meta.is_dir() => Some(format!("{} is a folder", path.display()).into()),
        Ok(_) if !overwrite => {
            Some(format!("{} already exists; pass overwrite: true to replace it", path.display()).into())
        }
        _ => None,
    }
}

impl Session {
    /// Starts a caller-owned preparation round, refusing to consume another caller's reads.
    pub fn begin_export_gather(&mut self, deadline: std::time::Instant) -> Result<ExportGather, CommandError> {
        if self.plugins.gathering() {
            return Err("an export is already gathering this project's plug-in state".into());
        }
        Ok(self.gather_export_data(deadline))
    }

    /// What an export renders from ([`export_from`]), taken off the session so a worker thread
    /// can run the render while the window and the audio thread carry on: the samples and the
    /// device assets share theirs through `Arc`s, so the clone is cheap. The running plug-ins'
    /// state is read now, as a save reads it ([`Session::project_as_heard`], ADR 0006
    /// decision 5), so the export plays what is heard: a slow plug-in is waited for at most a
    /// second, and what it left is said in the session's notices. The project is not changed
    /// by the read, and no undo step is recorded.
    pub fn export_data(&mut self) -> ExportData {
        ExportData {
            project: self.project_as_heard(),
            warnings: self.plugins.take_export_notices(),
            audio: self.audio.pool.clone(),
            assets: self.device_assets().clone(),
            plugins: self.plugins.env_handle(),
            folder: self.audio_folder().map(Path::to_path_buf),
            trusted: self.audio.trusted.clone(),
            title: self.export_title(),
        }
    }

    /// Cancels a gathering export: the round's reads stand where they are, late replies
    /// change nothing and start nothing, and a later export gathers its own round.
    pub fn cancel_export_gather(&mut self) {
        self.plugins.cancel_gather();
    }

    /// An export's data, gathered without ever waiting on the caller's thread: the running
    /// plug-ins' state is asked over the caller's frames ([`Session::export_data`]'s state
    /// read, without its waits), the answers showing up as the caller pumps this once per
    /// frame, bounded by `deadline` the way a save's one-second wait is. A pending result's
    /// caller keeps drawing frames and calls again; a ready one carries the whole data, with
    /// whatever the reads brought as they are ([`Session::export_data`]'s fallback for an
    /// un-answered plug-in). No undo step is recorded, and the project is not changed.
    pub fn gather_export_data(&mut self, deadline: std::time::Instant) -> ExportGather {
        if !self.plugins.gather_states(deadline) {
            return ExportGather::Pending;
        }
        ExportGather::Ready(Box::new(self.export_data_as_known()))
    }
}

impl Session {
    /// [`Session::export_data`], with the state the reads last brought in: whatever the
    /// gather settled on, asked for again nowhere.
    fn export_data_as_known(&mut self) -> ExportData {
        let states = self.plugins.known_states();
        ExportData {
            project: self.with_states(&states),
            warnings: self.plugins.take_export_notices(),
            audio: self.audio.pool.clone(),
            assets: self.device_assets().clone(),
            plugins: self.plugins.env_handle(),
            folder: self.audio_folder().map(Path::to_path_buf),
            trusted: self.audio.trusted.clone(),
            title: self.export_title(),
        }
    }

    /// The title an mp3 export writes: the stem of the file the project was opened from, or
    /// nothing for a project never saved.
    fn export_title(&self) -> String {
        self.source().and_then(Path::file_stem).map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// What [`Session::gather_export_data`] answers with: pending over the caller's frames, or
/// the data a worker thread takes (boxed; the two sizes are far apart).
pub enum ExportGather {
    /// More frames: the state reads are coming back.
    Pending,
    /// The export's data, the plug-ins' states as they were heard.
    Ready(Box<ExportData>),
}
