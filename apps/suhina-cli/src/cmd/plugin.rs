// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-cli plugin …` and `suhina-cli plugins …`: running, listing and editing plug-ins.

use crate::*;

/// `plugin run …` and `plugin run-device …`: audio, and an instrument's notes, through one
/// plug-in offline, written to a WAV file.
pub(crate) fn plugin(args: &[String]) -> Result<(), String> {
    let usage = || {
        format!(
            "usage: suhina-cli plugin run <plug-in> (<in-audio> | -) <out.wav> [--tail <seconds>] [--state <file>]
       suhina-cli plugin run-device <project.als> <device> (<in-audio> | -) <out.wav> [--tail <seconds>]
                                     [--from <bar>] [--seconds <seconds>]
       suhina-cli plugin (params <plug-in> | params-device <project.als> <device>)
       suhina-cli plugin (edit <plug-in> | edit-device <project.als> <device>) [--save <file>]

{USAGE}"
        )
    };
    let mut words = args.iter().map(String::as_str);
    let mut library = suhina_plugins::library::Library::default();
    let (target, mut state, input, output) = match words.next() {
        Some("run") => {
            let (Some(name), Some(input), Some(output)) = (words.next(), words.next(), words.next()) else {
                return Err(usage());
            };
            (Target::Alone(plugin_spec(&mut library, name)?), None, input, output)
        }
        Some("params") => {
            let Some(name) = words.next() else { return Err(usage()) };
            let spec = plugin_spec(&mut library, name)?;
            return print_params(&library, spec, None);
        }
        Some("edit") => {
            let Some(name) = words.next() else { return Err(usage()) };
            let spec = plugin_spec(&mut library, name)?;
            return edit_plugin(&library, spec, None, words);
        }
        Some("edit-device") => {
            let (Some(project), Some(device)) = (words.next(), words.next()) else { return Err(usage()) };
            let (found, env) = open_device(&mut library, project, device)?;
            return edit_device(found, env, words);
        }
        Some("params-device") => {
            let (Some(project), Some(device)) = (words.next(), words.next()) else { return Err(usage()) };
            let (found, env) = open_device(&mut library, project, device)?;
            return print_device_params(found, env);
        }
        Some("run-device") => {
            let (Some(project), Some(device), Some(input), Some(output)) =
                (words.next(), words.next(), words.next(), words.next())
            else {
                return Err(usage());
            };
            let (found, env) = open_device(&mut library, project, device)?;
            (Target::Device(Box::new(found), env), None, input, output)
        }
        _ => return Err(usage()),
    };
    let mut tail = 2.0f64;
    let mut from_bar: Option<f64> = None;
    let mut seconds = 16.0f64;
    let positive = |text: Option<&str>, what: &str, max: f64| {
        text.and_then(|t| t.parse::<f64>().ok())
            .filter(|t| t.is_finite() && (0.0..=max).contains(t))
            .ok_or_else(|| format!("{what} needs a number from 0 to {max}"))
    };
    while let Some(word) = words.next() {
        match word {
            "--state" => {
                let file = words.next().ok_or("--state needs a file")?;
                let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
                state = Some(bytes.into());
            }
            "--from" => from_bar = Some(positive(words.next(), "--from", 100_000.0)?),
            "--seconds" => seconds = positive(words.next(), "--seconds", 3_600.0)?,
            "--tail" => {
                tail = words
                    .next()
                    .and_then(|t| t.parse::<f64>().ok())
                    .filter(|t| t.is_finite() && (0.0..=600.0).contains(t))
                    .ok_or("--tail needs seconds, from 0 to 600")?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let audio = if input == "-" {
        let rate = 48_000u32;
        suhina_media::AudioData {
            sample_rate: rate,
            channels: 2,
            samples: vec![0.0; (seconds * f64::from(rate)) as usize * 2],
        }
    } else {
        suhina_media::decode_file(std::path::Path::new(input))?
    };
    let channels = usize::from(audio.channels.max(1));
    let stereo: Vec<f32> = audio
        .samples
        .chunks(channels)
        .flat_map(|frame| {
            let left = frame.first().copied().unwrap_or(0.0);
            [left, frame.get(1).copied().unwrap_or(left)]
        })
        .collect();
    let tail_frames = (tail * f64::from(audio.sample_rate)) as usize;
    // An instrument's notes: its track's arrangement, from the bar asked for or the first clip.
    let events = match &target {
        Target::Device(device, _) => {
            let (session, channel) = (&device.session, &device.channel);
            let project = session.project();
            let meter = project.transport.meter();
            let first_clip = || {
                let track = project.tracks.iter().find(|t| t.channel == Some(*channel))?;
                project
                    .arrangement
                    .lanes
                    .iter()
                    .filter(|l| l.track == Some(track.id))
                    .flat_map(|l| l.clips.iter())
                    .map(|c| c.start)
                    .filter(|s| s.is_finite())
                    .min_by(f64::total_cmp)
            };
            let from_beat = match from_bar {
                // Bars through the time-signature changes, counted from 1; a fraction is part of
                // that bar.
                Some(bar) => {
                    let bars = (bar - 1.0).clamp(0.0, f64::from(u32::MAX));
                    let start = meter.bar_start(bars.floor() as u32);
                    start + bars.fract() * meter.position(start).bar_beats
                }
                None => first_clip().unwrap_or(0.0),
            };
            let frames = (stereo.len() / 2) as u64;
            let events = track_notes::events(project, *channel, from_beat, frames, audio.sample_rate);
            if !events.is_empty() {
                let here = meter.position(from_beat);
                let bar = f64::from(here.bar) + 1.0 + (from_beat - here.bar_start) / here.bar_beats.max(1e-9);
                eprintln!("{} note events from bar {bar:.1}", events.len());
            }
            events
        }
        Target::Alone(_) => Vec::new(),
    };
    let started = std::time::Instant::now();
    let rendered = match target {
        Target::Device(device, env) => {
            render_device(&device, &env, audio.sample_rate, state, &stereo, &events, tail_frames)?
        }
        Target::Alone(spec) => {
            let launch = suhina_plugins::Launch {
                host: library.host().to_path_buf(),
                plugin: spec,
                channels: 2,
                max_frames: suhina_plugins::offline::BLOCK_FRAMES,
                sample_rate: f64::from(audio.sample_rate),
                timeout: std::time::Duration::from_secs(30),
                group: None,
                ara: None,
            };
            suhina_plugins::offline::render_with_events(launch, state, &stereo, &events, tail_frames)
                .map_err(|e| e.to_string())?
        }
    };
    if let Some(why) = &rendered.state_refused {
        eprintln!("warning: the plug-in did not take the saved settings ({why}); it ran with its defaults");
    }

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(output, spec).map_err(|e| format!("{output}: {e}"))?;
    for sample in &rendered.samples {
        writer.write_sample(*sample).map_err(|e| format!("{output}: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("{output}: {e}"))?;
    eprintln!(
        "wrote {output}: {:.1} s at {} Hz, plug-in latency {} frames (compensated), in {:.1} s",
        rendered.samples.len() as f64 / 2.0 / f64::from(audio.sample_rate),
        audio.sample_rate,
        rendered.latency,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// What `plugin run` and `run-device` run audio through: a plug-in on its own, by how the host
/// is asked to load it, or a project's plug-in device with the catalog it was found in.
enum Target {
    Alone(String),
    Device(Box<ProjectDevice>, std::sync::Arc<suhina_session::PluginEnv>),
}

pub(crate) fn plugins(args: &[String]) -> Result<(), String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut projects = suhina_session::Projects::new();
    let (id, params) = match words.as_slice() {
        ["check", project] => {
            let (session, warnings) =
                suhina_session::Session::open(Path::new(project)).map_err(|e| e.message().to_owned())?;
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            projects.add(session, None).map_err(|e| e.message().to_owned())?;
            ("plugin.check", serde_json::json!({}))
        }
        ["scan"] => ("plugin.scan", serde_json::json!({})),
        ["scan", "--all"] => ("plugin.scan", serde_json::json!({ "all": true })),
        ["list"] => ("plugin.list", serde_json::json!({})),
        ["list", "--ignored"] => ("plugin.list", serde_json::json!({ "ignored": true })),
        ["folders"] => ("plugin.folders", serde_json::json!({})),
        ["add-folder", folder] => ("plugin.addFolder", serde_json::json!({ "folder": folder })),
        ["add-folder", folder, "--format", format] => {
            ("plugin.addFolder", serde_json::json!({ "folder": folder, "format": format }))
        }
        ["folder-on", folder] => ("plugin.setFolderEnabled", serde_json::json!({ "folder": folder, "enabled": true })),
        ["folder-off", folder] => {
            ("plugin.setFolderEnabled", serde_json::json!({ "folder": folder, "enabled": false }))
        }
        ["enable", plugin] => ("plugin.setEnabled", serde_json::json!({ "plugin": plugin, "enabled": true })),
        ["disable", plugin] => ("plugin.setEnabled", serde_json::json!({ "plugin": plugin, "enabled": false })),
        ["scan-file", file] => ("plugin.scanFile", serde_json::json!({ "file": file })),
        ["remove-folder", folder] => ("plugin.removeFolder", serde_json::json!({ "folder": folder })),
        ["set-hosting", plugin, hosting] => {
            ("plugin.setHosting", serde_json::json!({ "plugin": plugin, "hosting": hosting }))
        }
        _ => {
            return Err(format!(
                "unknown plugins command

{USAGE}"
            ));
        }
    };
    if id == "plugin.scan" || id == "plugin.scanFile" {
        eprintln!("scanning for plug-ins; each new or changed file is loaded in a process of its own");
    }
    let result = projects.execute(id, &params).map_err(|e| e.message().to_owned())?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// How the host is asked to load the plug-in `name`: a `clap:`, `vst3:` or `test:` name as it
/// is, else a name or id from the catalog.
pub(crate) fn plugin_spec(library: &mut suhina_plugins::library::Library, name: &str) -> Result<String, String> {
    if ["clap:", "vst3:", "vst2:", "test:"].iter().any(|p| name.starts_with(p)) {
        return Ok(name.to_string());
    }
    let found = library.catalog().find(name).ok_or_else(|| {
        format!("no plug-in {name:?} in the catalog; `suhina-cli plugins scan` looks for installed ones")
    })?;
    eprintln!("{} by {} ({})", found.name, found.vendor, found.path.display());
    Ok(found.spec())
}

/// A project's plug-in device the CLI works on: the session the project is open in, and where
/// the device is in it.
pub(crate) struct ProjectDevice {
    pub(crate) session: suhina_session::Session,
    /// The device's channel, for its track's notes.
    pub(crate) channel: ChannelId,
    /// How the plug-in-device commands name it: its track (or main) and its place on it; `None`
    /// on a channel no track plays through, which those commands cannot name.
    pub(crate) params: Option<serde_json::Value>,
    /// Its plug-in device key, which the session's plug-in host runs it by.
    pub(crate) key: u64,
}

/// [`project_device`] with the catalog of the installed plug-ins that are switched on.
fn open_device(
    library: &mut suhina_plugins::library::Library,
    project: &str,
    device: &str,
) -> Result<(ProjectDevice, std::sync::Arc<suhina_session::PluginEnv>), String> {
    let env = std::sync::Arc::new(suhina_session::PluginEnv::from_library(library));
    Ok((project_device(&env, project, device)?, env))
}

/// A project's plug-in device by name, among its channels' devices, and the installed plug-in
/// found for it in `env`'s catalog (plug-ins switched off in Settings are not in it). The
/// project is opened as `render` and the app open it, trusting what the person's library
/// trusts; its audio is decoded only for a plug-in that edits clips (ARA), the one kind that
/// hears it.
pub(crate) fn project_device(
    env: &suhina_session::PluginEnv,
    project: &str,
    device: &str,
) -> Result<ProjectDevice, String> {
    let mut projects = suhina_session::Projects::new();
    projects.set_file_library(suhina_library::Library::new(suhina_plugins::library::settings_folder()));
    let options = suhina_session::OpenOptions { audio: false };
    let (mut session, warnings) =
        projects.open_path(Path::new(project), options).map_err(|e| e.message().to_owned())?;
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let (channel, channel_id, index, saved) = session
        .project()
        .channels
        .iter()
        .find_map(|c| {
            let index = c.devices.iter().position(|d| d.name == device)?;
            Some((c.name.clone(), c.id, index, c.devices.get(index)?.clone()))
        })
        .ok_or_else(|| format!("no device {device:?} in {project}; `suhina-cli plugins check` lists them"))?;
    let (found, _) = suhina_session::device_plugin(&env.catalog, &saved)
        .ok_or_else(|| format!("no installed plug-in stands for {device:?}; `suhina-cli plugins check` says why"))?;
    eprintln!(
        "{device} on {channel}: {} by {} ({}), found by {}{}",
        found.plugin.name,
        found.plugin.vendor,
        found.plugin.path.display(),
        found.how.name(),
        if found.how.keeps_state() { "" } else { "; it may not read the saved settings" }
    );
    if found.plugin.ara {
        for warning in projects.load_audio(&mut session) {
            eprintln!("warning: {warning}");
        }
    }
    let placed = suhina_engine::plugin_devices(session.project())
        .into_iter()
        .find(|d| d.channel == channel_id && d.path == [index])
        .ok_or_else(|| format!("{device:?} is not a plug-in device"))?;
    let params = placed.track.map(|track| serde_json::json!({ "track": track.0, "device": index }));
    Ok(ProjectDevice { session, channel: channel_id, params, key: placed.key })
}

/// The sample rate the session's plug-in host runs a device at for `params-device` and
/// `edit-device`, as `params` and `edit` do.
const HOST_RATE: u32 = 48_000;
/// How often `edit-device` looks at the plug-in while its editor is open.
const EDIT_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// A project's plug-in device running on the session's own plug-in host, as it runs while the
/// app plays the project, with no audio output: the engine's blocks are run here, a block each
/// time it is looked at, so the plug-in gets the parameter changes that come with blocks.
struct Hosted {
    device: ProjectDevice,
    renderer: suhina_engine::Renderer,
    block: Vec<f32>,
}

impl Hosted {
    /// Starts the device's plug-in, and only it, on the session's plug-in host and waits until
    /// it runs. Fails as the app reports it: not installed, could not start, crashed.
    fn start(mut device: ProjectDevice, env: std::sync::Arc<suhina_session::PluginEnv>) -> Result<Self, String> {
        if device.params.is_none() {
            return Err("the device is on a channel no track plays through; the device commands cannot name it".into());
        }
        let session = &mut device.session;
        let rack = session.rack_mut();
        rack.set_env(env);
        rack.set_owner(None);
        rack.only(Some(device.key));
        // No backups to keep: a backup's state read lends the plug-in out, and a command at that
        // moment would find it busy.
        rack.set_save_every(std::time::Duration::MAX);
        let mut renderer = suhina_engine::Renderer::build(
            session.project(),
            suhina_engine::RenderInputs::from_audio(session.audio(), suhina_session::devices())
                .with_assets(session.device_assets()),
            HOST_RATE,
            suhina_engine::RenderOptions::STOPPED,
        )
        .map_err(|e| e.to_string())?;
        session.attach_player(suhina_session::Player::new(renderer.controller(), None));
        let mut hosted =
            Self { device, renderer, block: vec![0.0; 2 * suhina_plugins::offline::BLOCK_FRAMES as usize] };
        let deadline = std::time::Instant::now() + suhina_session::START_TIMEOUT;
        // Running in two looks in a row: one found to edit clips as it loads is stopped in the
        // look after it first runs, to load again with its track's clips.
        let mut running = 0;
        while running < 2 {
            match hosted.poll() {
                Look::Running => running += 1,
                Look::Starting => running = 0,
                Look::Stopped(why) => return Err(why),
            }
            if running < 2 {
                if std::time::Instant::now() >= deadline {
                    return Err("the plug-in did not start in time".into());
                }
                std::thread::sleep(START_POLL);
            }
        }
        Ok(hosted)
    }

    /// Takes in what the plug-in host did since (a plug-in that started, crashed or closed its
    /// editor), runs one engine block, and says where the device's plug-in stands. Prints what
    /// the session would tell the person (a refused state, an editor that did not close).
    fn poll(&mut self) -> Look {
        for notice in self.device.session.sync_plugins() {
            eprintln!("warning: {notice}");
        }
        self.renderer.process(&mut self.block);
        let key = self.device.key;
        let rack = self.device.session.rack_mut();
        match rack.report(key).map(|r| r.status) {
            Some(suhina_session::PluginStatus::Running) => Look::Running,
            Some(suhina_session::PluginStatus::Loading) => Look::Starting,
            None if rack.reloading(key) => Look::Starting,
            Some(suhina_session::PluginStatus::Missing) | None => {
                Look::Stopped("no installed plug-in stands for the device; `suhina-cli plugins check` says why".into())
            }
            Some(suhina_session::PluginStatus::Crashed(why)) => Look::Stopped(format!("the plug-in crashed: {why}")),
            Some(suhina_session::PluginStatus::Failed(why)) => {
                Look::Stopped(format!("the plug-in could not start: {why}"))
            }
        }
    }

    /// Whether the device's plug-in has its editor open now.
    fn editor_open(&mut self) -> bool {
        let key = self.device.key;
        self.device.session.rack_mut().report(key).is_some_and(|r| r.editor_open)
    }

    /// Runs the plug-in-device command `id` on the device.
    fn execute(&mut self, id: &str) -> Result<serde_json::Value, String> {
        let params = self.device.params.clone().unwrap_or_default();
        self.device.session.execute(id, &params).map_err(|e| e.message().to_owned())
    }
}

/// Where a device's plug-in stands, as [`Hosted::poll`] finds it.
enum Look {
    Running,
    /// Starting, or stopped to load again with its track's clips.
    Starting,
    /// It will not run: why.
    Stopped(String),
}

/// How often [`Hosted::start`] looks whether the plug-in runs yet.
const START_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// `plugin params-device`: the device's parameters as the session's `device.pluginParams` lists
/// them, from the plug-in running on the session's host with the state the project saved.
fn print_device_params(device: ProjectDevice, env: std::sync::Arc<suhina_session::PluginEnv>) -> Result<(), String> {
    let mut hosted = Hosted::start(device, env)?;
    let list = hosted.execute("device.pluginParams")?;
    println!("{}", serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?);
    Ok(())
}

/// `plugin edit-device`: opens the device's editor with `device.openEditor` on the session's
/// plug-in host and waits until its window is closed (or `--close-after` seconds pass, then
/// `device.closeEditor`); with `--save <file>`, writes the state the plug-in has then, as
/// `edit` does.
fn edit_device<'a>(
    device: ProjectDevice,
    env: std::sync::Arc<suhina_session::PluginEnv>,
    words: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let (save, close_after) = edit_options(words)?;
    let mut hosted = Hosted::start(device, env)?;
    let opened = hosted.execute("device.openEditor")?;
    match (
        opened.get("width").and_then(serde_json::Value::as_u64),
        opened.get("height").and_then(serde_json::Value::as_u64),
    ) {
        (Some(width), Some(height)) => eprintln!("editor open ({width} × {height}); close its window to finish"),
        _ => eprintln!("editor opening; close its window to finish"),
    }
    let started = std::time::Instant::now();
    // An editor that answered with its size is open, even if it closes before the next look.
    let mut seen_open = opened.get("width").is_some();
    let mut close_asked = false;
    loop {
        match hosted.poll() {
            Look::Running => {}
            // Loading again with its track's clips; its editor opens again when it runs.
            Look::Starting => {
                std::thread::sleep(EDIT_POLL);
                continue;
            }
            Look::Stopped(why) => return Err(format!("the plug-in stopped while its editor was open ({why})")),
        }
        let open = hosted.editor_open();
        seen_open |= open;
        // Closed by the person, or by `--close-after`.
        if !open && seen_open {
            break;
        }
        if !seen_open && started.elapsed() >= suhina_session::START_TIMEOUT {
            return Err("the plug-in's editor did not open".into());
        }
        if !close_asked && close_after.is_some_and(|after| started.elapsed() >= after) {
            hosted.execute("device.closeEditor")?;
            close_asked = true;
        }
        std::thread::sleep(EDIT_POLL);
    }
    if let Some(file) = save {
        // Once more, for a crash that came as the window closed.
        if let Look::Stopped(why) = hosted.poll() {
            return Err(format!("the plug-in stopped while its editor was open ({why})"));
        }
        let key = hosted.device.key;
        let state = hosted.device.session.rack_mut().state_now(key).map_err(|e| e.message().to_owned())?;
        write_state(&file, &state)?;
    }
    Ok(())
}

/// `run-device`: `stereo` and `events` through the device's plug-in, started as the session
/// starts it for a project's offline render (the same installed plug-in, saved state, process
/// group and clips), or with `state` in place of the saved one. Unlike a render it gets no
/// song position, tempo or transport, and none of the project's automation.
fn render_device(
    device: &ProjectDevice,
    env: &suhina_session::PluginEnv,
    sample_rate: u32,
    state: Option<suhina_plugins::SavedState>,
    stereo: &[f32],
    events: &[suhina_plugins::Event],
    tail_frames: usize,
) -> Result<suhina_plugins::offline::Rendered, String> {
    let project = device.session.project();
    let mut started =
        suhina_session::start_offline_device(project, device.session.audio(), env, device.key, sample_rate, state)
            .map_err(|e| e.to_string())?
            .ok_or("no installed plug-in stands for the device; `suhina-cli plugins check` says why")?;
    let rendered =
        suhina_plugins::offline::render_started(&mut started.control, &mut started.audio, stereo, events, tail_frames)
            .map_err(|e| e.to_string())?;
    Ok(suhina_plugins::offline::Rendered { state_refused: started.refused, ..rendered })
}

/// Starts the plug-in `spec` with `state`, opens its editor and waits until its window is
/// closed (or `--close-after` seconds pass); with `--save <file>`, writes the state it has then
/// (a VST3 controller's own state next to it, in `<file>.controller`).
pub(crate) fn edit_plugin<'a>(
    library: &suhina_plugins::library::Library,
    spec: String,
    state: Option<suhina_plugins::SavedState>,
    words: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let (save, close_after) = edit_options(words)?;
    let launch = suhina_plugins::Launch {
        host: library.host().to_path_buf(),
        plugin: spec,
        channels: 2,
        max_frames: suhina_plugins::offline::BLOCK_FRAMES,
        sample_rate: 48_000.0,
        timeout: std::time::Duration::from_secs(30),
        group: None,
        ara: None,
    };
    let (mut plugin, _audio) = suhina_plugins::Plugin::start(launch).map_err(|e| e.to_string())?;
    if let Some(state) = state
        && let Err(e) = plugin.load_state(state)
    {
        eprintln!("warning: the plug-in did not take the saved settings ({e}); it opens with its defaults");
    }
    let (width, height) = plugin.open_editor().map_err(|e| e.to_string())?;
    eprintln!("editor open ({width} × {height}); close its window to finish");
    let started = std::time::Instant::now();
    while plugin.editor_open() {
        if plugin.health() != suhina_plugins::Health::Running {
            return Err("the plug-in stopped while its editor was open".into());
        }
        if close_after.is_some_and(|after| started.elapsed() >= after) {
            plugin.close_editor().map_err(|e| e.to_string())?;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if let Some(file) = save {
        let state = plugin.save_state().map_err(|e| e.to_string())?;
        write_state(&file, &state)?;
    }
    Ok(())
}

/// `edit`'s and `edit-device`'s options: `--save <file>` and `--close-after <seconds>`.
fn edit_options<'a>(
    mut words: impl Iterator<Item = &'a str>,
) -> Result<(Option<String>, Option<std::time::Duration>), String> {
    let mut save: Option<String> = None;
    let mut close_after: Option<std::time::Duration> = None;
    while let Some(word) = words.next() {
        match word {
            "--save" => save = Some(words.next().ok_or("--save needs a file")?.to_string()),
            "--close-after" => {
                let seconds = words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|s| s.is_finite() && (0.0..=86_400.0).contains(s))
                    .ok_or("--close-after needs seconds")?;
                close_after = Some(std::time::Duration::from_secs_f64(seconds));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((save, close_after))
}

/// Writes a plug-in's state to `file` (a VST3 controller's own state next to it, in
/// `<file>.controller`).
fn write_state(file: &str, state: &suhina_plugins::SavedState) -> Result<(), String> {
    std::fs::write(file, &state.data).map_err(|e| format!("{file}: {e}"))?;
    if let Some(controller) = &state.controller {
        let other = format!("{file}.controller");
        std::fs::write(&other, controller).map_err(|e| format!("{other}: {e}"))?;
    }
    eprintln!("saved the plug-in's state to {file} ({} bytes)", state.data.len());
    Ok(())
}

/// Starts the plug-in `spec`, gives it `state`, and prints its parameters as JSON.
pub(crate) fn print_params(
    library: &suhina_plugins::library::Library,
    spec: String,
    state: Option<suhina_plugins::SavedState>,
) -> Result<(), String> {
    let launch = suhina_plugins::Launch {
        host: library.host().to_path_buf(),
        plugin: spec,
        channels: 2,
        max_frames: suhina_plugins::offline::BLOCK_FRAMES,
        sample_rate: 48_000.0,
        timeout: std::time::Duration::from_secs(30),
        group: None,
        ara: None,
    };
    let (mut plugin, _audio) = suhina_plugins::Plugin::start(launch).map_err(|e| e.to_string())?;
    if let Some(state) = state
        && let Err(e) = plugin.load_state(state)
    {
        eprintln!("warning: the plug-in did not take the saved settings ({e}); these are its defaults");
    }
    let params = plugin.params().map_err(|e| e.to_string())?;
    let list: Vec<serde_json::Value> = params
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "name": p.name, "group": p.group, "value": p.value, "text": p.text,
                "min": p.min, "max": p.max, "default": p.default, "steps": p.steps,
                "automatable": p.automatable, "read_only": p.read_only, "hidden": p.hidden,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?);
    Ok(())
}
