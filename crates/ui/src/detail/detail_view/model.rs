// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the detail area shows, built from the project: the selected track's chain as boxes of devices, groups and plug-ins, and the selected clip.

use super::*;

pub(super) fn device_display_name(device: &suhina_engine::model::Device) -> String {
    let Some(file) =
        device.sample_file().and_then(|path| std::path::Path::new(path).file_name()).and_then(|s| s.to_str())
    else {
        return device.name.clone();
    };
    let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file);
    if device.name.to_lowercase().contains(&stem.to_lowercase()) {
        device.name.clone()
    } else {
        format!("{} \u{b7} {file}", device.name)
    }
}

/// Where the panel shows a plug-in's embedded editor this frame, for its host to put it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPlace {
    pub track: TrackId,
    pub device: usize,
    pub placement: Placement,
    /// The room the panel has for the editor, in pixels, where it is asked to fit (an ARA
    /// editor in the clip's place); `None` where it keeps its own size.
    pub fit: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceBox {
    /// Its index in the list it is in: the track's devices, or a group's branch.
    pub index: usize,
    /// Where it is on the track, as the device commands take it.
    pub at: DeviceAt,
    /// Which device it is, wherever it moves (`DeviceId::NONE` before the project gives ids).
    pub id: suhina_engine::model::DeviceId,
    /// A built-in effect, instrument or MIDI effect: its values one preset saves
    /// (`device.savePreset`, [docs/spec/nam-amp-and-cab.md] › Presets).
    pub is_builtin: bool,
    /// For a device group: its settings and branches.
    pub group: Option<GroupBox>,
    pub name: String,
    /// What it is: a built-in effect or instrument, a plug-in format, or one Suhina cannot run.
    pub kind: String,
    /// What a theme selects its panel by, after `#device-`: a built-in device's id
    /// (`#device-compressor`), `sd` for a device file, `plugin` for a plug-in.
    pub theme_id: String,
    pub enabled: bool,
    /// Parameter ids and values as text.
    pub params: Vec<(String, String)>,
    /// Built-in parameter values, by id (in the parameter's unit, so a panel's knobs take them).
    pub values: Vec<(String, f64)>,
    /// The id of a device that runs a file ([docs/spec/nam-amp-and-cab.md]); the file is the
    /// state's path.
    pub file_device: Option<&'static str>,
    /// Its opened long list scrolls down in two columns, as Settings › Devices says.
    pub list_tall: bool,
    /// The file's path, when the device runs one.
    pub file: Option<String>,
    pub graph: Option<GraphBox>,
    /// For a plug-in: where it stands, while the project plays.
    pub plugin: Option<PluginBox>,
    /// For a device on the track itself that reads a key (sidechain): its key and the tracks
    /// it can read one from (`docs/spec/sidechain.md` › Choosing a source).
    pub sidechain: Option<SidechainBox>,
}

/// A device's sidechain choices: the key it reads, and the tracks that can give one.
#[derive(Debug, Clone, PartialEq)]
pub struct SidechainBox {
    pub key: Option<KeyBox>,
    /// The tracks whose signal it can read, with their names: every track with a mixer
    /// channel but its own and those its own plays into (a key from them would close a loop).
    pub sources: Vec<(TrackId, String)>,
}

/// The key a device reads: from which track, where on it, and how loud.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyBox {
    pub track: TrackId,
    pub name: String,
    pub tap: suhina_engine::model::TapPoint,
    pub gain_db: f64,
    /// Whether the device reads it (`device.setKeyEnabled`): off, the source stays chosen.
    pub enabled: bool,
}

pub use crate::interaction::device_at::DeviceAt;

/// A device group in the detail area: its mix and gain and its branches.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBox {
    pub mix: f64,
    pub gain_db: f64,
    pub branches: Vec<BranchBox>,
}

/// One branch of a device group.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchBox {
    pub name: String,
    pub volume_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub devices: Vec<DeviceBox>,
}

/// What a device panel draws from a parameter list: an `.sd` device's graph, a built-in's
/// panel or a running plug-in's generated list.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphBox {
    /// The compact panel's canvas; `None` draws the parameters as a generated list.
    pub canvas: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    /// The device's larger panel, opened from its header over the main area, if it has one.
    pub expanded: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    /// Controls a built-in's layout puts in its header row, right of its name.
    pub header: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    /// The visible nodes form one chain, so the list marks each step with an arrow.
    pub linear: bool,
    /// The names of an `.sd` graph's visible nodes, in signal order; empty for the others.
    pub nodes: Vec<String>,
    /// Every parameter, in its device's order.
    pub parameters: Vec<GraphParameter>,
}

/// One parameter as a device panel shows and edits it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphParameter {
    /// How its device writes the value it was listed with (a plug-in's "-3.2 dB"), or empty.
    pub text: String,
    /// What a double-click puts it back to.
    pub default: f64,
    /// How it looks: label, size, colors, step, decimals, a choice's labels.
    pub ui: suhina_device_sdk::ParameterUi,
    /// The control that edits it.
    pub control: ParameterControl,
    /// Written after its value; empty for none.
    pub unit: String,
    /// The id `device.setParam` names it by.
    pub id: String,
    /// Its label in the panel.
    pub name: String,
    /// Its lowest value.
    pub min: f64,
    /// Its highest value.
    pub max: f64,
    /// Its value now, within `min..=max`.
    pub value: f64,
}

impl GraphParameter {
    /// A device's live value handed to its canvas (`#r0`…), which no list shows.
    pub(crate) fn is_readout(&self) -> bool {
        self.id.starts_with('#')
    }

