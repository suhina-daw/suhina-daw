// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio output a project plays on: which host (WASAPI, ASIO, ALSA…), device, sample rate
//! and buffer size, and the `audio.*` commands that list, choose and report it. The session
//! only describes the choice; the output layer (`suhina-output`) enumerates devices and opens
//! streams through [`AudioSystem`], which the app or the MCP server hands to [`Projects`]. Like
//! the `plugins.*` commands they belong to Suhina, not to one project.

use std::sync::Arc;

use serde_json::{Value, json};
use suhina_engine::{AudioPool, DeviceAssets};
use suhina_model::Project;

use crate::commands::{CommandKind, CommandSpec, ParamType, optional, param};
use crate::{CommandError, Player, Projects};

/// Smallest and largest buffer size, in frames, a choice may ask for; the engine renders in
/// blocks of the chosen size (`docs/architecture.md` › Threads). The output layer fits the
/// choice to what the device declares and asks the device for one of those sizes only: a
/// driver whose sizes all fall outside this span is asked for none and keeps its own
/// (`suhina-output` › `devices`), and WASAPI shared mode, where Windows sets the device's
/// buffer, renders in blocks of the chosen size alone.
pub const MIN_BUFFER_FRAMES: u32 = 64;
pub const MAX_BUFFER_FRAMES: u32 = 4096;
/// Highest sample rate a choice may ask for.
pub const MAX_SAMPLE_RATE: u32 = 384_000;
/// Highest output pair the cue can play on: pair 31 is channels 63 and 64.
pub const MAX_CUE_PAIR: u16 = 31;
/// Loudest cue volume, in dB (the engine's `MAX_CUE_GAIN`).
pub const MAX_CUE_DB: f64 = 12.0;

/// The output the user chose. `None` means the host's or device's own default, so an empty
/// choice plays on the system's default output as it is set up.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputChoice {
    /// A host's name as [`AudioSystem::hosts`] lists it ("WASAPI", "ASIO").
    pub host: Option<String>,
    /// An output device's name on that host.
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    /// Frames per buffer; the engine renders in blocks of this size. The output layer fits it
    /// to the sizes the device allows and asks the device for one of those only, saying so in a
    /// notice when that is not the size asked for; where the device's own buffer is not ours to
    /// set (WASAPI shared mode) it is the engine's block alone.
    pub buffer_frames: Option<u32>,
    /// WASAPI only: take the device for Suhina alone (exclusive mode) when it allows it.
    /// Other programs are not heard on it meanwhile, so it is off unless asked for.
    pub exclusive: bool,
    /// The output pair of the same device the cue (headphones) plays on: pair 1 is channels 3
    /// and 4, pair 2 channels 5 and 6, and so on. `None` plays the cue with the main output on
    /// channels 1 and 2, as a device with two channels must.
    pub cue_pair: Option<u16>,
    /// The cue volume, linear (1.0 is 0 dB), from 0 to the engine's `MAX_CUE_GAIN`.
    pub cue_gain: f32,
    /// The audio input device the tracks' audio inputs hear (ADR 0017): off unless chosen, so
    /// nothing opens a microphone unasked.
    pub input: InputChoice,
}

impl Default for OutputChoice {
    fn default() -> Self {
        Self {
            host: None,
            device: None,
            sample_rate: None,
            buffer_frames: None,
            exclusive: false,
            cue_pair: None,
            cue_gain: 1.0,
            input: InputChoice::Off,
        }
    }
}

/// The channels, counted from 1, of output pair `pair` (1 is channels 3 and 4); the main output
/// is pair 0.
pub fn cue_channels(pair: u16) -> (u32, u32) {
    let first = u32::from(pair) * 2 + 1;
    (first, first + 1)
}

/// A gain in dB, rounded to a hundredth; silence is `None` (JSON null).
fn gain_db(gain: f32) -> Option<f64> {
    (gain > 0.0).then(|| (20.0 * f64::from(gain).log10() * 100.0).round() / 100.0)
}

/// The audio input device chosen, on the output's host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InputChoice {
    /// No input device is opened.
    #[default]
    Off,
    /// The host's default input device.
    Default,
    /// An input device by name.
    Named(String),
}

