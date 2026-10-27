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
    /// The file's path, when the device runs one.
    pub file: Option<String>,
    pub graph: Option<GraphBox>,
    /// For a plug-in: where it stands, while the project plays.
    pub plugin: Option<PluginBox>,
}

/// Where a device is on its track: its index, or its path through device groups
/// (`docs/spec/device-groups.md` › Commands).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DeviceAt(pub Vec<usize>);

impl DeviceAt {
    /// The `device` parameter of a device command: the index of a device on the track, or the
    /// path `"2/1/0"` of one inside a group.
    pub fn value(&self) -> serde_json::Value {
        match self.0.as_slice() {
            [index] => json!(index),
            path => json!(path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")),
        }
    }

    /// Whether it is on the track itself, not inside a group.
    pub fn top(&self) -> bool {
        self.0.len() == 1
    }

    /// The device at `index` on the track itself.
    pub fn at(index: usize) -> Self {
        Self(vec![index])
    }

    /// From a device command's `device` parameter: an index, or a path `"2/1/0"`.
    pub fn parse(value: &serde_json::Value) -> Option<Self> {
        if let Some(index) = value.as_u64() {
            return usize::try_from(index).ok().map(Self::at);
        }
        let path: Option<Vec<usize>> = value.as_str()?.split('/').map(|n| n.trim().parse().ok()).collect();
        path.filter(|p| !p.is_empty()).map(Self)
    }

    /// Its index on the track, when it is on the track itself.
    pub fn index(&self) -> Option<usize> {
        match self.0.as_slice() {
            [index] => Some(*index),
            _ => None,
        }
    }

    /// The chain or branch it is in (the path to it without its own index), and its index there.
    pub fn split(&self) -> (&[usize], usize) {
        match self.0.split_last() {
            Some((last, parent)) => (parent, *last),
            None => (&[], 0),
        }
    }
}

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
    /// How it looks: label, size, colours, step, decimals, a choice's labels.
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
    parameters
        .iter()
        .map(|p| {
            GraphParameter::new(p, device.params.iter().find(|v| v.id == p.id).map_or(p.default, |v| p.clamp(v.value)))
        })
        .collect()
}

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
                }
                (PluginState::Running, line)
            }
            PluginStatus::Missing => (PluginState::Missing, "Not installed; kept with its settings".into()),
            PluginStatus::Crashed(why) => (PluginState::Crashed, format!("Crashed: {why}")),
            PluginStatus::Failed(why) => (PluginState::Crashed, format!("Could not start: {why}")),
        };
        Self { state, line, editor_open: report.editor_open, embedded: report.editor_embedded, ara: report.ara }
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
    pub nam: crate::detail::nam_panel::Lists,
}

/// What the detail area shows: a selected clip comes with its track's chain, since that chain
/// is what the clip plays through. The clip's properties are boxed: they are much larger than the
/// chain or nothing, and the detail is one value moved around each frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Devices(Chain),
    Clip(Box<ClipInfo>, Chain),
    Nothing,
}

impl Detail {
    /// While the transport runs, the panels show what automation plays at `beat`: every enabled
    /// lane of a device's parameter sets the value its knob, slider and text show (#706).
    pub fn follow_automation(&mut self, project: &Project, beat: f64) {
        let chain = match self {
            Self::Devices(chain) | Self::Clip(_, chain) => chain,
            Self::Nothing => return,
        };
        let Some(channel) = project
            .tracks
            .iter()
            .find(|t| t.id == chain.track)
            .and_then(|t| t.channel)
            .and_then(|c| project.channel(c))
        else {
            return;
        };
        fn follow(boxes: &mut [DeviceBox], devices: &[suhina_engine::model::Device], beat: f64) {
            for device_box in boxes {
                if let Some(group) = device_box.group.as_mut() {
                    for branch in &mut group.branches {
                        follow(&mut branch.devices, devices, beat);
                    }
                }
                let Some(device) = suhina_engine::model::device_at(devices, &device_box.at.0) else { continue };
                // A built-in parameter's description, for its kind and range.
                let info = match &device.kind {
                    DeviceKind::Builtin(id) => suhina_device_registry::info(id),
                    _ => None,
                };
                for lane in device.automation.iter().filter(|l| l.enabled && !l.points.is_empty()) {
                    let spec = info.and_then(|info| info.params.iter().find(|p| p.id == lane.param));
                    // Evenly along the parameter's control, as the engine plays it and the lane
                    // draws it.
                    use crate::arrangement::automation_lanes::Scale;
                    let scale = match spec {
                        Some(spec) => Some(Scale::Device(*spec)),
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
                    let Some(value) = lane.value_at(beat, scale.stepped(), &scale) else { continue };
                    let value = spec.map_or(value, |spec| spec.clamp(value));
                    for (id, v) in &mut device_box.values {
                        if *id == lane.param {
                            *v = value;
                        }
                    }
                    for (id, text) in &mut device_box.params {
                        if *id == lane.param {
                            *text = format_value(value);
                        }
                    }
                    if let Some(graph) = device_box.graph.as_mut() {
                        for parameter in graph.parameters.iter_mut().filter(|p| p.id == lane.param) {
                            let value = value.clamp(parameter.min, parameter.max);
                            // A plug-in's text is for the value it listed, not the one played.
                            if value != parameter.value {
                                parameter.text.clear();
                            }
                            parameter.value = value;
                        }
                    }
                }
            }
        }
        follow(&mut chain.devices, &channel.devices, beat);
    }
}

/// The detail for a selected clip (it wins, with its own track's chain) or track.
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<ClipRef>) -> Detail {
    build_with(project, track, clip, &mut |_, _| None, crate::detail::nam_panel::Lists::default())
}

/// [`build`], with each plug-in device's state from `plugins` (track, device path).
pub fn build_with(
    project: &Project,
    track: Option<TrackId>,
    clip: Option<ClipRef>,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
    nam: crate::detail::nam_panel::Lists,
) -> Detail {
    let bar = bar_beats(project);
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
                start: scene.map_or_else(|| bars_beats(placed.start, bar, 1), |s| (s + 1).to_string()),
                scene: scene.is_some(),
                length: bars_beats(placed.length, bar, 0),
                looping: placed.loop_range.is_some(),
                loop_range: placed.loop_range.map(|r| (r.start, r.end)),
                audio: audio.is_some(),
                gain_db: audio.map(|region| gain_to_db(region.gain)),
                warped: audio.is_some_and(|region| region.warped),
                stretch: audio.map(|region| region.stretch).unwrap_or_default(),
                transpose: audio.map_or(0.0, |region| region.transpose),
                at: Some(match scene {
                    Some(scene) => ClipRef::Slot(id, scene),
                    None => ClipRef::Arrangement(id, placed.id),
                }),
                fades: (placed.fade_in, placed.fade_out),
                markers: (placed.offset, placed.offset + placed.length),
                notes,
                selection: SelectionInfo::default(),
                switch: None,
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
    nam: crate::detail::nam_panel::Lists,
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
    Chain { track: id, name: track_name, devices, host, nam }
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
            info.map(|info| {
                info.params
                    .iter()
                    .map(|p| {
                        (
                            p.id.to_owned(),
                            d.params
                                .iter()
                                .find(|stored| stored.id == p.id)
                                .map_or(p.default, |stored| p.clamp(stored.value)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        },
        file_device: match &d.kind {
            DeviceKind::Builtin(id) if suhina_device_registry::is_file_device(id) => {
                suhina_device_registry::info(id).map(|info| info.id)
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
            let stored = d.params.iter().find(|v| suhina_engine::param_id(&v.id) == Some(p.id)).map(|v| v.value);
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