    /// The live value `index` of a device for its canvas, as the SDK's `readout` expression
    /// looks it up.
    pub(crate) fn readout(index: usize, value: f32) -> Option<Self> {
        let id = suhina_device_sdk::ui::READOUT_IDS.get(index)?;
        Some(Self::host_value(id, value))
    }

    /// A value the host hands a canvas under `id` (`#r0`, `#expanded`): read like a parameter,
    /// never shown in a list or changed by a control.
    pub(crate) fn host_value(id: &str, value: f32) -> Self {
        let value = if value.is_finite() { f64::from(value) } else { 0.0 };
        Self {
            text: String::new(),
            default: 0.0,
            ui: suhina_device_sdk::ParameterUi::default(),
            control: ParameterControl::default(),
            unit: String::new(),
            id: id.into(),
            name: id.into(),
            min: value.min(0.0),
            max: value.max(0.0).max(f64::MIN_POSITIVE),
            value,
        }
    }

    /// `parameter` as a panel shows it, at `value`.
    pub(crate) fn new(parameter: &suhina_device_sdk::Parameter, value: f64) -> Self {
        Self {
            text: String::new(),
            default: parameter.default,
            ui: parameter.ui.clone(),
            control: parameter.control,
            unit: parameter.unit.clone(),
            id: parameter.id.clone(),
            name: parameter.name.clone(),
            min: parameter.min,
            max: parameter.max,
            value,
        }
    }
}

/// `parameters` at the values `device` keeps for them (kept in range), or at their defaults.
pub(super) fn graph_parameters(
    parameters: &[suhina_device_sdk::Parameter],
    device: &suhina_engine::model::Device,
) -> Vec<GraphParameter> {
    let stored = stored_values(device);
    parameters
        .iter()
        .map(|p| GraphParameter::new(p, stored.get(p.id.as_str()).map_or(p.default, |value| p.clamp(*value))))
        .collect()
}

/// A device's stored values by id, as a device with hundreds of parameters is listed every frame.
/// The first of an id wins, as a search through them would have it.
fn stored_values(device: &suhina_engine::model::Device) -> std::collections::HashMap<&str, f64> {
    device.params.iter().rev().map(|p| (p.id.as_str(), p.value)).collect()
}

/// A plug-in device's own texts for values it was asked about, by its track and its place on the
/// track ([`Detail::follow_automation_texts`], #1482).
pub type PluginTextsAt<'a> = dyn FnMut(TrackId, &[usize]) -> Option<std::sync::Arc<suhina_session::ParamTexts>> + 'a;

/// Where a plug-in device's plug-in stands, as its panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginBox {
    pub state: PluginState,
    /// One line about it: the plug-in that runs, or why it does not.
    pub line: String,
    pub editor_open: bool,
    /// Its editor is open inside the panel: the size of it, in pixels.
    pub embedded: Option<(u32, u32)>,
    /// It is bound to its track's audio clips (ARA), so its editor edits them.
    pub ara: bool,
    /// The ids of the parameters whose lanes hold each value until the next point (a switch, a
    /// list of choices: `ParamInfo::holds`, #1482), from the list it gave last.
    pub held: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// The project is not playing, so its plug-ins are not running.
    Stopped,
    Loading,
    Running,
    Missing,
    Crashed,
}

impl PluginBox {
    pub(super) fn from_report(report: Option<PluginReport>) -> Self {
        let Some(report) = report else {
            return Self {
                state: PluginState::Stopped,
                line: "Not running".into(),
                editor_open: false,
                embedded: None,
                ara: false,
                held: Vec::new(),
            };
        };
        let name = report.plugin.as_ref().map(|(name, vendor, _)| format!("{name} by {vendor}"));
        let (state, line) = match &report.status {
            PluginStatus::Loading => (PluginState::Loading, format!("Loading {}", name.unwrap_or_default())),
            PluginStatus::Running => {
                let mut line = match (&report.state_refused, report.latency) {
                    (Some(_), _) => "Running with its own settings".to_string(),
                    (None, 0) => name.unwrap_or_else(|| "Running".into()),
                    (None, latency) => format!("{} · {latency} frames", name.unwrap_or_else(|| "Running".into())),
                };
                // Blocks it did not answer in time: the chain went on without it for those.
                if report.late_blocks > 0 && report.blocks > 0 {
                    let percent = (u64::from(report.late_blocks) * 100 / u64::from(report.blocks)).max(1);
                    line.push_str(&format!(" · {percent}% of blocks late"));
                    // The plug-in's own longest time, as its host measured it (#1262): close to
                    // the deadline, the plug-in is slow; far under it, its host woke late.
                    if report.longest_process_us > 0 {
                        line.push_str(&format!(" · plug-in up to {}", duration_text(report.longest_process_us)));
                    }
                }
                (PluginState::Running, line)
            }
            PluginStatus::Missing => (PluginState::Missing, "Not installed; kept with its settings".into()),
            PluginStatus::Crashed(why) => (PluginState::Crashed, format!("Crashed: {why}")),
            PluginStatus::Failed(why) => (PluginState::Crashed, format!("Could not start: {why}")),
        };
        let held = report.params.as_deref().unwrap_or_default().iter().filter(|p| p.holds()).map(|p| p.id).collect();
        Self { state, line, editor_open: report.editor_open, embedded: report.editor_embedded, ara: report.ara, held }
    }