impl InputChoice {
    /// As `audio.setOutput` takes it and the settings keep it: "off", "default" or a name.
    pub fn as_text(&self) -> &str {
        match self {
            Self::Off => "off",
            Self::Default => "default",
            Self::Named(name) => name,
        }
    }

    /// From "off" (or empty), "default" or a device's name.
    pub fn from_text(text: &str) -> Self {
        match text.trim() {
            "" | "off" => Self::Off,
            "default" => Self::Default,
            name => Self::Named(name.to_string()),
        }
    }
}

impl OutputChoice {
    pub fn to_json(&self) -> Value {
        json!({
            "host": self.host,
            "device": self.device,
            "sample_rate": self.sample_rate,
            "buffer_frames": self.buffer_frames,
            "exclusive": self.exclusive,
            "cue_pair": self.cue_pair,
            "cue_db": gain_db(self.cue_gain),
            "input": self.input.as_text(),
        })
    }

    /// The same device and format as this choice, with the cue's pair and volume of `cue`.
    pub fn with_cue_of(&self, cue: &OutputChoice) -> Self {
        Self { cue_pair: cue.cue_pair, cue_gain: cue.cue_gain, ..self.clone() }
    }

    /// The same device and format without the cue's settings, to compare two choices by the
    /// device they open.
    pub fn device_only(&self) -> Self {
        Self { cue_pair: None, cue_gain: 1.0, ..self.clone() }
    }

    /// This choice with the parts `params` gives changed (`audio.setOutput`): an empty text or
    /// 0 goes back to the default, and a new host forgets the device chosen on the old one.
    pub fn with_params(&self, params: &Value) -> Result<Self, CommandError> {
        let mut next = self.clone();
        let text = |key: &str| -> Result<Option<Option<String>>, CommandError> {
            match params.get(key) {
                None => Ok(None),
                Some(Value::String(s)) if s.trim().is_empty() => Ok(Some(None)),
                Some(Value::String(s)) => Ok(Some(Some(s.trim().to_string()))),
                Some(_) => Err(format!("{key} must be a text").into()),
            }
        };
        let number = |key: &str, max: u32| -> Result<Option<Option<u32>>, CommandError> {
            match params.get(key) {
                None => Ok(None),
                Some(v) => match v.as_u64() {
                    Some(0) => Ok(Some(None)),
                    Some(n) if n <= u64::from(max) => Ok(Some(u32::try_from(n).ok())),
                    _ => Err(format!("{key} must be a whole number from 0 (the default) to {max}").into()),
                },
            }
        };
        if let Some(host) = text("host")? {
            if host != next.host {
                next.device = None;
            }
            next.host = host;
        }
        if let Some(device) = text("device")? {
            next.device = device;
        }
        if let Some(rate) = number("sample_rate", MAX_SAMPLE_RATE)? {
            next.sample_rate = rate;
        }
        if let Some(frames) = number("buffer_frames", MAX_BUFFER_FRAMES)? {
            if frames.is_some_and(|f| f < MIN_BUFFER_FRAMES) {
                return Err(
                    format!("buffer_frames must be from {MIN_BUFFER_FRAMES} to {MAX_BUFFER_FRAMES}, or 0").into()
                );
            }
            next.buffer_frames = frames;
        }
        if let Some(exclusive) = params.get("exclusive") {
            next.exclusive = exclusive.as_bool().ok_or("exclusive must be true or false")?;
        }
        if let Some(pair) = number("cue_pair", u32::from(MAX_CUE_PAIR))? {
            next.cue_pair = pair.and_then(|p| u16::try_from(p).ok());
        }
        if let Some(input) = params.get("input") {
            next.input = InputChoice::from_text(input.as_str().ok_or("input must be a text")?);
        }
        Ok(next)
    }

