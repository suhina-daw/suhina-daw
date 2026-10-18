// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless Suhina CLI.

mod play;
mod track_notes;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use suhina_automation::{SuhinaMcp, Workspace, render_wavs, scene_as_arrangement};
use suhina_engine::AudioPool;
use suhina_engine::model::*;

const USAGE: &str = "\
usage: suhina-cli render (<project.als> | --demo) -o <out.wav> [--cue <cue.wav>] [--scene <n>] [--sample-rate <hz>]
                         [<fm>]
       suhina-cli play (<project.als> | --demo) [--scene <n> | --launcher] [<fm>] [<output>]
       suhina-cli fm-synth voices <file.syx>
       suhina-cli commands [--markdown]
       suhina-cli mcp [--read-root <dir>] [--write-root <dir>] [--audio]
       suhina-cli mcp --attach
       suhina-cli audio devices
       suhina-cli plugins (scan [--all] | scan-file <file> | list [--ignored] | folders
                             | add-folder <dir> [--format (any | vst3 | clap)] | remove-folder <dir>
                             | (folder-on | folder-off) <dir> | (enable | disable) <plug-in>
                             | check <project.als> | set-hosting <plug-in> (binary | vendor | instance))
       suhina-cli plugin run <plug-in> (<in-audio> | -) <out.wav> [--tail <seconds>] [--state <file>]
       suhina-cli plugin run-device <project.als> <device> (<in-audio> | -) <out.wav> [--tail <seconds>]
                                     [--from <bar>] [--seconds <seconds>]
       suhina-cli plugin (params <plug-in> | params-device <project.als> <device>)
       suhina-cli plugin (edit <plug-in> | edit-device <project.als> <device>) [--save <file>]

commands:
  render    render a project offline to a 32-bit float stereo WAV file
  play      play a project once on the default audio output, or with --launcher launch its
            clips and scenes by typing commands
  commands  list every command the MCP server and the UI run, with what it does; --markdown
            prints the table in docs/commands.md
  mcp       serve the Model Context Protocol on stdin/stdout for agents; files can be read only
            below --read-root and written only below --write-root (no root, no access); with
            --audio, opened projects play on the default output so agents can launch clips,
            and audio.setOutput chooses another; with --attach, the agent works on the
            projects open in the running suhina app of this build instead, once the app
            lets agents in (Settings › General) and the person allows the connection
  audio     audio devices: list the audio hosts (WASAPI, ASIO, ALSA...) and each one's output
            devices with their sample rates and buffer sizes
  plugins   find installed plug-ins: scan looks in the standard folders and the added ones
            (new and changed files and failed ones again, or every file with --all; scan-file
            reads one file again), list shows what was found, what failed and what is built for
            another processor (--ignored: also the files that hold no plug-ins), folders shows
            where scans look, add-folder, remove-folder, folder-on and folder-off change that;
            enable and disable switch a plug-in on or off (off: not offered, not loaded); check
            lists a project's plug-ins and the installed plug-in found for each, or none;
            set-hosting chooses whether a plug-in's instances share a process per file (the
            default), per vendor, or each run in one of their own
  fm-synth  fm-synth voices: list the voices in a DX7 voice or bank file (.syx)
  plugin    plugin run: put an audio file through a plug-in (a name or id from plugins list)
            in its own process, offline, and write a 32-bit float stereo WAV; --tail adds
            that many seconds of silence for reverbs and delays to ring out (default 2);
            plugin run-device does the same with a project's plug-in device (by its name) and
            the settings the project saved, through the installed plug-in plugins check finds;
            an instrument plays its track's arrangement notes from --from (a bar, default the
            first clip) for --seconds (default 16), and `-` as input means silence; --state
            gives the plug-in a saved state from a file first (its own, or a VST2 .fxb/.fxp);
            plugin params lists a plug-in's parameters with their values, params-device with
            the settings a project saved; plugin edit opens a plug-in's editor and waits until
            its window is closed, then --save writes the state it has to a file