    /// The size the embedded editor takes in the panel, in points.
    pub(super) fn editor_size(&self, pixels_per_point: f32) -> Option<Vec2> {
        let (width, height) = self.embedded?;
        let scale = pixels_per_point.max(0.01);
        Some(Vec2::new(width as f32 / scale, height as f32 / scale))
    }

    /// What its button does: its label and the command it runs.
    pub(super) fn button(&self) -> Option<(&'static str, &'static str)> {
        match self.state {
            PluginState::Running if self.editor_open => Some(("Close editor", "device.closeEditor")),
            PluginState::Running => Some(("Open editor", "device.openEditor")),
            PluginState::Crashed => Some(("Restart", "device.restart")),
            PluginState::Stopped | PluginState::Loading | PluginState::Missing => None,
        }
    }
}

/// A selected clip: on the arrangement (its track and its id, which stays when it moves), or in
/// a launcher slot (its track and scene, from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRef {
    Arrangement(TrackId, ClipId),
    Slot(TrackId, usize),
}

impl ClipRef {
    pub fn track(self) -> TrackId {
        match self {
            Self::Arrangement(track, _) | Self::Slot(track, _) => track,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub track: String,
    /// Where it is: bar.beat where an arrangement clip starts, or the number of a slot's scene
    /// (`scene` tells which); and its length in bars and beats.
    pub start: String,
    pub scene: bool,
    pub length: String,
    pub looping: bool,
    /// The loop's start and end in content beats, when it loops.
    pub loop_range: Option<(f64, f64)>,
    pub audio: bool,
    pub gain_db: Option<f64>,
    /// An audio clip's warp switch, warp mode and transpose in semitones (`clip.setAudio`).
    pub warped: bool,
    pub stretch: StretchMode,
    pub transpose: f64,
    /// The settings of the warp modes that have them (`clip.setAudio`'s slice, tail, envelope,
    /// grain and scatter).
    pub warp: suhina_engine::model::WarpSettings,
    /// The clip the column's controls edit; none where nothing can be edited.
    pub at: Option<ClipRef>,
    /// Fade-in and fade-out in beats (`clip.setFades`).
    pub fades: (f64, f64),
    /// The start and end markers in content beats: where in its content the clip starts and
    /// stops playing (`clip.setStart`, `clip.setLength`).
    pub markers: (f64, f64),
    pub notes: usize,
    /// What the piano roll has selected in this clip (› Clip column in
    /// `docs/spec/ui-clip-editor.md`).
    pub selection: SelectionInfo,
    /// The Clip / Devices switch at the end of the title row, and which part is on: only for a
    /// clip with an editor (› Where it opens in `docs/spec/ui-clip-editor.md`).
    pub switch: Option<Showing>,
    /// Both parts are on: the devices are stacked beside the clip's editor, which stays open.
    pub switch_both: bool,
    /// The other arrangement clips selected with it, by id: the title says how many are
    /// selected, and a switch (Loop) turns all of them as it turns this one.
    pub others: Vec<u32>,
    /// Of them, the audio clips, by id, while this one is an audio clip: the panel's audio
    /// settings (Warp, its mode and settings, tempo, gain, pitch) set all of them at once, one undo
    /// step (`docs/spec/clip-panel.md` › Several clips).
    pub audio_others: Vec<u32>,
    /// Every picked MIDI clip, for its row in the column that gives it the piano roll's focus
    /// (#1054): its track and clip, what the row says ("Bass · Riff") and whether it has the focus.
    pub picked: Vec<(TrackId, ClipId, String, bool)>,
    /// What the clip's panel shows beyond the rows (`docs/spec/clip-panel.md`): its color, length,
    /// scale and Set buttons for every clip, and an audio clip's own state.
    pub panel: Option<AudioPanel>,
}

/// What a clip's panel shows beyond the rows: its id, color, length, scale and where the playhead
/// is in it, and for an audio clip its tempo and envelope.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioPanel {
    pub clip: ClipId,
    /// The color it shows: its own, or its track's.
    pub color: Option<suhina_engine::model::Rgb>,
    /// The tempo its audio is warped from, when it is warped, and where that came from.
    pub tempo: Option<f64>,
    pub tempo_source: Option<suhina_engine::model::TempoSource>,
    /// How many points its gain envelope has.
    pub envelope_points: usize,
    /// The playhead as a beat of the clip's content, for the Set buttons; none for a slot's clip,
    /// which has no place on the arrangement's timeline.
    pub playhead: Option<f64>,
    /// Whether the panel has Set buttons: an arrangement clip's has, grayed while `playhead` is
    /// none; a slot's clip has none.
    pub set_buttons: bool,
    /// What each source says the tempo is (metadata, file name, detection), once the Find menu
    /// asked for it.
    pub candidates: Option<[Option<f64>; 3]>,
    /// A bar of the project in beats, for the markers' `bar.beat.sixteenth`.
    pub bar_beats: f64,
    /// The clip's length in beats, as its title says it.
    pub length_beats: f64,
    /// Its scale (an audio clip's key), when one was given (`clip.setScale`).
    pub scale: Option<suhina_engine::model::Scale>,
    /// A MIDI clip's swing (its own, or following the project's), and the project's (#1221).
    pub swing: suhina_engine::model::ClipSwing,
    pub global_swing: f64,
}

/// What the detail area shows for a clip that has an editor: the editor, or its track's devices.
/// Window state, not part of the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Clip,
    Devices,
}

/// What the detail area's frame picked: the Clip / Devices switch's value, and a device panel
/// clicked (to select it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picked {
    pub showing: Option<Showing>,
    /// A device panel was clicked: the track and where the device is, to select (one inside a
    /// group too).
    pub clicked: Option<(TrackId, DeviceAt)>,
    /// How tall the device chain's panels are, in points, when the area shows only the chain:
    /// the detail area is kept no taller.
    pub content_height: Option<u32>,
}