    /// Whether `hosts` lists the host and device this choice names (the default host when it
    /// names none; any device when it names none). Names compare as the output layer compares
    /// them: without surrounding spaces, ignoring ASCII case.
    pub fn found_in(&self, hosts: &[HostInfo]) -> bool {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        let host = match &self.host {
            Some(name) => hosts.iter().find(|h| same(&h.name, name)),
            None => hosts.iter().find(|h| h.default),
        };
        match (host, &self.device) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(host), Some(device)) => host.devices.iter().any(|d| same(&d.name, device)),
        }
    }

    /// This choice with the cue volume `params.db` gives (`output.setCueVolume`): from silence
    /// (-inf, or below -150) to [`MAX_CUE_DB`].
    pub fn with_cue_volume(&self, params: &Value) -> Result<Self, CommandError> {
        let db = match params.get("db") {
            Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("-inf") => f64::NEG_INFINITY,
            Some(v) => v.as_f64().ok_or("db must be a number, or \"-inf\" for silence")?,
            None => return Err("db must be a number, or \"-inf\" for silence".into()),
        };
        if db.is_nan() || db > MAX_CUE_DB {
            return Err(format!("db must be at most +{MAX_CUE_DB}, got {db}").into());
        }
        let cue_gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) as f32 };
        Ok(Self { cue_gain, ..self.clone() })
    }

    /// Checks that the chosen cue pair is one of `device_channels` (when the device is known):
    /// the main output's channels 1 and 2 are not a cue pair of their own.
    pub fn check_cue_pair(&self, device_channels: Option<u16>) -> Result<(), CommandError> {
        let (Some(pair), Some(channels)) = (self.cue_pair, device_channels) else { return Ok(()) };
        let (_, last) = cue_channels(pair);
        if last > u32::from(channels) {
            let most = channels / 2;
            let offer = if most >= 2 {
                format!("pairs 1 to {} (channels 3/4 to {}/{})", most - 1, most * 2 - 1, most * 2)
            } else {
                "none: the cue plays with the main output (pair 0)".to_string()
            };
            return Err(format!(
                "the device has {channels} output channels, so pair {pair} (channels {}/{last}) is not there; it has {offer}",
                last - 1
            )
            .into());
        }
        Ok(())
    }

    /// The channels the device this choice names has, as `hosts` lists it (the default host
    /// and device when it names none); `None` when it is not listed.
    pub fn device_channels(&self, hosts: &[HostInfo]) -> Option<u16> {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        let host = match &self.host {
            Some(name) => hosts.iter().find(|h| same(&h.name, name)),
            None => hosts.iter().find(|h| h.default),
        }?;
        let device = match &self.device {
            Some(name) => host.devices.iter().find(|d| same(&d.name, name)),
            None => host.devices.iter().find(|d| d.default),
        }?;
        Some(device.channels)
    }
}

/// An audio host on this system and its output devices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostInfo {
    pub name: String,
    /// The one used when no host is chosen.
    pub default: bool,
    pub devices: Vec<DeviceInfo>,
    /// Its input devices (microphones, interfaces' inputs).
    pub inputs: Vec<InputInfo>,
    /// Why the host's devices could not be listed, when they could not.
    pub error: Option<String>,
}

/// An output device and what it can be opened with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    /// The host's default output device.
    pub default: bool,
    pub channels: u16,
    pub sample_rates: Vec<u32>,
    pub default_sample_rate: Option<u32>,
    /// The buffer sizes it can be asked for, smallest first (the driver's allowed values,
    /// within [`MIN_BUFFER_FRAMES`] … [`MAX_BUFFER_FRAMES`]). Empty when the driver declared a
    /// range with no size in that span: it is then asked for no size and keeps its own. On
    /// WASAPI shared mode these are the engine's own sizes, since Windows sets the device's
    /// buffer there.
    pub buffer_sizes: Vec<u32>,
    /// The driver's own range, when it reports one.
    pub buffer_range: Option<(u32, u32)>,
    /// Whether it can be opened in exclusive mode (WASAPI).
    pub exclusive: bool,
}

/// An audio input device and what it can be opened with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputInfo {
    pub name: String,
    /// The host's default input device.
    pub default: bool,
    /// Its input channels; a track's audio input names them from 1.
    pub channels: u16,
    pub sample_rates: Vec<u32>,
}