options:
  <project.als>      a Live Set to import, or a Suhina project (.suhina) to open
  --demo             use the built-in demo project instead
  -o, --output       output WAV path (render only)
  --cue <cue.wav>    render only: also write the cue (the tracks whose cue is on, after their
                     own volume and pan) to this WAV file, as long as the main one
  --scene <n>        use scene n (1-based) of the clip launcher instead of the arrangement
  --launcher         play only: start stopped and read launcher commands (scene 2, clip Bass 1,
                     stop Bass, quit, help) from standard input
  --sample-rate      sample rate in Hz (render: default 48000; play: the device's when left out)
  <output>           play only: --host <name> --device <name> --buffer <frames> --exclusive: the
                     audio output to play on (from audio devices; the default output when left
                     out); the engine renders in blocks of the buffer size (64 to 4096)
  <fm>               --fm [--voices <file.syx>] [--voice <n>]: play every track's notes on the
                     built-in FM synth, with voice n (from 1, default 1) of a DX7 voice or bank
                     file, or its initial voice";

#[derive(Debug)]
enum Source {
    Demo,
    Als(PathBuf),
}

#[derive(Debug)]
struct Args {
    source: Source,
    output: Option<PathBuf>,
    /// `render --cue`: where the cue bus goes.
    cue: Option<PathBuf>,
    scene: Option<usize>,
    launcher: bool,
    sample_rate: u32,
    fm: Option<FmChoice>,
    /// The audio output `play` uses.
    audio_output: suhina_session::OutputChoice,
}

/// `--fm`, with the voice to load.
#[derive(Debug, Default)]
struct FmChoice {
    voices: Option<PathBuf>,
    /// From 0.
    voice: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Render,
    Play,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Settings kept under the program's old name are copied to the new one on the first start.
    let old = suhina_plugins::library::old_settings_folder();
    if let Some(warning) =
        suhina_plugins::library::adopt_old_folder(old, suhina_plugins::library::new_settings_folder())
    {
        eprintln!("warning: {warning}");
    }
    match args.first().map(String::as_str) {
        Some(name @ ("render" | "play")) => {
            let command = if name == "render" { Command::Render } else { Command::Play };
            let result = parse_args(args.get(1..).unwrap_or_default(), command).and_then(|a| match command {
                Command::Render => render(&a),
                Command::Play => {
                    let (project, audio) = load_project(&a)?;
                    if a.launcher {
                        play::launcher(project, &audio, &a.audio_output)
                    } else {
                        play::play(&project, &audio, &a.audio_output)
                    }
                }
            });
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("error: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("fm-synth") => match fm_synth(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("plugin") => match plugin(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("audio") => match audio_devices(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("plugins") => match plugins(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("commands") => match list_commands(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("mcp") => match mcp(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("--version" | "-V") => {
            println!("suhina-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `suhina-cli commands [--markdown]`.
fn list_commands(args: &[String]) -> Result<(), String> {
    match args {
        [] => {
            for command in suhina_session::all_commands().as_array().into_iter().flatten() {
                let id = command["id"].as_str().unwrap_or_default();
                let label = command["label"].as_str().unwrap_or_default();
                println!("{id:<24} {label}");
            }
            Ok(())
        }
        [flag] if flag == "--markdown" => {
            print!("{}", suhina_session::commands_markdown());
            Ok(())
        }
        _ => Err("usage: suhina-cli commands [--markdown]".into()),
    }
}

fn parse_args(args: &[String], command: Command) -> Result<Args, String> {
    let mut output = None;
    let mut cue = None;
    let mut sample_rate = 48_000;
    let mut scene = None;
    let mut launcher = false;
    let mut source = None;
    let mut fm: Option<FmChoice> = None;
    let mut audio_output = suhina_session::OutputChoice::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let next = match arg.as_str() {
            "--demo" => Some(Source::Demo),
            "-o" | "--output" => {
                output = Some(PathBuf::from(it.next().ok_or("--output needs a path")?));
                None
            }
            "--cue" => {
                cue = Some(PathBuf::from(it.next().ok_or("--cue needs a path")?));
                None
            }
            "--sample-rate" => {
                let value = it.next().ok_or("--sample-rate needs a value")?;
                sample_rate = value.parse().map_err(|_| format!("invalid sample rate: {value}"))?;
                audio_output.sample_rate = Some(sample_rate);
                None
            }
            "--host" => {
                audio_output.host = Some(it.next().ok_or("--host needs a name, from audio devices")?.clone());
                None
            }
            "--device" => {
                audio_output.device = Some(it.next().ok_or("--device needs a name, from audio devices")?.clone());
                None
            }
            "--buffer" => {
                let value = it.next().ok_or("--buffer needs a number of frames")?;
                let frames: u32 = value.parse().map_err(|_| format!("invalid buffer size: {value}"))?;
                audio_output =
                    audio_output.with_params(&serde_json::json!({ "buffer_frames": frames })).map_err(|e| e.0)?;
                None
            }
            "--exclusive" => {
                audio_output.exclusive = true;
                None
            }
            "--scene" => {
                let value = it.next().ok_or("--scene needs a number")?;
                let n: usize = value.parse().map_err(|_| format!("invalid scene number: {value}"))?;
                scene = Some(n.checked_sub(1).ok_or("scenes are numbered from 1")?);
                None
            }
            "--launcher" => {
                launcher = true;
                None
            }
            "--fm" => {
                fm.get_or_insert_with(FmChoice::default);
                None
            }
            "--voices" => {
                fm.get_or_insert_with(FmChoice::default).voices =
                    Some(PathBuf::from(it.next().ok_or("--voices needs a .syx file")?));
                None
            }
            "--voice" => {
                let value = it.next().ok_or("--voice needs a number")?;
                let n: usize = value.parse().map_err(|_| format!("invalid voice number: {value}"))?;
                fm.get_or_insert_with(FmChoice::default).voice =
                    n.checked_sub(1).ok_or("voices are numbered from 1")?;
                None
            }
            other if other.starts_with('-') => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
            path => Some(Source::Als(PathBuf::from(path))),
        };
        if let Some(next) = next {
            if source.is_some() {
                return Err(format!("give one project, either a file or --demo\n\n{USAGE}"));
            }
            source = Some(next);
        }
    }
    match (command, &output) {
        (Command::Render, None) => return Err(format!("missing --output\n\n{USAGE}")),
        (Command::Play, Some(_)) => return Err(format!("play does not write a file; use render\n\n{USAGE}")),
        _ => {}
    }
    if cue.is_some() && command != Command::Render {
        return Err(format!("--cue works with render\n\n{USAGE}"));
    }
    if launcher && (command != Command::Play || scene.is_some()) {
        return Err(format!("--launcher works with play, without --scene\n\n{USAGE}"));
    }
    Ok(Args {
        source: source.ok_or(format!("missing project: give a .suhina or .als file, or --demo\n\n{USAGE}"))?,
        output,
        cue,
        scene,
        launcher,
        sample_rate,
        fm,
        audio_output,
    })
}

/// Opens the project named by the arguments and the audio files it uses, printing warnings, and
/// applies `--scene`.
fn load_project(args: &Args) -> Result<(Project, AudioPool), String> {
    let (project, audio) = match &args.source {
        Source::Demo => (demo_project(), AudioPool::new()),
        Source::Als(path) => {
            // A Suhina project (.suhina) or a Live Set to import.
            let (session, warnings) = suhina_session::Session::open(path).map_err(|e| e.0)?;
            let folder = path.parent().unwrap_or(std::path::Path::new("."));
            let started = std::time::Instant::now();
            let (audio, audio_warnings) = suhina_media::load_project_audio(session.project(), folder);
            for warning in warnings.iter().chain(&audio_warnings) {
                eprintln!("warning: {warning}");
            }
            if !audio.files.is_empty() {
                eprintln!("audio: {} files decoded in {:.1} s", audio.files.len(), started.elapsed().as_secs_f64());
            }
            (session.project().clone(), audio)
        }
    };
    let mut project = match args.scene {
        Some(scene) => scene_as_arrangement(&project, scene)?,
        None => project,
    };
    if let Some(choice) = &args.fm {
        let patch = load_voice(choice)?;
        eprintln!("FM synth: {}", if patch.name().is_empty() { "(no name)".to_string() } else { patch.name() });
        put_fm_synth_on_note_tracks(&mut project, &patch);
    }
    Ok((project, audio))
}

/// The voice `--fm` asks for: from a DX7 voice or bank file, or the initial voice.
fn load_voice(choice: &FmChoice) -> Result<suhina_device_fm_synth::Patch, String> {
    let Some(path) = &choice.voices else { return Ok(suhina_device_fm_synth::Patch::default()) };
    let voices = read_voices(path)?;
    let count = voices.patches.len();
    voices
        .patches
        .get(choice.voice)
        .copied()
        .ok_or_else(|| format!("{} has {count} voices; --voice must be from 1 to {count}", path.display()))
}

fn read_voices(path: &std::path::Path) -> Result<suhina_device_fm_synth::patch::Voices, String> {
    const MAX_FILE: u64 = 1 << 20;
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_FILE {
        return Err(format!("{} is too large for DX7 voice data", path.display()));
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let voices = suhina_device_fm_synth::patch::read(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    if !voices.checksum_ok {
        eprintln!("warning: {}: the checksum does not match; the voices are read anyway", path.display());
    }
    Ok(voices)
}

/// Puts the FM synth with `patch` first on the channel of every track that plays notes, in
/// place of an instrument already there.
fn put_fm_synth_on_note_tracks(project: &mut Project, patch: &suhina_device_fm_synth::Patch) {
    use suhina_device_api::Device as _;
    let mut synth = suhina_device_fm_synth::FmSynth::new();
    synth.load_patch(patch);
    let params: Vec<DeviceParam> = suhina_device_fm_synth::INFO
        .params
        .iter()
        .enumerate()
        .filter_map(|(i, p)| synth.param(i).map(|v| DeviceParam::new(p.id, v)))
        .collect();
    let device = Device {
        name: suhina_device_fm_synth::INFO.name.into(),
        kind: DeviceKind::Builtin(suhina_device_fm_synth::INFO.id.into()),
        enabled: true,
        state: None,
        params,
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    let channels: Vec<ChannelId> =
        project.tracks.iter().filter(|t| t.content.notes).filter_map(|t| t.channel).collect();
    for channel in project.channels.iter_mut().filter(|c| channels.contains(&c.id)) {
        let first_is_instrument = channel.devices.first().is_some_and(|d| {
            matches!(&d.kind, DeviceKind::Builtin(id) if suhina_device_registry::info(id)
                .is_some_and(|i| i.role == suhina_device_api::Role::Instrument))
        });
        if first_is_instrument {
            channel.devices.remove(0);
        }
        channel.devices.insert(0, device.clone());
    }
}

/// `fm-synth voices <file.syx>`.
fn fm_synth(args: &[String]) -> Result<(), String> {
    let (Some("voices"), Some(path)) = (args.first().map(String::as_str), args.get(1)) else {
        return Err(format!("usage: suhina-cli fm-synth voices <file.syx>\n\n{USAGE}"));
    };
    let voices = read_voices(std::path::Path::new(path))?;
    for (i, patch) in voices.patches.iter().enumerate() {
        println!("{:>2}  {}", i + 1, patch.name());
    }
    Ok(())
}

fn render(args: &Args) -> Result<(), String> {
    let path = args.output.as_ref().ok_or("missing --output")?;
    let (project, audio) = load_project(args)?;
    let mut library = suhina_plugins::library::Library::default();
    let env = suhina_session::PluginEnv::from_library(&mut library);
    let summary = render_wavs(&project, &audio, path, args.cue.as_deref(), args.sample_rate, Some(&env))?;
    for notice in &summary.plugins {
        eprintln!("plug-ins: {notice}");
    }
    println!("wrote {} ({:.2} s, {} Hz)", path.display(), summary.seconds, summary.sample_rate);
    if let Some(cue) = &args.cue {
        println!("wrote the cue to {}", cue.display());
    }
    Ok(())
}

/// `plugin run …` and `plugin run-device …`: audio, and an instrument's notes, through one
/// plug-in offline, written to a WAV file.
fn plugin(args: &[String]) -> Result<(), String> {
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

/// `plugins …`: runs the matching `plugins.*` command and prints its result.
/// `audio devices`: runs `audio.devices` on the system's devices.
fn audio_devices(args: &[String]) -> Result<(), String> {
    if args.iter().map(String::as_str).ne(["devices"]) {
        return Err(format!("unknown audio command\n\n{USAGE}"));
    }
    let mut projects = suhina_session::Projects::new();
    projects.set_audio_system(std::sync::Arc::new(suhina_output::DeviceOutput::new()));
    let result = projects.execute("audio.devices", &serde_json::json!({})).map_err(|e| e.0)?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

fn plugins(args: &[String]) -> Result<(), String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut projects = suhina_session::Projects::new();
    let (id, params) = match words.as_slice() {
        ["check", project] => {
            let (session, warnings) = suhina_session::Session::open(Path::new(project)).map_err(|e| e.0)?;
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            projects.add(session, None).map_err(|e| e.0)?;
            ("plugins.check", serde_json::json!({}))
        }
        ["scan"] => ("plugins.scan", serde_json::json!({})),
        ["scan", "--all"] => ("plugins.scan", serde_json::json!({ "all": true })),
        ["list"] => ("plugins.list", serde_json::json!({})),
        ["list", "--ignored"] => ("plugins.list", serde_json::json!({ "ignored": true })),
        ["folders"] => ("plugins.folders", serde_json::json!({})),
        ["add-folder", folder] => ("plugins.addFolder", serde_json::json!({ "folder": folder })),
        ["add-folder", folder, "--format", format] => {
            ("plugins.addFolder", serde_json::json!({ "folder": folder, "format": format }))
        }
        ["folder-on", folder] => ("plugins.setFolderEnabled", serde_json::json!({ "folder": folder, "enabled": true })),
        ["folder-off", folder] => {
            ("plugins.setFolderEnabled", serde_json::json!({ "folder": folder, "enabled": false }))
        }
        ["enable", plugin] => ("plugins.setEnabled", serde_json::json!({ "plugin": plugin, "enabled": true })),
        ["disable", plugin] => ("plugins.setEnabled", serde_json::json!({ "plugin": plugin, "enabled": false })),
        ["scan-file", file] => ("plugins.scanFile", serde_json::json!({ "file": file })),
        ["remove-folder", folder] => ("plugins.removeFolder", serde_json::json!({ "folder": folder })),
        ["set-hosting", plugin, hosting] => {
            ("plugins.setHosting", serde_json::json!({ "plugin": plugin, "hosting": hosting }))
        }
        _ => {
            return Err(format!(
                "unknown plugins command

{USAGE}"
            ));
        }
    };
    if id == "plugins.scan" || id == "plugins.scanFile" {
        eprintln!("scanning for plug-ins; each new or changed file is loaded in a process of its own");
    }
    let result = projects.execute(id, &params).map_err(|e| e.0)?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// How the host is asked to load the plug-in `name`: a `clap:`, `vst3:` or `test:` name as it
/// is, else a name or id from the catalog.
fn plugin_spec(library: &mut suhina_plugins::library::Library, name: &str) -> Result<String, String> {
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
fn project_device(
    library: &mut suhina_plugins::library::Library,
    project: &str,
    device: &str,
) -> Result<(suhina_session::Session, ChannelId, String, Option<suhina_plugins::SavedState>), String> {
    let (session, _) = suhina_session::Session::open(Path::new(project)).map_err(|e| e.0)?;
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
fn edit_plugin<'a>(
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
fn print_params(
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

/// Serves MCP on stdin/stdout. Nothing else may be printed to stdout while it runs.
fn mcp(args: &[String]) -> Result<(), String> {
    let mut read_root = None;
    let mut write_root = None;
    let mut audio = false;
    let mut attach = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--read-root" => read_root = Some(PathBuf::from(it.next().ok_or("--read-root needs a folder")?)),
            "--write-root" => write_root = Some(PathBuf::from(it.next().ok_or("--write-root needs a folder")?)),
            "--audio" => audio = true,
            "--attach" => attach = true,
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    if attach {
        if read_root.is_some() || write_root.is_some() || audio {
            return Err("--attach takes no other arguments: the app decides what an agent may read and write, \
                        and plays on its own output"
                .into());
        }
        let folder = suhina_automation::endpoint::build_folder()
            .map_err(|e| format!("could not find this program's folder: {e}"))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("could not start the async runtime: {e}"))?;
        return runtime.block_on(suhina_automation::attach::attach_stdio(&folder));
    }
    let workspace = Workspace::new(read_root.as_deref(), write_root.as_deref())?;
    let library = suhina_library::Library::new(suhina_plugins::library::settings_folder());
    let mut server = SuhinaMcp::new(workspace).with_file_library(library);
    if audio {
        server = server
            .with_audio_output(std::sync::Arc::new(suhina_output::DeviceOutput::new()))
            .with_midi_input(std::sync::Arc::new(suhina_midi::Midi::new()));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the async runtime: {e}"))?;
    runtime.block_on(server.serve_stdio())
}

/// Two bars at 120 bpm: a bass line and a chord pattern on separate tracks, each placed twice.
fn demo_project() -> Project {
    let note = |time: f64, duration: f64, key: u8, velocity: f32| Note {
        time,
        duration,
        key,
        velocity,
        release_velocity: 0.5,
        channel: 0,
        muted: false,
    };
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        midi_input: Default::default(),
        audio_input: Default::default(),
        extensions: Extensions::default(),
    };
    let channel = |id: u32, name: &str, volume: f64, pan: f64| Channel {
        id: ChannelId(id),
        name: name.into(),
        role: ChannelRole::Regular,
        volume,
        pan,
        mute: false,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
        automation: Vec::new(),
    };
    let content = |id: u32, name: &str, notes: Vec<Note>| ClipContent {
        id: ContentId(id),
        name: name.into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
        scale: None,
    };
    let clip = |content: u32, start: f64| Clip {
        id: Default::default(),
        content: ContentId(content),
        start,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        extensions: Extensions::default(),
    };

    let bass = (0..8).map(|i| note(f64::from(i) * 0.5, 0.4, if i % 4 == 3 { 43 } else { 36 }, 0.9)).collect();
    let chords = [0.0, 1.5, 2.5].into_iter().flat_map(|t| [60, 64, 67].map(|k| note(t, 0.75, k, 0.5))).collect();

    Project {
        tracks: vec![track(1, "Bass"), track(2, "Chords")],
        channels: vec![channel(1, "Bass", 0.9, 0.0), channel(2, "Chords", 0.6, 0.3)],
        contents: vec![content(1, "Bass", bass), content(2, "Chords", chords)],
        arrangement: Arrangement {
            lanes: vec![
                ArrangementLane {
                    id: Default::default(),
                    name: "Bass".into(),
                    track: Some(TrackId(1)),
                    clips: vec![clip(1, 0.0), clip(1, 4.0)],
                },
                ArrangementLane {
                    id: Default::default(),
                    name: "Chords".into(),
                    track: Some(TrackId(2)),
                    clips: vec![clip(2, 0.0), clip(2, 4.0)],
                },
            ],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_is_valid_and_renders_two_bars() {
        let project = demo_project();
        assert_eq!(project.validate(), Ok(()));
        let samples = suhina_engine::render_offline(&project, 48_000, 512).unwrap();
        let seconds = samples.len() as f64 / 2.0 / 48_000.0;
        assert!((4.0..4.2).contains(&seconds), "{seconds} s");
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1 && peak <= 1.0, "peak {peak}");
    }

    #[test]
    fn render_args_need_one_project_and_an_output() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parse_render = |a: &[String]| parse_args(a, Command::Render);
        assert!(parse_render(&args(&["-o", "x.wav"])).is_err());
        assert!(parse_render(&args(&["--demo"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o"])).is_err());
        assert!(parse_render(&args(&["--demo", "a.als", "-o", "x.wav"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--sample-rate", "abc"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--scene", "0"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--bogus"])).is_err());
        let ok = parse_render(&args(&["a.als", "-o", "x.wav", "--sample-rate", "44100", "--scene", "2"])).unwrap();
        assert_eq!(ok.sample_rate, 44_100);
        assert_eq!(ok.scene, Some(1));
        assert!(matches!(ok.source, Source::Als(_)));
        assert_eq!(ok.cue, None);
        let cued = parse_render(&args(&["--demo", "-o", "x.wav", "--cue", "cue.wav"])).unwrap();
        assert_eq!(cued.cue, Some(PathBuf::from("cue.wav")));
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--cue"])).is_err());
        assert!(parse_args(&args(&["--demo", "--cue", "cue.wav"]), Command::Play).is_err());
    }

    #[test]
    fn play_args_take_a_project_but_no_output() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse_args(&args(&["--demo"]), Command::Play).is_ok());
        assert!(parse_args(&args(&["a.als", "--scene", "3"]), Command::Play).is_ok());
        assert!(parse_args(&args(&["--demo", "-o", "x.wav"]), Command::Play).is_err());
        assert!(parse_args(&args(&[]), Command::Play).is_err());
        assert!(parse_args(&args(&["a.als", "--launcher"]), Command::Play).unwrap().launcher);
        assert!(parse_args(&args(&["a.als", "--launcher", "--scene", "1"]), Command::Play).is_err());
        assert!(parse_args(&args(&["a.als", "--launcher", "-o", "x.wav"]), Command::Render).is_err());
    }

    #[test]
    fn scene_replaces_the_arrangement_with_its_clips() {
        let mut project = demo_project();
        let clip = project.arrangement.lanes[0].clips[0].clone();
        project.launcher = Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot::default(), Slot { clip: Some(clip), has_stop: true }],
            }],
        };
        let scene_2 = scene_as_arrangement(&project, 1).unwrap();
        assert_eq!(scene_2.arrangement.lanes.len(), 1);
        assert_eq!(scene_2.arrangement.lanes[0].clips.len(), 1);
        assert!(scene_as_arrangement(&project, 0).unwrap().arrangement.lanes[0].clips.is_empty());
        assert!(scene_as_arrangement(&project, 2).is_err());
    }

    #[test]
    fn fm_plays_every_note_track_with_a_voice_from_a_bank() {
        use suhina_device_fm_synth::{FmSynth, Patch, patch};
        // A bank whose second voice is a two-operator sound, written to a file.
        let mut synth = FmSynth::new();
        let index = |id: &str| suhina_device_fm_synth::INFO.param_index(id).unwrap();
        use suhina_device_api::Device as _;
        synth.set_param(index("op2.level"), 80.0);
        synth.set_param(index("op2.coarse"), 3.0);
        let mut voice = synth.patch();
        voice.set_name("TWO OPS");
        let path = std::env::temp_dir().join(format!("suhina-cli-bank-{}.syx", std::process::id()));
        std::fs::write(&path, patch::write_bank(&[Patch::default(), voice])).unwrap();

        let choice = FmChoice { voices: Some(path.clone()), voice: 1 };
        let loaded = load_voice(&choice).unwrap();
        assert_eq!(loaded.name(), "TWO OPS");
        assert!(load_voice(&FmChoice { voices: Some(path.clone()), voice: 40 }).is_err());
        assert!(fm_synth(&["voices".into(), path.display().to_string()]).is_ok());

        let mut project = demo_project();
        put_fm_synth_on_note_tracks(&mut project, &loaded);
        // Twice: the synth replaces the instrument it put there before.
        put_fm_synth_on_note_tracks(&mut project, &loaded);
        assert!(project.channels.iter().all(|c| c.devices.len() == 1));
        let stored = &project.channels[0].devices[0];
        assert_eq!(stored.kind, DeviceKind::Builtin("fm-synth".into()));
        assert!(stored.params.iter().any(|p| p.id == "op2.coarse" && p.value == 3.0));
        let samples = suhina_engine::render_offline(&project, 48_000, 512).unwrap();
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak <= 1.0, "peak {peak}");
        let _ = std::fs::remove_file(path);
    }
}