/// What the piano roll has selected in the clip the detail area shows: how many notes, how many
/// of them are muted, the span in bars and beats, and the range of keys and velocities (1 to 127).
/// Empty when nothing is selected, so the column shows no rows for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectionInfo {
    pub selected: usize,
    pub muted: usize,
    pub from: Option<String>,
    pub to: Option<String>,
    pub low_key: Option<u8>,
    pub high_key: Option<u8>,
    pub low_velocity: Option<u8>,
    pub high_velocity: Option<u8>,
}

impl SelectionInfo {
    /// The selection `selected` (indices into `notes`) described as the clip column shows it.
    /// `bar_beats` is how long a bar is, as [`build_with`] reads it from the time signature.
    pub fn of(notes: &[Note], selected: &std::collections::BTreeSet<usize>, bar_beats: f64) -> Self {
        let mut info = Self { selected: selected.len(), ..Self::default() };
        let mut low_key: Option<u8> = None;
        let mut high_key: Option<u8> = None;
        let mut low_velocity: Option<u8> = None;
        let mut high_velocity: Option<u8> = None;
        let mut start: Option<f64> = None;
        let mut end: Option<f64> = None;
        for index in selected {
            let Some(note) = notes.get(*index) else { continue };
            if note.muted {
                info.muted += 1;
            }
            low_key = Some(low_key.map_or(note.key, |key| key.min(note.key)));
            high_key = Some(high_key.map_or(note.key, |key| key.max(note.key)));
            let velocity = suhina_session::midi_velocity(note.velocity);
            low_velocity = Some(low_velocity.map_or(velocity, |v| v.min(velocity)));
            high_velocity = Some(high_velocity.map_or(velocity, |v| v.max(velocity)));
            start = Some(start.map_or(note.time, |time: f64| time.min(note.time)));
            let note_end = note.time + note.duration;
            end = Some(end.map_or(note_end, |time: f64| time.max(note_end)));
        }
        info.from = start.map(|beats| bars_beats(beats, bar_beats, 1));
        info.to = end.map(|beats| bars_beats(beats, bar_beats, 1));
        info.low_key = low_key;
        info.high_key = high_key;
        info.low_velocity = low_velocity;
        info.high_velocity = high_velocity;
        info
    }
}

/// How long a bar is in the project's time signature, in beats, as the clip column counts bars
/// and beats.
pub fn bar_beats(project: &Project) -> f64 {
    let ts = project.transport.time_signature;
    if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
}

/// A song position as the column writes it, `bar.beat` counted from one, with the bars of the
/// project's meter, so it reads the bar the ruler shows after a time-signature change.
pub fn song_position(beat: f64, meter: &Meter) -> String {
    if !beat.is_finite() {
        return "-".into();
    }
    let beat = beat.max(0.0);
    let here = meter.position(beat);
    format!("{}.{}", u64::from(here.bar) + 1, (beat - here.bar_start).max(0.0).floor() as u64 + 1)
}

/// A position as the column writes it: `bar.beat`, counted from one for a position and from zero
/// for a length.
pub fn bars_beats(beats: f64, bar_beats: f64, from_one: u64) -> String {
    if !(beats.is_finite() && bar_beats.is_finite()) || bar_beats <= 0.0 {
        return "-".into();
    }
    let beats = beats.max(0.0);
    let bar = (beats / bar_beats).floor() as u64 + from_one;
    let beat = (beats % bar_beats).floor() as u64 + from_one;
    format!("{bar}.{beat}")
}

/// A track's device chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub track: TrackId,
    pub name: String,
    pub devices: Vec<DeviceBox>,
    /// The channel's clip host, the plug-in that edits its clips (#990): not among `devices`, as
    /// it belongs to the clips, but its editor shows in the clip's place.
    pub host: Option<DeviceBox>,
    /// The NAM panels' folders' files and a file's names and notes
    /// ([docs/spec/nam-amp-and-cab.md] › Folders).
    pub nam: crate::detail::device_widgets::Lists,
}

/// What the detail area shows: a selected clip comes with its track's chain, since that chain
/// is what the clip plays through. The clip's properties are boxed: they are much larger than the
/// chain or nothing, and the detail is one value moved around each frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Devices(Chain),
    Clip(Box<ClipInfo>, Chain),
    Nothing,
    /// The clip view with no clip selected: it stays the clip view until a track's devices are
    /// asked for (a double-click on its header).
    NoClip,
}

impl Detail {
    /// While the transport runs, the panels show what automation plays at `beat`: every enabled
    /// lane of a device's parameter sets the value its knob, slider and text show (#706).
    pub fn follow_automation(&mut self, project: &Project, beat: f64) {
        self.follow_automation_dragged(project, beat, None);
    }

    /// [`Detail::follow_automation`], with the line of a lane being dragged in the Arrangement
    /// view where the drag puts it: its knob follows the drag at the playhead before the command
    /// is in (#811).
    pub fn follow_automation_dragged(
        &mut self,
        project: &Project,
        beat: f64,
        dragged: Option<&crate::arrangement::automation_lanes::Dragged>,
    ) {
        let _ = self.follow_automation_texts(project, beat, dragged, &mut |_, _| None);
    }