/// What an open output runs with now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputInfo {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    /// Frames the device asks for per callback, as last measured (0 before the first).
    pub buffer_frames: u32,
    /// Frames the engine renders per block.
    pub engine_block: u32,
    pub exclusive: bool,
    /// Measured time from the engine rendering a frame to the device playing it, in ms.
    pub latency_ms: f64,
    /// A track is armed, so the engine keeps less queued ahead of the device (ADR 0017).
    pub live: bool,
    /// The time from a key pressed on a MIDI controller to the speaker, in ms: the measured
    /// latency and the fixed delay a stamped note plays after (ADR 0017, #269).
    pub live_latency_ms: f64,
    /// Device buffers the engine had not filled in time.
    pub underruns: u64,
    /// The device reported an error (unplugged, taken by another program).
    pub failed: bool,
    /// The chosen host or device was not there when the output opened, so another one (the
    /// default) plays in its place until it is back.
    pub fallback: bool,
    /// The output channels the device is opened with.
    pub channels: u16,
    /// The pair the cue plays on (1 is channels 3 and 4), or `None` when it plays with the main
    /// output.
    pub cue_pair: Option<u16>,
    /// The audio input device open beside it, and its channels (ADR 0017).
    pub input: Option<String>,
    pub input_channels: u16,
    /// Blocks the engine found too little input for.
    pub input_underruns: u64,
}

impl OutputInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "host": self.host,
            "device": self.device,
            "sample_rate": self.sample_rate,
            "buffer_frames": self.buffer_frames,
            "engine_block": self.engine_block,
            "exclusive": self.exclusive,
            "latency_ms": (self.latency_ms * 100.0).round() / 100.0,
            "live": self.live,
            "live_latency_ms": (self.live_latency_ms * 100.0).round() / 100.0,
            "underruns": self.underruns,
            "failed": self.failed,
            "fallback": self.fallback,
            "channels": self.channels,
            "cue_pair": self.cue_pair,
            "input": self.input,
            "input_channels": self.input_channels,
            "input_underruns": self.input_underruns,
        })
    }

    /// One line for the window: device, host, rate and buffer.
    pub fn describe(&self) -> String {
        let mode = if self.exclusive && self.host == "WASAPI" { ", exclusive" } else { "" };
        let cue = match self.cue_pair {
            Some(pair) => {
                let (first, second) = cue_channels(pair);
                format!(", cue on {first}/{second}")
            }
            None => String::new(),
        };
        format!(
            "{} ({}{mode}, {} Hz, {} frames{cue})",
            self.device,
            self.host,
            self.sample_rate,
            if self.buffer_frames > 0 { self.buffer_frames } else { self.engine_block }
        )
    }
}

/// A playing output's live numbers, read from the main thread.
pub trait LiveOutput: Send + Sync {
    fn info(&self) -> OutputInfo;
}

/// A player on the output a choice names.
pub struct Opened {
    pub player: Player,
    /// What the user should hear about: the chosen device was not there and the default
    /// plays instead, or a part of the choice could not be had.
    pub notice: Option<String>,
    /// The previous player's plug-ins run on in the new one; when false the session starts
    /// them again (a new sample rate, or the old output did not give them back).
    pub plugins_kept: bool,
}

/// The audio devices of the system, and opening a project on one of them; made by the output
/// layer, which is the only code that opens an audio device.
pub trait AudioSystem: Send + Sync {
    /// The hosts and their output devices.
    fn hosts(&self) -> Vec<HostInfo>;

    /// Plays `project` with its transport stopped on the output `choice` names, or on the
    /// default one, with a notice, when that is not there. `previous` is the player it
    /// replaces: it is closed first (a device may only be open once), and its running plug-ins
    /// carry over when the sample rate stays the same.
    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        assets: &DeviceAssets,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String>;

    /// Whether the host and device `choice` names are there now, so an output playing in their
    /// place can go back to them; `None` while that is not known yet. Asked every few seconds
    /// from the thread that owns the projects, so it must answer at once: the output layer
    /// looks in the background and gives the last answer. This one lists the hosts.
    fn chosen_present(&self, choice: &OutputChoice) -> Option<bool> {
        Some(choice.found_in(&self.hosts()))
    }
}

