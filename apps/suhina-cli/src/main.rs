// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless Suhina CLI.

mod cmd;
mod play;
mod track_notes;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cmd::audio::audio_devices;
use cmd::mcp::mcp;
use cmd::plugin::{plugin, plugins};
use cmd::theme::theme;

use suhina_automation::{SuhinaMcp, Workspace, render_wavs, scene_as_arrangement};
use suhina_engine::AudioPool;
use suhina_engine::model::*;

const USAGE: &str = "\
usage: suhina-cli render (<project.als> | --demo) -o <out.wav> [--cue <cue.wav>] [--scene <n>] [--sample-rate <hz>]
                         [--source <track or group>] [<fm>]
       suhina-cli play (<project.als> | --demo) [--scene <n> | --launcher] [<fm>] [<output>]
       suhina-cli fm-synth voices <file.syx>
       suhina-cli theme (pack <theme-folder> [-o <out.theme.suhina>] | validate (<file.theme.suhina> | <folder>))
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
  render    render a project offline to a 32-bit float stereo WAV file; --source renders one
            track or group (its id or name) through project.export instead — the sum at that
            node, with the file's extension choosing the format there (wav, flac, ogg or mp3)
  play      play a project once on the default audio output, or with --launcher launch its
            clips and scenes by typing commands
  commands  list every command the MCP server and the UI run, with what it does; --markdown
            prints the table in docs/commands.md
  mcp       serve the Model Context Protocol on stdin/stdout for agents; files can be read only
            below --read-root and written only below --write-root (no root, no access); with
            --audio, opened projects play on the default output so agents can launch clips,
            and audio.setOutput chooses another; with --attach, the agent works on the
            projects open in the running suhina app of this build instead, once the app
            lets agents in (Settings › Agents and updates) and the person allows the connection
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
  theme     theme pack: pack a theme's folder (its theme.css and files) into a theme package
            with a manifest of each file's SHA-256 sum, after checking it as an import does;
            the package is <folder name>.theme.suhina beside the folder unless -o names it;
            theme validate: check a package or a folder and list what the checks found
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
    /// `render --source`: one node of the track tree instead of the whole mix.
    node: Option<String>,
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
                    let (project, audio, assets) = load_project(&a)?;
                    if a.launcher {
                        play::launcher(project, &audio, &assets, &a.audio_output)
                    } else {
                        play::play(&project, &audio, &assets, &a.audio_output)
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
        Some(name) if let Some(run) = subcommand(name) => match run(args.get(1..).unwrap_or_default()) {
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

/// A subcommand run with the arguments after its name.
type Subcommand = fn(&[String]) -> Result<(), String>;

/// The subcommands that take their arguments as given, by name.
fn subcommand(name: &str) -> Option<Subcommand> {
    Some(match name {
        "fm-synth" => fm_synth,
        "theme" => theme,
        "plugin" => plugin,
        "audio" => audio_devices,
        "plugins" => plugins,
        "commands" => list_commands,
        "mcp" => mcp,
        _ => return None,
    })
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
    let mut node = None;
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
            "--source" => {
                node = Some(it.next().ok_or("--source needs a track or group name")?.clone());
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
                audio_output = audio_output
                    .with_params(&serde_json::json!({ "bufferFrames": frames }))
                    .map_err(|e| e.message().to_owned())?;
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
    if node.is_some() && command != Command::Render {
        return Err(format!("--source works with render\n\n{USAGE}"));
    }
    if node.is_some() && cue.is_some() {
        return Err(format!("a cue goes with the whole mix; --source renders one node of the track tree\n\n{USAGE}"));
    }
    if launcher && (command != Command::Play || scene.is_some()) {
        return Err(format!("--launcher works with play, without --scene\n\n{USAGE}"));
    }
    Ok(Args {
        source: source.ok_or(format!("missing project: give a .suhina or .als file, or --demo\n\n{USAGE}"))?,
        output,
        cue,
        node,
        scene,
        launcher,
        sample_rate,
        fm,
        audio_output,
    })
}

/// Opens the project named by the arguments and the audio files it uses, printing warnings, and
/// applies `--scene`.
fn load_project(args: &Args) -> Result<(Project, AudioPool, suhina_engine::DeviceAssets), String> {
    let (project, audio, assets) = match &args.source {
        Source::Demo => (demo_project(), AudioPool::new(), suhina_engine::DeviceAssets::new()),
        Source::Als(path) => {
            // A Suhina project (.suhina) or a Live Set to import.
            let (session, warnings) = suhina_session::Session::open(path).map_err(|e| e.message().to_owned())?;
            let folder = path.parent().unwrap_or(std::path::Path::new("."));
            let started = std::time::Instant::now();
            let (audio, audio_warnings) = suhina_media::load_project_audio(session.project(), folder);
            for warning in warnings.iter().chain(&audio_warnings) {
                eprintln!("warning: {warning}");
            }
            if !audio.files.is_empty() {
                eprintln!("audio: {} files decoded in {:.1} s", audio.files.len(), started.elapsed().as_secs_f64());
            }
            (session.project().clone(), audio, session.device_assets().clone())
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
    Ok((project, audio, assets))
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

/// Puts the FM synth with `patch` on the channel of every track that plays notes, first after
/// its MIDI effects, in place of an instrument already there.
fn put_fm_synth_on_note_tracks(project: &mut Project, patch: &suhina_device_fm_synth::Patch) {
    use suhina_device_api::{Device as _, Role};
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
        clip_host: false,
        extensions: Extensions::default(),
    };
    let channels: Vec<ChannelId> =
        project.tracks.iter().filter(|t| t.content.notes).filter_map(|t| t.channel).collect();
    let role = |d: &Device| match &d.kind {
        DeviceKind::Builtin(id) => suhina_device_registry::info(id).map(|info| info.role),
        _ => None,
    };
    for channel in project.channels.iter_mut().filter(|c| channels.contains(&c.id)) {
        // The synth goes after the MIDI effects that lead the chain, in place of a built-in
        // instrument there.
        let at = channel.devices.iter().take_while(|d| role(d) == Some(Role::MidiEffect)).count();
        if channel.devices.get(at).is_some_and(|d| role(d) == Some(Role::Instrument)) {
            channel.devices.remove(at);
        }
        channel.devices.insert(at, device.clone());
    }
}

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
    let (project, audio, assets) = load_project(args)?;
    let mut library = suhina_plugins::library::Library::default();
    let env = suhina_session::PluginEnv::from_library(&mut library);
    if let Some(node) = &args.node {
        // The node render `project.export` runs: the export's own writer and rules, the file
        // name's extension choosing the format (wav, flac, ogg or mp3). An mp3's title is the
        // project file's stem, as the app writes it.
        let title = match &args.source {
            Source::Als(path) => path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            Source::Demo => String::new(),
        };
        let data = suhina_session::ExportData {
            warnings: Vec::new(),
            project,
            audio,
            assets,
            plugins: Some(std::sync::Arc::new(env)),
            folder: None,
            trusted: suhina_media::Trusted::default(),
            title,
        };
        let params = serde_json::json!({
            "path": path.display().to_string(),
            "source": node,
            "sampleRate": args.sample_rate,
            // An explicit -o is replaced like the mix render's is (its WriteMode::Replace):
            // the same caller's render of the same command writes over what it wrote before.
            "overwrite": true,
        });
        let summary = suhina_session::export_from(&data, &params).map_err(|e| e.message().to_owned())?;
        println!(
            "wrote {} ({:.2} s, {:.0} Hz, {})",
            summary["path"].as_str().unwrap_or_default(),
            summary["seconds"].as_f64().unwrap_or_default(),
            summary["sample_rate"].as_f64().unwrap_or_default(),
            summary["format"].as_str().unwrap_or_default()
        );
        for warning in summary["warnings"].as_array().into_iter().flatten() {
            if let Some(text) = warning.as_str() {
                eprintln!("warning: {text}");
            }
        }
        return Ok(());
    }
    let summary = render_wavs(&project, &audio, &assets, path, args.cue.as_deref(), args.sample_rate, Some(&env))?;
    for notice in &summary.plugins {
        eprintln!("plug-ins: {notice}");
    }
    println!("wrote {} ({:.2} s, {} Hz)", path.display(), summary.seconds, summary.sample_rate);
    if let Some(cue) = &args.cue {
        println!("wrote the cue to {}", cue.display());
    }
    Ok(())
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
        color: None,
        ara: false,
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
            takes: Vec::new(),
        },
        ..Project::default()
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