    /// [`Detail::follow_automation_dragged`], with each plug-in device's own texts for values it
    /// was asked about (`texts`, by track and the device's place on it, #1482): an automated
    /// plug-in parameter shows the plug-in's text for the value played. Gives back the values
    /// whose text is not known yet, by track, device place and parameter id, for the plug-in to
    /// be asked about.
    pub fn follow_automation_texts(
        &mut self,
        project: &Project,
        beat: f64,
        dragged: Option<&crate::arrangement::automation_lanes::Dragged>,
        texts: &mut PluginTextsAt<'_>,
    ) -> Vec<(TrackId, Vec<usize>, u32, f64)> {
        let chain = match self {
            Self::Devices(chain) | Self::Clip(_, chain) => chain,
            Self::Nothing | Self::NoClip => return Vec::new(),
        };
        let Some(channel) = project
            .tracks
            .iter()
            .find(|t| t.id == chain.track)
            .and_then(|t| t.channel)
            .and_then(|c| project.channel(c))
        else {
            return Vec::new();
        };
        type Dragged = crate::arrangement::automation_lanes::Dragged;
        type Texts<'a> = dyn FnMut(&[usize]) -> Option<std::sync::Arc<suhina_session::ParamTexts>> + 'a;
        fn follow(
            boxes: &mut [DeviceBox],
            devices: &[suhina_engine::model::Device],
            beat: f64,
            dragged: Option<&Dragged>,
            texts: &mut Texts<'_>,
            wanted: &mut Vec<(Vec<usize>, u32, f64)>,
        ) {
            for device_box in boxes {
                if let Some(group) = device_box.group.as_mut() {
                    for branch in &mut group.branches {
                        follow(&mut branch.devices, devices, beat, dragged, texts, wanted);
                    }
                }
                // The lane a drag holds is a device's on the track itself, not one in a group.
                let held = dragged.filter(|d| d.key.device.is_some_and(|index| device_box.at.0 == [index]));
                let Some(device) = suhina_engine::model::device_at(devices, &device_box.at.0) else { continue };
                // A built-in parameter's description, for its kind and range.
                let info = match &device.kind {
                    DeviceKind::Builtin(id) => suhina_device_registry::info(id),
                    _ => None,
                };
                // A plug-in's own texts, looked up only for a plug-in with a lane that plays.
                let mut plugin_texts = None;
                for lane in device.automation.iter().filter(|l| l.enabled && !l.points.is_empty()) {
                    let spec = info.and_then(|info| info.params.iter().find(|p| p.id == lane.param));
                    // On/off and choice parameters hold each point's value until the next, as the
                    // engine plays them and the lane draws them; so do a plug-in's switches and
                    // lists of choices (#1482).
                    // Evenly along the parameter's control, as the engine plays it and the lane
                    // draws it.
                    use crate::arrangement::automation_lanes::Scale;
                    let scale = match spec {
                        Some(spec) => Some(Scale::Device(spec.clone())),
                        None => device_box
                            .graph
                            .as_ref()
                            .and_then(|g| g.parameters.iter().find(|p| p.id == lane.param))
                            .map(|p| {
                                let mut sd =
                                    suhina_device_sdk::Parameter::new(&p.id, &p.name, (p.min, p.max), p.default);
                                sd.control = p.control;
                                Scale::Sd(Box::new(sd))
                            }),
                    };
                    let Some(scale) = scale else { continue };
                    let plugin_id = device_box.plugin.as_ref().and_then(|_| suhina_engine::param_id(&lane.param));
                    let holds =
                        device_box.plugin.as_ref().zip(plugin_id).is_some_and(|(plugin, id)| plugin.held.contains(&id));
                    let (points, beat) = match held {
                        Some(held) if held.key.param == lane.param => (held.points.as_slice(), held.at.unwrap_or(beat)),
                        _ => (&*lane.points, beat),
                    };
                    let Some(value) =
                        suhina_engine::model::automation::value_at(points, beat, scale.stepped() || holds, &scale)
                    else {
                        continue;
                    };
                    // A whole-number parameter shows what it plays, the fraction cut off toward zero.
                    let value = spec.map_or(value, |spec| spec.played(value));
                    let Some(id) = plugin_id else {
                        show_value(device_box, &lane.param, value, None);
                        continue;
                    };
                    let known = plugin_texts.get_or_insert_with(|| texts(&device_box.at.0)).clone();
                    if let Some(value) = show_value(device_box, &lane.param, value, known.as_deref()) {
                        wanted.push((device_box.at.0.clone(), id, value));
                    }
                }
            }
        }
        let dragged = dragged.filter(|d| d.track == chain.track);
        let mut wanted = Vec::new();
        let track = chain.track;
        follow(&mut chain.devices, &channel.devices, beat, dragged, &mut |path| texts(track, path), &mut wanted);
        wanted.into_iter().map(|(path, id, value)| (track, path, id, value)).collect()
    }
}