static AUDIO_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "audio.devices",
        label: "List the audio hosts (WASAPI, ASIO, ALSA…), each one's output devices with their sample rates and buffer sizes and its input devices with their channels, and the output chosen",
        params: &[],
        kind: CommandKind::Audio,
        run: devices,
    },
    CommandSpec {
        id: "audio.setOutput",
        label: "Choose the audio output: host, device, sample rate, buffer size (the block the engine renders in, fitted to the sizes the device allows; on WASAPI shared mode the engine's block alone, since Windows sets the device's buffer), WASAPI exclusive mode and the audio input device the tracks' audio inputs hear; parts left out stay, an empty text or 0 goes back to the default. Reopens the active project's output, keeping the transport where it is",
        params: &[
            optional("host", ParamType::Text, "a host from audio.devices, \"\" for the default"),
            optional("device", ParamType::Text, "an output device of that host, \"\" for its default"),
            optional("sample_rate", ParamType::Integer, "Hz, one the device lists; 0 for its default"),
            optional(
                "buffer_frames",
                ParamType::Integer,
                "frames per buffer, 64 to 4096, fitted to the sizes the device allows; 0 for the default",
            ),
            optional("exclusive", ParamType::Bool, "WASAPI: open the device for Suhina alone (default false)"),
            optional(
                "input",
                ParamType::Text,
                "the audio input device on that host, from audio.devices' inputs: \"off\" (the default), \"default\" or a name",
            ),
        ],
        kind: CommandKind::Audio,
        run: set_output,
    },
    CommandSpec {
        id: "output.setCuePair",
        label: "Choose the output pair of the same device the cue plays on: previews and tracks whose cue is on are heard there (headphones) and not on the main output's channels 1 and 2. Pair 0 plays the cue with the main output. Reopens the output, keeping the transport where it is",
        params: &[param("pair", ParamType::Integer, "1 for channels 3/4, 2 for 5/6 … up to 31; 0 for the main output")],
        kind: CommandKind::Audio,
        run: set_cue_pair,
    },
    CommandSpec {
        id: "output.setCueVolume",
        label: "Set the cue volume: the level of previews and cued tracks on the cue, from silence to +12 dB. The main output does not change, except for previews while the cue plays with it",
        params: &[param("db", ParamType::Number, "dB, -inf (or below -150) for silence, at most +12; 0 is unity")],
        kind: CommandKind::Audio,
        run: set_cue_volume,
    },
    CommandSpec {
        id: "audio.info",
        label: "Say what the active project plays on now: host, device, sample rate, buffer frames, the engine's block, measured latency, the longest a key on a MIDI controller takes to be heard, whether a track is armed (which keeps less queued) and underruns, and whether it plays in place of a chosen device that is not there",
        params: &[],
        kind: CommandKind::Audio,
        run: info,
    },
];

/// The commands for the audio output.
pub fn audio_commands() -> &'static [CommandSpec<Projects>] {
    AUDIO_COMMANDS
}

fn system(projects: &Projects) -> Result<Arc<dyn AudioSystem>, CommandError> {
    projects.audio_system().ok_or_else(|| {
        CommandError("no audio output here; the desktop app has one, and suhina-cli mcp with --audio".into())
    })
}

