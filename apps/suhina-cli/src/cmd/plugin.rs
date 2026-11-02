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
    // For a device: the project and the device's channel, to read its track's notes.
    let mut device_source: Option<(suhina_session::Session, ChannelId)> = None;
    let (spec, mut state, input, output) = match words.next() {
        Some("run") => {
            let (Some(name), Some(input), Some(output)) = (words.next(), words.next(), words.next()) else {
                return Err(usage());
            };
            (plugin_spec(&mut library, name)?, None, input, output)
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
            let (_, _, spec, state) = project_device(&mut library, project, device)?;
            return edit_plugin(&library, spec, state, words);
        }
        Some("params-device") => {
            let (Some(project), Some(device)) = (words.next(), words.next()) else { return Err(usage()) };
            let (_, _, spec, state) = project_device(&mut library, project, device)?;
            return print_params(&library, spec, state);
        }
        Some("run-device") => {
            let (Some(project), Some(device), Some(input), Some(output)) =
                (words.next(), words.next(), words.next(), words.next())
            else {
                return Err(usage());
            };
            let (session, channel_id, spec, state) = project_device(&mut library, project, device)?;
            device_source = Some((session, channel_id));
            (spec, state, input, output)
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
    // An instrument's notes: its track's arrangement, from the bar asked for or the first clip.
    let events = match &device_source {
        Some((session, channel)) => {
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
        None => Vec::new(),
    };
    let started = std::time::Instant::now();
    let rendered = suhina_plugins::offline::render_with_events(launch, state, &stereo, &events, tail_frames)
        .map_err(|e| e.to_string())?;
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
    if ["clap:", "vst3:", "test:"].iter().any(|p| name.starts_with(p)) {
        return Ok(name.to_string());
    }
    let found = library.catalog().find(name).ok_or_else(|| {
        format!("no plug-in {name:?} in the catalog; `suhina-cli plugins scan` looks for installed ones")
    })?;
    eprintln!("{} by {} ({})", found.name, found.vendor, found.path.display());
    Ok(found.spec())
}

/// A project's plug-in device by name: the opened project, the device's channel, the installed
/// plug-in found for it and the state the project saved.
pub(crate) fn project_device(
    library: &mut suhina_plugins::library::Library,
    project: &str,
    device: &str,
) -> Result<(suhina_session::Session, ChannelId, String, Option<suhina_plugins::SavedState>), String> {
    let (session, _) = suhina_session::Session::open(Path::new(project)).map_err(|e| e.message().to_owned())?;
    let (channel, channel_id, saved) = session
        .project()
        .channels
        .iter()
        .find_map(|c| c.devices.iter().find(|d| d.name == device).map(|d| (c.name.clone(), c.id, d.clone())))
        .ok_or_else(|| format!("no device {device:?} in {project}; `suhina-cli plugins check` lists them"))?;
    let (found, state) = suhina_session::device_plugin(library.catalog(), &saved)
        .ok_or_else(|| format!("no installed plug-in stands for {device:?}; `suhina-cli plugins check` says why"))?;
    eprintln!(
        "{device} on {channel}: {} by {} ({}), found by {}{}",
        found.plugin.name,
        found.plugin.vendor,
        found.plugin.path.display(),
        found.how.name(),
        if found.how.keeps_state() { "" } else { "; it may not read the saved settings" }
    );
    let spec = found.plugin.spec();
    Ok((session, channel_id, spec, state))
}

/// Starts the plug-in `spec` with `state`, opens its editor and waits until its window is
/// closed (or `--close-after` seconds pass); with `--save <file>`, writes the state it has then
/// (a VST3 controller's own state next to it, in `<file>.controller`).
pub(crate) fn edit_plugin<'a>(
    library: &suhina_plugins::library::Library,
    spec: String,
    state: Option<suhina_plugins::SavedState>,
    mut words: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
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
        std::fs::write(&file, &state.data).map_err(|e| format!("{file}: {e}"))?;
        if let Some(controller) = &state.controller {
            let other = format!("{file}.controller");
            std::fs::write(&other, controller).map_err(|e| format!("{other}: {e}"))?;
        }
        eprintln!("saved the plug-in's state to {file} ({} bytes)", state.data.len());
    }
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