/// Shows `value` as parameter `param`'s on its device's panel: its knob or slider and its text.
/// For a plug-in's parameter, `texts` are the plug-in's own texts for values it was asked about
/// (#1482): the text of the value played when it gave one, else the number. Gives back the value
/// as the panel keeps it when the plug-in is to be asked for its text.
fn show_value(
    device_box: &mut DeviceBox,
    param: &str,
    value: f64,
    texts: Option<&suhina_session::ParamTexts>,
) -> Option<f64> {
    for (id, v) in &mut device_box.values {
        if id.as_str() == param {
            *v = value;
        }
    }
    for (id, text) in &mut device_box.params {
        if id.as_str() == param {
            *text = format_value(value);
        }
    }
    let mut ask = None;
    if let Some(graph) = device_box.graph.as_mut() {
        for parameter in graph.parameters.iter_mut().filter(|p| p.id == param) {
            let mut value = value.clamp(parameter.min, parameter.max);
            // A plug-in's parameter listed with steps plays on its nearest step, as the lane
            // shows it; the plug-in names that step.
            let step = parameter.ui.step;
            if texts.is_some() && step > 0.0 && step.is_finite() {
                value = (parameter.min + ((value - parameter.min) / step).round() * step)
                    .clamp(parameter.min, parameter.max);
            }
            // A plug-in's listed text is for the value it listed, not the one played: the one
            // played takes the plug-in's text for it, once the plug-in has given it. A value
            // that did not move but has no text yet (the project keeps one the plug-in did not
            // list) is looked up too; a listed text for the same value stays.
            if value != parameter.value || (texts.is_some() && parameter.text.is_empty()) {
                let id = suhina_engine::param_id(&parameter.id);
                let known = texts.zip(id).map(|(texts, id)| {
                    texts.lock().map(|kept| kept.get(&(id, value.to_bits())).cloned()).unwrap_or_default()
                });
                match known {
                    // Asked before: its text, or the number when it gave none.
                    Some(Some(text)) => parameter.text = if text.trim().is_empty() { String::new() } else { text },
                    Some(None) => {
                        parameter.text.clear();
                        ask = Some(value);
                    }
                    None => parameter.text.clear(),
                }
            }
            parameter.value = value;
        }
    }
    ask
}

/// The beat of `clip`'s content that plays at song beat `beat`, as the engine plays it: its start
/// marker plus the beats since its start, times its repitch rate when it is not warped, around its
/// loop. For the clip panel's Set buttons (`docs/spec/clip-panel.md` › Set to the playhead).
pub fn content_beat_at(clip: &suhina_engine::model::Clip, beat: f64) -> f64 {
    let rate = if clip.audio.warped { 1.0 } else { 2f64.powf(clip.audio.transpose / 12.0) };
    let rate = if rate.is_finite() && rate > 0.0 { rate } else { 1.0 };
    let loop_range = clip.loop_range.filter(|r| r.end > r.start);
    let offset = match loop_range {
        Some(r) if clip.offset >= r.end => r.start,
        _ => clip.offset,
    };
    let content = offset + (beat - clip.start).max(0.0) * rate;
    match loop_range {
        Some(r) if content >= r.end => r.start + (content - r.end).rem_euclid(r.end - r.start),
        _ => content,
    }
}

/// The detail for a selected clip (it wins, with its own track's chain) or track.
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<ClipRef>) -> Detail {
    build_with(project, track, clip, &mut |_, _| None, crate::detail::device_widgets::Lists::default())
}

/// [`build`], with each plug-in device's state from `plugins` (track, device path).
pub fn build_with(
    project: &Project,
    track: Option<TrackId>,
    clip: Option<ClipRef>,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
    nam: crate::detail::device_widgets::Lists,
) -> Detail {
    let bar = bar_beats(project);
    let meter = project.transport.meter();
    let track_name = |id: TrackId| project.shown_track_name(id);
    let found = clip.and_then(|clip| match clip {
        ClipRef::Arrangement(id, clip) => project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(id))
            .and_then(|l| l.clips.iter().find(|c| c.id == clip))
            .map(|placed| (id, placed, None)),
        ClipRef::Slot(id, scene) => project
            .launcher
            .columns
            .iter()
            .find(|c| c.track == id)
            .and_then(|c| c.slots.get(scene))
            .and_then(|s| s.clip.as_ref())
            .map(|placed| (id, placed, Some(scene))),
    });
    if let Some((id, placed, scene)) = found {
        let content = project.content(placed.content);
        let audio = content.and_then(|c| {
            c.lanes.iter().find_map(|l| match &l.data {
                LaneData::Audio(region) => Some(region),
                LaneData::Notes(_) => None,
            })
        });
        let notes = content.map_or(0, |c| {
            c.lanes
                .iter()
                .map(|l| match &l.data {
                    LaneData::Notes(notes) => notes.len(),
                    LaneData::Audio(_) => 0,
                })
                .sum()
        });
        return Detail::Clip(
            Box::new(ClipInfo {
                name: content.map(|c| c.name.clone()).unwrap_or_default(),
                track: track_name(id),
                start: scene.map_or_else(|| song_position(placed.start, &meter), |s| (s + 1).to_string()),
                scene: scene.is_some(),
                length: bars_beats(placed.length, bar, 0),
                looping: placed.loop_range.is_some(),
                loop_range: placed.loop_range.map(|r| (r.start, r.end)),
                audio: audio.is_some(),
                gain_db: audio.map(|_| gain_to_db(placed.audio.gain)),
                warped: audio.is_some() && placed.audio.warped,
                stretch: if audio.is_some() { placed.audio.stretch } else { Default::default() },
                transpose: if audio.is_some() { placed.audio.transpose } else { 0.0 },
                warp: placed.audio.warp,
                at: Some(match scene {
                    Some(scene) => ClipRef::Slot(id, scene),
                    None => ClipRef::Arrangement(id, placed.id),
                }),
                fades: (placed.fade_in, placed.fade_out),
                markers: (placed.offset, placed.offset + placed.length),
                notes,
                selection: SelectionInfo::default(),
                switch: None,
                switch_both: false,
                others: Vec::new(),
                audio_others: Vec::new(),
                picked: Vec::new(),
                panel: Some(AudioPanel {
                    clip: placed.id,
                    color: placed.color.or_else(|| project.track(id).and_then(|t| t.color)),
                    tempo: placed.audio.tempo().filter(|_| audio.is_some() && placed.audio.warped),
                    tempo_source: placed.audio.tempo_source.filter(|_| audio.is_some()),
                    envelope_points: if audio.is_some() { placed.audio.gain_envelope.len() } else { 0 },
                    playhead: None,
                    set_buttons: scene.is_none(),
                    candidates: None,
                    bar_beats: bar,
                    length_beats: placed.length,
                    scale: content.and_then(|c| c.scale),
                    swing: placed.swing,
                    global_swing: project.transport.swing,
                }),
            }),
            chain(project, id, plugins, nam.clone()),
        );
    }
    track.map_or(Detail::Nothing, |id| Detail::Devices(chain(project, id, plugins, nam.clone())))
}