pub(crate) fn hosts_json(hosts: &[HostInfo]) -> Value {
    Value::Array(
        hosts
            .iter()
            .map(|h| {
                json!({
                    "name": h.name,
                    "default": h.default,
                    "error": h.error,
                    "devices": h.devices.iter().map(|d| json!({
                        "name": d.name,
                        "default": d.default,
                        "channels": d.channels,
                        "sample_rates": d.sample_rates,
                        "default_sample_rate": d.default_sample_rate,
                        "buffer_sizes": d.buffer_sizes,
                        "buffer_range": d.buffer_range.map(|(min, max)| json!({ "min": min, "max": max })),
                        "exclusive": d.exclusive,
                    })).collect::<Vec<_>>(),
                    "inputs": h.inputs.iter().map(|d| json!({
                        "name": d.name,
                        "default": d.default,
                        "channels": d.channels,
                        "sample_rates": d.sample_rates,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn devices(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let hosts = system(projects)?.hosts();
    Ok(json!({ "hosts": hosts_json(&hosts), "chosen": projects.output_choice().to_json() }))
}

fn set_output(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let choice = projects.output_choice().with_params(params)?;
    projects.set_output_choice(choice);
    let notice = projects.reopen_output()?;
    let mut result = info(projects, &Value::Null)?;
    result["notice"] = json!(notice);
    Ok(result)
}

fn set_cue_pair(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    if params.get("pair").is_none() {
        return Err("pair must be a whole number from 0 (the main output) to 31".into());
    }
    let choice = projects.output_choice().with_params(&json!({ "cue_pair": params.get("pair") }))?;
    // Checked against the device when there is an audio system; else only chosen, as
    // `audio.setOutput` chooses.
    if let Some(system) = projects.audio_system() {
        choice.check_cue_pair(choice.device_channels(&system.hosts()))?;
    }
    projects.set_output_choice(choice);
    let notice = projects.reopen_output()?;
    let mut result = info(projects, &Value::Null)?;
    result["notice"] = json!(notice);
    Ok(result)
}

fn set_cue_volume(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let choice = projects.output_choice().with_cue_volume(params)?;
    let gain = choice.cue_gain;
    projects.set_output_choice(choice);
    // The playing output takes the new volume at once; nothing is reopened.
    if let Ok(session) = projects.session_mut(None)
        && let Ok(player) = session.player_mut()
    {
        player.send(suhina_engine::LaunchCommand::SetCue { gain: Some(gain), separate: None })?;
    }
    Ok(json!({ "cue_db": gain_db(gain) }))
}

fn info(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let chosen = projects.output_choice().to_json();
    let playing = projects.session_mut(None).ok().and_then(|s| s.player()).and_then(Player::output_info);
    Ok(match playing {
        Some(info) => {
            let mut value = info.to_json();
            value["playing"] = json!(true);
            value["chosen"] = chosen;
            value
        }
        None => json!({ "playing": false, "chosen": chosen }),
    })
}

/// Where the transport was, to put it back on a new output: the play start marker, the
/// playhead, and what the launcher plays and has queued.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Transport {
    running: bool,
    beat: f64,
    marker: f64,
    metronome: bool,
    tempo_bpm: f64,
    /// The launcher clock followed the tempo map: no scene had taken it off.
    follows_map: bool,
    quantization: Option<suhina_engine::Quantization>,
    /// Columns that play launched clips or have a launch or stop queued.
    columns: Vec<ColumnState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ColumnState {
    column: usize,
    status: suhina_engine::ColumnStatus,
    elapsed_beats: f64,
}

impl Transport {
    pub(crate) fn of(player: &Player) -> Self {
        let monitor = player.monitor();
        let running = monitor.running();
        let columns = if running {
            (0..monitor.columns())
                .filter_map(|column| {
                    let status = monitor.column(column)?;
                    let elapsed_beats = monitor.elapsed_beats(column).unwrap_or(0.0);
                    (status.overrides_arrangement || status.playing.is_some() || status.queued.is_some())
                        .then_some(ColumnState { column, status, elapsed_beats })
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            running,
            beat: monitor.beat(),
            marker: monitor.start_beat(),
            metronome: monitor.metronome(),
            tempo_bpm: monitor.tempo_bpm(),
            follows_map: monitor.meter().follows_map,
            quantization: player.quantization(),
            columns,
        }
    }

    /// Sends `player` the commands that bring it to this state, all applied before its first
    /// block: the quantisation, the metronome and the marker; while playing, the start, a jump
    /// to the playhead that leaves the marker where it is, the launcher's tempo, each column's
    /// launched clip as far into it as it was (or the column's arrangement kept silent), and the
    /// launches and stops that were waiting, which wait for the next boundary again.
    pub(crate) fn restore(&self, player: &mut Player) -> Result<(), CommandError> {
        use suhina_engine::{LaunchCommand, Queued};
        if let Some(quantization) = self.quantization {
            player.send(LaunchCommand::SetQuantization(quantization))?;
        }
        if self.metronome {
            player.send(LaunchCommand::SetMetronome { on: true, gain: None })?;
        }
        if self.marker.is_finite() && self.marker > 0.0 {
            player.send(LaunchCommand::SetPosition { beat: self.marker })?;
        }
        if !self.running {
            return Ok(());
        }
        player.send(LaunchCommand::Play)?;
        if self.beat.is_finite() && self.beat != self.marker {
            player.send(LaunchCommand::Locate { beat: self.beat })?;
        }
        // A scene's tempo took the clock off the tempo map: the new output plays it too. On the
        // map, the new output reads the tempo from the map at the playhead.
        if !self.follows_map && self.tempo_bpm.is_finite() {
            player.send(LaunchCommand::SetLaunchTempo { bpm: self.tempo_bpm })?;
        }
        for state in &self.columns {
            let column = state.column;
            if state.status.overrides_arrangement {
                let slot = state.status.playing;
                player.send(LaunchCommand::ResumeColumn { column, slot, elapsed_beats: state.elapsed_beats })?;
            }
            match state.status.queued {
                Some(Queued::Launch(slot)) => player.send(LaunchCommand::LaunchClip { column, slot })?,
                Some(Queued::Stop) => player.send(LaunchCommand::StopColumn { column })?,
                None => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_changes_only_what_is_given() {
        let start = OutputChoice {
            host: Some("WASAPI".into()),
            device: Some("Speakers".into()),
            sample_rate: Some(48_000),
            buffer_frames: Some(256),
            exclusive: false,
            ..OutputChoice::default()
        };
        let same = start.with_params(&json!({})).unwrap();
        assert_eq!(same, start);

        let buffer = start.with_params(&json!({ "buffer_frames": 512 })).unwrap();
        assert_eq!(buffer.buffer_frames, Some(512));
        assert_eq!(buffer.device.as_deref(), Some("Speakers"));

        // A new host forgets the device; an empty text and 0 go back to the default.
        let asio = start.with_params(&json!({ "host": "ASIO" })).unwrap();
        assert_eq!((asio.host.as_deref(), asio.device), (Some("ASIO"), None));
        let same_host = start.with_params(&json!({ "host": "WASAPI" })).unwrap();
        assert_eq!(same_host.device.as_deref(), Some("Speakers"));
        let reset = start.with_params(&json!({ "device": "", "sample_rate": 0, "buffer_frames": 0 })).unwrap();
        assert_eq!((reset.device, reset.sample_rate, reset.buffer_frames), (None, None, None));
        let both = start.with_params(&json!({ "host": "ASIO", "device": "Interface", "exclusive": true })).unwrap();
        assert_eq!((both.device.as_deref(), both.exclusive), (Some("Interface"), true));
        // The input: off until chosen, the default or one by name, off again.
        assert_eq!(start.input, InputChoice::Off);
        let mic = start.with_params(&json!({ "input": "default" })).unwrap();
        assert_eq!(mic.input, InputChoice::Default);
        let named = mic.with_params(&json!({ "input": " Line In (Interface) " })).unwrap();
        assert_eq!(named.input, InputChoice::Named("Line In (Interface)".into()));
        assert_eq!(named.with_params(&json!({ "input": "off" })).unwrap().input, InputChoice::Off);
        assert_eq!(named.with_params(&json!({ "input": "" })).unwrap().input, InputChoice::Off);
        assert_eq!(named.with_params(&json!({ "buffer_frames": 128 })).unwrap().input, named.input, "kept");
        for choice in [InputChoice::Off, InputChoice::Default, InputChoice::Named("Mic".into())] {
            assert_eq!(InputChoice::from_text(choice.as_text()), choice);
        }
    }

    #[test]
    fn a_choice_refuses_nonsense() {
        let start = OutputChoice::default();
        for bad in [
            json!({ "buffer_frames": 32 }),
            json!({ "buffer_frames": 8192 }),
            json!({ "buffer_frames": -1 }),
            json!({ "buffer_frames": 1.5 }),
            json!({ "sample_rate": 10_000_000 }),
            json!({ "host": 5 }),
            json!({ "exclusive": "yes" }),
            json!({ "input": 3 }),
        ] {
            assert!(start.with_params(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn output_info_describes_itself() {
        let info = OutputInfo {
            host: "WASAPI".into(),
            device: "Speakers".into(),
            sample_rate: 44_100,
            buffer_frames: 0,
            engine_block: 512,
            exclusive: true,
            latency_ms: 23.456,
            ..OutputInfo::default()
        };
        assert_eq!(info.describe(), "Speakers (WASAPI, exclusive, 44100 Hz, 512 frames)");
        let cued = OutputInfo { cue_pair: Some(1), ..info.clone() };
        assert_eq!(cued.describe(), "Speakers (WASAPI, exclusive, 44100 Hz, 512 frames, cue on 3/4)");
        assert_eq!(info.to_json()["latency_ms"], json!(23.46));
    }
}