/// A track's devices as the detail area shows them.
pub(super) fn chain(
    project: &Project,
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
    nam: crate::detail::device_widgets::Lists,
) -> Chain {
    // Main has no track: its name and devices are the main channel's (#687).
    let track_name = if id == TrackId::MAIN { "Main".to_string() } else { project.shown_track_name(id) };
    let channel = project.channel_of_track(id);
    // The clip host is the clips' (#990): not in the track's chain as the area shows it.
    let devices = channel
        .map(|c| {
            c.devices
                .iter()
                .enumerate()
                .filter(|(_, d)| !d.clip_host)
                .map(|(index, d)| device_box(d, &[index], id, plugins))
                .collect()
        })
        .unwrap_or_default();
    let host = channel.and_then(|c| {
        c.devices.iter().enumerate().find(|(_, d)| d.clip_host).map(|(index, d)| device_box(d, &[index], id, plugins))
    });
    let mut devices: Vec<DeviceBox> = devices;
    if let Some(channel) = channel {
        let mut sources = None;
        for shown in &mut devices {
            let Some(device) = channel.devices.get(shown.index).filter(|d| suhina_session::reads_a_key(d)) else {
                continue;
            };
            let sources = sources.get_or_insert_with(|| key_sources(project, channel.id)).clone();
            shown.sidechain = Some(SidechainBox { key: key_box(project, device), sources });
        }
    }
    Chain { track: id, name: track_name, devices, host, nam }
}

/// The tracks a device on `reader` can read a key from, in project order: each track with a
/// mixer channel but the reader's own, leaving out those a key from would close a loop.
pub(crate) fn key_sources(project: &Project, reader: suhina_engine::model::ChannelId) -> Vec<(TrackId, String)> {
    // One walk: the channels the reader plays into, whose key would close a loop.
    let after = project.channels_after(reader);
    project
        .tracks
        .iter()
        .filter_map(|t| t.channel.map(|c| (t.id, c)))
        .filter(|&(_, channel)| channel != reader && !after.contains(&channel))
        .map(|(track, _)| (track, project.shown_track_name(track)))
        .collect()
}

/// The key `device` reads, as the panel names it; `None` when it reads none, or its source has
/// no track.
fn key_box(project: &Project, device: &suhina_engine::model::Device) -> Option<KeyBox> {
    let input = device.inputs.first()?;
    let channel = input.source.channel();
    let track = project.tracks.iter().find(|t| t.channel == Some(channel))?.id;
    let gain_db = if input.gain > 0.0 { 20.0 * input.gain.log10() } else { -150.0 };
    Some(KeyBox {
        track,
        name: project.shown_track_name(track),
        tap: input.source.tap(),
        gain_db,
        enabled: input.enabled,
    })
}

/// One device as the detail area shows it, at `path` on track `id`; a group with its branches.
pub(super) fn device_box(
    d: &suhina_engine::model::Device,
    path: &[usize],
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
) -> DeviceBox {
    let report = matches!(d.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. })
        .then(|| plugins(id, path));
    DeviceBox {
        index: path.last().copied().unwrap_or(0),
        at: DeviceAt(path.to_vec()),
        id: d.id,
        is_builtin: matches!(d.kind, DeviceKind::Builtin(_)),
        group: d.as_group().map(|g| group_box(g, d, path, id, plugins)),
        name: device_display_name(d),
        theme_id: match &d.kind {
            DeviceKind::Builtin(id) => id.to_ascii_lowercase(),
            DeviceKind::Sd => "sd".into(),
            DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => "plugin".into(),
            _ => "unsupported".into(),
        },
        kind: match &d.kind {
            DeviceKind::Builtin(_) => "Suhina".into(),
            DeviceKind::Sd => d
                .state
                .as_deref()
                .and_then(|state| std::str::from_utf8(state).ok())
                .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
                .map(|graph| {
                    match graph.role {
                        suhina_device_sdk::DeviceRole::Effect => "AudioEffect",
                        suhina_device_sdk::DeviceRole::Instrument => "Instrument",
                        suhina_device_sdk::DeviceRole::MidiEffect => "MIDI Effect",
                    }
                    .to_owned()
                })
                .unwrap_or_else(|| "AudioEffect".into()),
            DeviceKind::Clap { .. } => "CLAP".into(),
            DeviceKind::Vst3 { .. } => "VST3".into(),
            DeviceKind::Vst2 { .. } => "VST2".into(),
            DeviceKind::Foreign { format, .. } => format!("Not supported ({format})"),
            DeviceKind::Group(_) => "Group".into(),
        },
        enabled: d.enabled,
        params: d.params.iter().map(|p| (p.id.clone(), format_value(p.value))).collect(),
        values: if let DeviceKind::Builtin(id) = &d.kind {
            let info = suhina_device_registry::info(id);
            let stored = stored_values(d);
            info.map(|info| {
                info.params
                    .iter()
                    .map(|p| (p.id.to_string(), stored.get(&*p.id).map_or(p.default, |value| p.clamp(*value))))
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        },
        list_tall: false,
        file_device: match &d.kind {
            DeviceKind::Builtin(id) if suhina_device_registry::is_file_device(id) => {
                suhina_device_registry::info(id).map(|info| &*info.id)
            }
            _ => None,
        },
        file: d
            .state
            .as_deref()
            .and_then(|state| std::str::from_utf8(state).ok())
            .filter(|path| !path.is_empty())
            .map(str::to_owned),
        graph: match (&d.kind, d.state.as_deref()) {
            // A built-in's panel, loaded by its id from this Suhina (ADR 0021 › Amendment: device
            // panels): its parameters from the device, its layout's canvas or the generated list.
            (DeviceKind::Builtin(id), _) => suhina_device_registry::panel(id).map(|panel| GraphBox {
                canvas: panel.ui.clone(),
                expanded: panel.expanded.clone(),
                header: panel.header.clone(),
                linear: false,
                nodes: vec![],
                parameters: graph_parameters(&panel.parameters, d),
            }),
            (DeviceKind::Sd, Some(state)) => crate::detail::device_canvas::graph(state).map(|cached| {
                let graph = &cached.graph;
                GraphBox {
                    canvas: cached.canvas.clone(),
                    expanded: cached.expanded.clone(),
                    header: None,
                    linear: graph.connections.len() == graph.nodes.len().saturating_sub(1)
                        && graph
                            .nodes
                            .iter()
                            .all(|node| graph.connections.iter().filter(|e| e.from == node.id).count() <= 1)
                        && graph.connections.iter().all(|e| e.from_port == "out" && e.to_port == "in"),
                    nodes: graph
                        .ordered_nodes()
                        .iter()
                        .filter(|node| node.visible)
                        .map(|node| node.kind.info().name.into())
                        .collect(),
                    parameters: graph_parameters(&graph.parameters, d),
                }
            }),
            // A plug-in's generated list, from the parameters it listed (#679).
            _ => report
                .as_ref()
                .and_then(Option::as_ref)
                .filter(|r| r.status == PluginStatus::Running)
                .and_then(|r| r.params.as_deref())
                .map(|list| plugin_graph(d, list)),
        },
        plugin: report.map(PluginBox::from_report),
        sidechain: None,
    }
}

/// A plug-in's parameters as its generated list draws them: every one it shows and lets be
/// changed, in its order, with the value the project keeps for it or the one it listed, and its
/// own text for that value. A switch is a toggle; a stepped parameter moves in its steps.
pub(super) fn plugin_graph(d: &suhina_engine::model::Device, list: &[suhina_plugins::ParamInfo]) -> GraphBox {
    let parameters = list
        .iter()
        .filter(|p| !p.hidden && !p.read_only && p.min < p.max)
        .take(suhina_device_sdk::MAX_PARAMETERS)
        .map(|p| {
            let id = p.id.to_string();
            // The last value wins, as the engine reads a duplicate parameter id.
            let stored = d.params.iter().rev().find(|v| suhina_engine::param_id(&v.id) == Some(p.id)).map(|v| v.value);
            let toggle = p.steps == 1 && p.min == 0.0 && p.max == 1.0;
            let mut ui = suhina_device_sdk::ParameterUi { group: p.group.clone(), decimals: 2, ..Default::default() };
            if p.steps > 1 {
                ui.step = (p.max - p.min) / f64::from(p.steps);
            }
            GraphParameter {
                text: if stored.is_none_or(|v| v == p.value) { p.text.clone() } else { String::new() },
                default: p.default.clamp(p.min, p.max),
                ui,
                control: if toggle { ParameterControl::Toggle } else { ParameterControl::Slider },
                unit: String::new(),
                name: if p.name.is_empty() { id.clone() } else { p.name.clone() },
                id,
                min: p.min,
                max: p.max,
                value: stored.unwrap_or(p.value).clamp(p.min, p.max),
            }
        })
        .collect();
    GraphBox { canvas: None, expanded: None, header: None, linear: false, nodes: vec![], parameters }
}

/// A device group's settings and its branches with their devices.
pub(super) fn group_box(
    group: &suhina_engine::model::DeviceGroup,
    device: &suhina_engine::model::Device,
    path: &[usize],
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
) -> GroupBox {
    let values = suhina_engine::group_param_values(&device.params);
    GroupBox {
        mix: values.first().copied().unwrap_or(1.0),
        gain_db: values.get(1).copied().unwrap_or(0.0),
        branches: group
            .branches
            .iter()
            .enumerate()
            .map(|(b, branch)| BranchBox {
                name: branch.name.clone(),
                volume_db: gain_to_db(branch.volume),
                pan: branch.pan,
                mute: branch.mute,
                solo: branch.solo,
                devices: branch
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(i, inner)| {
                        let mut at = path.to_vec();
                        at.extend([b, i]);
                        device_box(inner, &at, id, plugins)
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// `us` microseconds as the device panel shows a time: whole milliseconds from 10 ms, one decimal
/// below that, microseconds under 1 ms.
pub(super) fn duration_text(us: u32) -> String {
    match us {
        0..1_000 => format!("{us} µs"),
        1_000..10_000 => format!("{:.1} ms", f64::from(us) / 1000.0),
        _ => format!("{} ms", (u64::from(us) + 500) / 1000),
    }
}
