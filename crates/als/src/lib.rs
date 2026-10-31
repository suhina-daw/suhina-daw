// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads Live Sets (`.als`) into the Suhina project model.
//!
//! The format is described in `docs/spec/als-format.md`, which was written from Live Sets we
//! authored ourselves. This reader covers what that spec covers: tempo, tracks, mixer, sends,
//! scenes, locators, MIDI and audio clips, plug-ins, and Live's devices that have a Suhina counterpart
//! (see `devices.rs`). Anything else is skipped or kept as a placeholder and reported in
//! [`Import::warnings`], so a project still opens with everything else intact.

mod devices;
mod scale;

use std::fmt;
use std::io::Read;
use std::path::Path;

use roxmltree::Node;
use suhina_model::{
    Arrangement, ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ClipId, ContentId,
    ContentKinds, ContentLane, Device, DeviceKind, Extension, Extensions, LaneData, LaneId, Launcher, LauncherColumn,
    LoopRange, MAX_CHANGE_BEAT, MAX_GROUP_DEPTH, MAX_TEMPO_CHANGES, MAX_TIME_SIGNATURE_CHANGES, Marker, Note, Project,
    Scene, SceneId, Send, Slot, StretchMode, TempoPoint, TimeSignature, TimeSignatureChange, Track, TrackId, Transport,
    ValidationError, WarpMarker, valid_bpm,
};

/// Upper bound for the decompressed XML, so a malicious file cannot exhaust memory.
const MAX_XML_BYTES: u64 = 512 * 1024 * 1024;
/// Live stores a send turned all the way down ("-inf" in the UI) as -70 dB.
const SEND_OFF: f64 = 0.000_316_3;

/// A project read from a Live Set, with notes about everything that was not imported.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub project: Project,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum AlsError {
    Io(std::io::Error),
    TooLarge,
    NotUtf8,
    Xml(roxmltree::Error),
    NotALiveSet,
    Invalid(Vec<ValidationError>),
}

impl fmt::Display for AlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read the Live Set: {e}"),
            Self::TooLarge => write!(f, "the Live Set is larger than {} MiB uncompressed", MAX_XML_BYTES / 1024 / 1024),
            Self::NotUtf8 => write!(f, "the Live Set is not valid UTF-8"),
            Self::Xml(e) => write!(f, "the Live Set is not valid XML: {e}"),
            Self::NotALiveSet => write!(f, "the file is not a Live Set"),
            Self::Invalid(errors) => {
                write!(f, "the imported project is inconsistent:")?;
                for e in errors {
                    write!(f, " {e};")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AlsError {}

impl From<std::io::Error> for AlsError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Reads a `.als` file from disk.
pub fn read_file(path: &Path) -> Result<Import, AlsError> {
    let file = std::fs::File::open(path)?;
    read_gzip(file)
}

/// Reads a gzip-compressed Live Set.
pub fn read_gzip(reader: impl Read) -> Result<Import, AlsError> {
    let mut xml = Vec::new();
    flate2::read::GzDecoder::new(reader).take(MAX_XML_BYTES + 1).read_to_end(&mut xml)?;
    if xml.len() as u64 > MAX_XML_BYTES {
        return Err(AlsError::TooLarge);
    }
    let xml = String::from_utf8(xml).map_err(|_| AlsError::NotUtf8)?;
    read_xml(&xml)
}

/// Reads an already decompressed Live Set.
pub fn read_xml(xml: &str) -> Result<Import, AlsError> {
    let doc = roxmltree::Document::parse(xml).map_err(AlsError::Xml)?;
    let root = doc.root_element();
    if !root.has_tag_name("Ableton") {
        return Err(AlsError::NotALiveSet);
    }
    let live_set = child(root, "LiveSet").ok_or(AlsError::NotALiveSet)?;
    let mut reader = Reader::default();
    let mut project = reader.live_set(live_set);
    // The set's own object ids are not Suhina's (ADR 0025): every lane, clip and scene gets a new
    // one, in document order. A set within the size cap holds far fewer objects than there are ids.
    project.assign_ids().map_err(|_| AlsError::TooLarge)?;
    project.validate().map_err(AlsError::Invalid)?;
    Ok(Import { project, warnings: reader.warnings })
}

/// A track as listed in the Live Set, with the ids it gets in the model.
struct TrackEntry<'a, 'input> {
    node: Node<'a, 'input>,
    kind: TrackKind,
    track: TrackId,
    channel: ChannelId,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackKind {
    Midi,
    Audio,
    Return,
    Group,
}

#[derive(Default)]
struct Reader {
    warnings: Vec<String>,
    contents: Vec<ClipContent>,
    /// Project tempo, for converting unwarped audio clips from seconds to beats.
    tempo_bpm: f64,
}

impl Reader {
    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn live_set(&mut self, live_set: Node) -> Project {
        let transport = self.transport(live_set);
        self.tempo_bpm = transport.tempo_bpm;

        // The main output gets channel 1; tracks are numbered from 1 and their channels from 2.
        let main_channel = ChannelId(1);
        let mut entries = Vec::new();
        if let Some(tracks) = child(live_set, "Tracks") {
            for node in tracks.children().filter(Node::is_element) {
                let kind = match node.tag_name().name() {
                    "MidiTrack" => TrackKind::Midi,
                    "AudioTrack" => TrackKind::Audio,
                    "ReturnTrack" => TrackKind::Return,
                    "GroupTrack" => TrackKind::Group,
                    other => {
                        self.warn(format!("skipped unknown track type {other}"));
                        continue;
                    }
                };
                let n = u32::try_from(entries.len()).unwrap_or(u32::MAX).saturating_add(1);
                let name = value(node, &["Name", "EffectiveName"]).unwrap_or_default().to_string();
                entries.push(TrackEntry {
                    node,
                    kind,
                    track: TrackId(n),
                    channel: ChannelId(n.saturating_add(1)),
                    name,
                });
            }
        }
        let returns: Vec<ChannelId> =
            entries.iter().filter(|e| e.kind == TrackKind::Return).map(|e| e.channel).collect();

        let mut channels = vec![self.main_channel(live_set, main_channel)];
        let mut tracks = Vec::new();
        let mut lanes = Vec::new();
        let mut columns = Vec::new();
        let scenes = self.scenes(live_set);

        for entry in &entries {
            let parent = self.parent(entry, &entries);
            let parent_channel = parent.and_then(|p| entries.iter().find(|e| e.track == p)).map(|e| e.channel);
            let mut channel = self.channel(entry, &returns, parent_channel);
            channel.devices = self.devices(entry.node, &entry.name);
            channels.push(channel);
            tracks.push(Track {
                id: entry.track,
                name: entry.name.clone(),
                color: None,
                content: ContentKinds { notes: entry.kind == TrackKind::Midi, audio: entry.kind == TrackKind::Audio },
                channel: Some(entry.channel),
                parent,
                midi_input: Default::default(),
                audio_input: Default::default(),
                extensions: Extensions::default(),
            });
            if entry.kind != TrackKind::Return {
                lanes.push(ArrangementLane {
                    id: LaneId::NONE,
                    name: entry.name.clone(),
                    track: Some(entry.track),
                    clips: self.arrangement_clips(entry),
                });
            }
            if matches!(entry.kind, TrackKind::Midi | TrackKind::Audio) {
                columns.push(LauncherColumn { track: entry.track, slots: self.slots(entry, scenes.len()) });
            }
        }

        let mut project = Project {
            transport,
            tracks,
            channels,
            contents: std::mem::take(&mut self.contents),
            arrangement: Arrangement { lanes, markers: self.markers(live_set), takes: Vec::new() },
            launcher: Launcher { scenes, columns, mappings: Vec::new() },
            extensions: Extensions::default(),
            default_scale: None,
            next_id: 0,
        };
        self.untangle_groups(&mut project);
        project
    }

    /// Takes out of its group a track whose groups lead back to it, or that sits deeper than
    /// Suhina nests groups ([`MAX_GROUP_DEPTH`]), so the set still opens: the track moves to the
    /// top level and plays to the main output instead of into that group.
    fn untangle_groups(&mut self, project: &mut Project) {
        for _ in 0..project.tracks.len() {
            let tangled = project
                .tracks
                .iter()
                .find(|t| project.group_depth(t.id).is_none_or(|depth| depth > MAX_GROUP_DEPTH))
                .map(|t| (t.id, t.name.clone(), t.channel, t.parent));
            let Some((id, name, channel, parent)) = tangled else { return };
            let deep = project.group_depth(id).is_some();
            let group_channel = parent.and_then(|p| project.track(p)).and_then(|t| t.channel);
            if let Some(track) = project.track_mut(id) {
                track.parent = None;
            }
            if let Some(c) = project.channels.iter_mut().find(|c| Some(c.id) == channel)
                && c.output.is_some()
                && c.output == group_channel
            {
                c.output = None;
            }
            self.warn(if deep {
                format!("track {name}: groups nest more than {MAX_GROUP_DEPTH} deep; moved out of its group")
            } else {
                format!("track {name}: its groups contain each other; moved out of its group")
            });
        }
    }

    fn transport(&mut self, live_set: Node) -> Transport {
        let mut transport = Transport::default();
        self.loop_region(live_set, &mut transport);
        let main = child(live_set, "MainTrack").or_else(|| child(live_set, "MasterTrack"));
        let Some((main, mixer)) = main.and_then(|m| path(m, &["DeviceChain", "Mixer"]).map(|mixer| (m, mixer))) else {
            self.warn("no main track found; using 120 bpm in 4/4");
            return transport;
        };
        match parameter_value(main, mixer, "Tempo").and_then(parse_number) {
            Some(bpm) if (1.0..=999.0).contains(&bpm) => transport.tempo_bpm = bpm,
            _ => self.warn("tempo missing or out of range; using 120 bpm"),
        }
        match parameter_value(main, mixer, "TimeSignature") {
            None => {}
            Some(code) => match code.parse::<u32>().ok().and_then(time_signature) {
                Some(signature) => transport.time_signature = signature,
                None => self.warn(format!("time signature code {code} is not understood; using 4/4")),
            },
        }
        self.tempo_changes(main, mixer, &mut transport);
        self.time_signature_changes(main, mixer, &mut transport);
        transport
    }

    /// The arrangement's loop region and whether it is on (`docs/spec/als-format.md` ›
    /// Arrangement loop). A region that is missing a value or does not end after it starts is
    /// left out with a warning.
    fn loop_region(&mut self, live_set: Node, transport: &mut Transport) {
        let Some(node) = child(live_set, "Transport") else { return };
        let start = number(node, &["LoopStart"]);
        let length = number(node, &["LoopLength"]);
        if start.is_none() && length.is_none() {
            return;
        }
        let range = start.zip(length).map(|(start, length)| LoopRange { start, end: start + length });
        match range.filter(LoopRange::is_valid_region) {
            Some(range) => {
                transport.loop_range = Some(range);
                transport.loop_enabled = boolean(node, &["LoopOn"]).unwrap_or(false);
            }
            None => self.warn("skipped the arrangement loop: its start or length is missing or not usable"),
        }
    }

    /// The tempo envelope's breakpoints (`docs/spec/als-format.md` › Tempo and time-signature
    /// changes): one at or before beat 0 sets the start tempo; each later one is a change, a
    /// step when it shares its time with the breakpoint before, otherwise a ramp from it.
    fn tempo_changes(&mut self, main: Node, mixer: Node, transport: &mut Transport) {
        let mut previous: Option<f64> = None;
        for (time, text) in parameter_events(main, mixer, "Tempo") {
            let Some(bpm) = parse_number(text).filter(|bpm| valid_bpm(*bpm)) else {
                self.warn(format!("tempo {text} at beat {time} is out of range; left out"));
                continue;
            };
            if time <= 0.0 {
                transport.tempo_bpm = bpm;
            } else if time > MAX_CHANGE_BEAT || transport.tempo_changes.len() >= MAX_TEMPO_CHANGES {
                self.warn(format!("tempo change at beat {time} is past what a project keeps; left out"));
                continue;
            } else {
                // A breakpoint on the time of the one before is a jump. After a glide it is kept
                // beside it (the glide arrives, then the tempo jumps); after a jump it replaces it.
                let ramp = previous.is_some_and(|p| p < time);
                match transport.tempo_changes.last_mut().filter(|p| p.beat == time) {
                    Some(last) if !last.ramp => *last = TempoPoint { beat: time, bpm, ramp: false },
                    _ => transport.tempo_changes.push(TempoPoint { beat: time, bpm, ramp }),
                }
            }
            previous = Some(time.max(0.0));
        }
    }

    /// The time-signature envelope's breakpoints: one at or before beat 0 sets the start
    /// signature; a later one on a bar line changes it from that bar. One inside a bar is left
    /// out, since a change in Suhina starts a bar.
    fn time_signature_changes(&mut self, main: Node, mixer: Node, transport: &mut Transport) {
        for (time, text) in parameter_events(main, mixer, "TimeSignature") {
            let Some(signature) = text.parse::<u32>().ok().and_then(time_signature).filter(|t| t.is_valid()) else {
                self.warn(format!("time signature code {text} at beat {time} is not understood; left out"));
                continue;
            };
            if time <= 0.0 {
                transport.time_signature = signature;
                continue;
            }
            let here = transport.meter().position(time + 1e-6);
            if (here.bar_start - time).abs() > 1e-6 || here.bar == 0 {
                self.warn(format!("time signature change at beat {time} is not on a bar line; left out"));
                continue;
            }
            if transport.time_signature_changes.len() >= MAX_TIME_SIGNATURE_CHANGES {
                self.warn("more time signature changes than a project keeps; the rest are left out");
                break;
            }
            transport.time_signature_changes.retain(|c| c.bar != here.bar);
            transport.time_signature_changes.push(TimeSignatureChange { bar: here.bar, time_signature: signature });
        }
    }

    fn main_channel(&mut self, live_set: Node, id: ChannelId) -> Channel {
        let mixer = path(live_set, &["MainTrack", "DeviceChain", "Mixer"])
            .or_else(|| path(live_set, &["MasterTrack", "DeviceChain", "Mixer"]));
        let mut channel = empty_channel(id, "Main", ChannelRole::Master);
        if let Some(mixer) = mixer {
            self.mixer(mixer, &mut channel, "Main");
        }
        if let Some(main) = child(live_set, "MainTrack").or_else(|| child(live_set, "MasterTrack")) {
            channel.devices = self.devices(main, "Main");
        }
        channel
    }

    fn channel(&mut self, entry: &TrackEntry, returns: &[ChannelId], parent_channel: Option<ChannelId>) -> Channel {
        let role = match entry.kind {
            TrackKind::Return => ChannelRole::Effect,
            TrackKind::Group => ChannelRole::Submix,
            TrackKind::Midi | TrackKind::Audio => ChannelRole::Regular,
        };
        let mut channel = empty_channel(entry.channel, &entry.name, role);
        let Some(mixer) = path(entry.node, &["DeviceChain", "Mixer"]) else {
            self.warn(format!("track {}: no mixer found", entry.name));
            return channel;
        };
        self.mixer(mixer, &mut channel, &entry.name);

        if let Some(sends) = child(mixer, "Sends") {
            for (holder, target) in sends.children().filter(|n| n.has_tag_name("TrackSendHolder")).zip(returns) {
                let level = number(holder, &["Send", "Manual"]).unwrap_or(0.0);
                if level > SEND_OFF {
                    channel.sends.push(Send { target: *target, level, pre_fader: false });
                }
            }
        }

        match value(entry.node, &["DeviceChain", "AudioOutputRouting", "Target"]) {
            Some("AudioOut/Main") | None => {}
            Some("AudioOut/GroupTrack") => match parent_channel {
                Some(group) => channel.output = Some(group),
                None => {
                    self.warn(format!("track {}: routed to a group but is in none; using the main output", entry.name))
                }
            },
            Some(other) => {
                self.warn(format!("track {}: output {other} is not supported yet; using the main output", entry.name))
            }
        }
        channel
    }

    fn mixer(&mut self, mixer: Node, channel: &mut Channel, name: &str) {
        match number(mixer, &["Volume", "Manual"]) {
            Some(v) if v >= 0.0 => channel.volume = v,
            Some(_) => self.warn(format!("{name}: negative volume ignored")),
            None => {}
        }
        if let Some(pan) = number(mixer, &["Pan", "Manual"]) {
            channel.pan = pan.clamp(-1.0, 1.0);
        }
        if let Some(on) = boolean(mixer, &["Speaker", "Manual"]) {
            channel.mute = !on;
        }
    }

    fn parent(&mut self, entry: &TrackEntry, entries: &[TrackEntry]) -> Option<TrackId> {
        let group = value(entry.node, &["TrackGroupId"])?;
        if group == "-1" {
            return None;
        }
        let found = entries.iter().find(|e| e.kind == TrackKind::Group && e.node.attribute("Id") == Some(group));
        if found.is_none() {
            self.warn(format!("track {}: group {group} not found", entry.name));
        }
        found.map(|e| e.track)
    }

    /// Reads a track's device chain. Plug-ins keep their state verbatim; Live's own devices
    /// become placeholders.
    fn devices(&mut self, track: Node, track_name: &str) -> Vec<Device> {
        let Some(devices) = path(track, &["DeviceChain", "DeviceChain", "Devices"]) else { return Vec::new() };
        devices.children().filter(Node::is_element).filter_map(|node| self.device(node, track_name)).collect()
    }

    fn device(&mut self, node: Node, track_name: &str) -> Option<Device> {
        let tag = node.tag_name().name();
        let enabled = boolean(node, &["On", "Manual"]).unwrap_or(true);
        let user_name = value(node, &["UserName"]).filter(|n| !n.is_empty());
        let mut extensions = Extensions::default();

        if let Some((id, name, params)) = devices::builtin(node, &mut |w| self.warn(format!("track {track_name}: {w}")))
        {
            return Some(Device {
                name: user_name.unwrap_or(name).to_string(),
                kind: DeviceKind::Builtin(id.into()),
                enabled,
                state: None,
                params,
                automation: Vec::new(),
                clip_host: false,
                extensions,
            });
        }
        if !node.has_tag_name("PluginDevice") {
            self.warn(format!("track {track_name}: Live device {tag} is imported as a placeholder"));
            return Some(Device {
                name: user_name.unwrap_or(tag).to_string(),
                kind: DeviceKind::Foreign { format: "als".into(), id: tag.into() },
                enabled,
                state: None,
                params: Vec::new(),
                automation: Vec::new(),
                clip_host: false,
                extensions,
            });
        }

        let info = path(node, &["PluginDesc"]).and_then(|d| d.children().find(Node::is_element));
        let Some(info) = info else {
            self.warn(format!("track {track_name}: plug-in without a description skipped"));
            return None;
        };
        let (kind, plugin_name, state) = match info.tag_name().name() {
            "Vst3PluginInfo" => {
                let Some(id) = vst3_uid(info) else {
                    self.warn(format!("track {track_name}: VST3 plug-in without a valid id skipped"));
                    return None;
                };
                let preset = path(info, &["Preset", "Vst3Preset"]);
                let state = preset.and_then(|p| self.hex_state(p, "ProcessorState", track_name));
                if let Some(controller) = preset.and_then(|p| self.hex_state(p, "ControllerState", track_name)) {
                    extensions.0.push(Extension {
                        format: "als".into(),
                        key: "Vst3ControllerState".into(),
                        data: controller,
                    });
                }
                (DeviceKind::Vst3 { id }, value(info, &["Name"]), state)
            }
            "VstPluginInfo" => {
                let Some(id) = value(info, &["UniqueId"]).and_then(|v| v.parse::<i64>().ok()) else {
                    self.warn(format!("track {track_name}: VST2 plug-in without an id skipped"));
                    return None;
                };
                // Accept a signed or unsigned decimal and keep the 32 bits; only positive ids
                // are in the corpus so far (docs/spec/als-format.md).
                let id = id as u32;
                let preset = path(info, &["Preset", "VstPreset"]);
                let buffer = preset.and_then(|p| self.hex_state(p, "Buffer", track_name));
                let state = match (preset, buffer) {
                    (Some(preset), Some(buffer)) => {
                        match vst2_preset(preset, id).and_then(|p| suhina_plugins::vst2::bank(&p, &buffer)) {
                            Ok(bank) => Some(bank),
                            Err(why) => {
                                // Keep the bytes as they were, so nothing is lost.
                                self.warn(format!("track {track_name}: VST2 state not read ({why}); kept as it was"));
                                extensions.0.push(Extension {
                                    format: "als".into(),
                                    key: "VstPresetBuffer".into(),
                                    data: buffer,
                                });
                                None
                            }
                        }
                    }
                    _ => None,
                };
                if let Some(plugin_path) = value(info, &["Path"]).filter(|p| !p.is_empty()) {
                    extensions.0.push(Extension {
                        format: "als".into(),
                        key: "VstPath".into(),
                        data: plugin_path.as_bytes().to_vec(),
                    });
                }
                (DeviceKind::Vst2 { id }, value(info, &["PlugName"]), state)
            }
            other => {
                self.warn(format!("track {track_name}: plug-in type {other} is not imported yet"));
                return None;
            }
        };
        Some(Device {
            name: user_name.or(plugin_name).unwrap_or("Plug-in").to_string(),
            kind,
            enabled,
            state,
            params: Vec::new(),
            automation: Vec::new(),
            clip_host: false,
            extensions,
        })
    }

    /// Decodes a hex-encoded state blob such as a plug-in's `Buffer` or `ProcessorState`.
    fn hex_state(&mut self, preset: Node, element: &str, track_name: &str) -> Option<Vec<u8>> {
        let text: String = child(preset, element)?.text()?.chars().filter(|c| !c.is_whitespace()).collect();
        if text.is_empty() {
            return None;
        }
        let decoded = decode_hex(&text);
        if decoded.is_none() {
            self.warn(format!("track {track_name}: plug-in {element} is not valid hex; state dropped"));
        }
        decoded
    }

    fn scenes(&mut self, live_set: Node) -> Vec<Scene> {
        let Some(scenes) = child(live_set, "Scenes") else { return Vec::new() };
        scenes
            .children()
            .filter(|n| n.has_tag_name("Scene"))
            .map(|scene| Scene {
                id: SceneId::NONE,
                name: value(scene, &["Name"]).unwrap_or_default().to_string(),
                tempo_bpm: if boolean(scene, &["IsTempoEnabled"]) == Some(true) {
                    number(scene, &["Tempo"]).filter(|bpm| (1.0..=999.0).contains(bpm))
                } else {
                    None
                },
                // The scene's time signature element is not recorded in the spec yet.
                time_signature: None,
            })
            .collect()
    }

    /// The set's locators as markers, in beat order (`docs/spec/als-format.md` › Locators). One
    /// without a usable time is left out with a warning.
    fn markers(&mut self, live_set: Node) -> Vec<Marker> {
        let Some(locators) = path(live_set, &["Locators", "Locators"]) else { return Vec::new() };
        let mut arrangement = Arrangement::default();
        for locator in locators.children().filter(|n| n.has_tag_name("Locator")) {
            let name = value(locator, &["Name"]).unwrap_or_default();
            match number(locator, &["Time"]).filter(|beat| *beat >= 0.0) {
                Some(beat) => {
                    arrangement.insert_marker(Marker::new(beat, name));
                }
                None => self.warn(format!("skipped the locator {name:?}: its time is missing or before the start")),
            }
        }
        arrangement.markers
    }

    fn arrangement_clips(&mut self, entry: &TrackEntry) -> Vec<Clip> {
        // MIDI tracks keep arrangement clips under ClipTimeable, audio tracks under Sample.
        let mut clips = Vec::new();
        for timeable in ["ClipTimeable", "Sample"] {
            let Some(events) =
                path(entry.node, &["DeviceChain", "MainSequencer", timeable, "ArrangerAutomation", "Events"])
            else {
                continue;
            };
            for node in events.children().filter(Node::is_element) {
                if let Some(clip) = self.clip(node, true, &entry.name) {
                    clips.push(clip);
                }
            }
        }
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        clips
    }

    fn slots(&mut self, entry: &TrackEntry, scenes: usize) -> Vec<Slot> {
        let mut slots = Vec::new();
        if let Some(list) = path(entry.node, &["DeviceChain", "MainSequencer", "ClipSlotList"]) {
            for slot in list.children().filter(|n| n.has_tag_name("ClipSlot")) {
                let clip = path(slot, &["ClipSlot", "Value"])
                    .and_then(|v| v.children().find(Node::is_element))
                    .and_then(|node| self.clip(node, false, &entry.name));
                let has_stop = boolean(slot, &["HasStop"]).unwrap_or(true);
                slots.push(Slot { clip, has_stop });
            }
        }
        if slots.len() > scenes {
            self.warn(format!(
                "track {}: {} clip slots but {scenes} scenes; extra slots dropped",
                entry.name,
                slots.len()
            ));
            slots.truncate(scenes);
        }
        slots.resize_with(scenes, || Slot { clip: None, has_stop: true });
        slots
    }

    /// Reads one clip into the content pool and returns its placement.
    fn clip(&mut self, node: Node, arrangement: bool, track: &str) -> Option<Clip> {
        let name = value(node, &["Name"]).unwrap_or_default().to_string();
        let mut scale = None;
        let data = match node.tag_name().name() {
            "MidiClip" => {
                scale = scale::clip_scale(node).unwrap_or_else(|reason| {
                    self.warn(format!("track {track}: clip {name:?} imported without a scale: {reason}"));
                    None
                });
                LaneData::Notes(self.notes(node, &name, track))
            }
            "AudioClip" => LaneData::Audio(self.audio(node, &name, track)?),
            other => {
                self.warn(format!("track {track}: {other} is not imported yet"));
                return None;
            }
        };
        // An unwarped audio clip keeps its loop region in seconds (docs/spec/als-format.md).
        let unwarped = matches!(&data, LaneData::Audio(audio) if !audio.warped);
        let beats_per_second = self.tempo_bpm / 60.0;
        let loop_time = |names: &[&str]| number(node, names).map(|t| if unwarped { t * beats_per_second } else { t });

        let loop_on = boolean(node, &["Loop", "LoopOn"]).unwrap_or(false);
        let loop_start = loop_time(&["Loop", "LoopStart"]).unwrap_or(0.0);
        let loop_end = loop_time(&["Loop", "LoopEnd"]).unwrap_or(loop_start);
        let start_relative = loop_time(&["Loop", "StartRelative"]).unwrap_or(0.0);
        let out_marker = number(node, &["Loop", "OutMarker"]).unwrap_or(loop_end);
        let id = ContentId(u32::try_from(self.contents.len()).unwrap_or(u32::MAX).saturating_add(1));
        self.contents.push(ClipContent {
            id,
            name: name.clone(),
            length: loop_end.max(out_marker).max(0.0),
            lanes: vec![ContentLane { target: None, data }],
            extensions: Extensions::default(),
            scale,
        });

        let (offset, loop_range) = if loop_on && loop_end > loop_start {
            (loop_start + start_relative, Some(LoopRange { start: loop_start, end: loop_end }))
        } else {
            (loop_start, None)
        };
        let (start, length) = if arrangement {
            let start = node.attribute("Time").and_then(parse_number).unwrap_or(0.0);
            let current_start = number(node, &["CurrentStart"]).unwrap_or(start);
            let current_end = number(node, &["CurrentEnd"]).unwrap_or(current_start);
            (start, current_end - current_start)
        } else if let Some(range) = loop_range {
            (0.0, range.end - range.start)
        } else {
            (0.0, loop_end - loop_start)
        };
        if !(length.is_finite() && length > 0.0) {
            self.warn(format!("track {track}: clip {name:?} has no length; skipped"));
            return None;
        }

        Some(Clip {
            id: ClipId::NONE,
            content: id,
            start,
            length,
            offset,
            loop_range,
            enabled: boolean(node, &["Disabled"]) != Some(true),
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
            follow_action: Default::default(),
            launch: Default::default(),
            color: None,
            ara: false,
            extensions: Extensions::default(),
        })
    }

    fn audio(&mut self, clip: Node, clip_name: &str, track: &str) -> Option<AudioRegion> {
        let file_ref = path(clip, &["SampleRef", "FileRef"]);
        let relative = file_ref.and_then(|f| value(f, &["RelativePath"])).filter(|p| !p.is_empty());
        let absolute = file_ref.and_then(|f| value(f, &["Path"])).filter(|p| !p.is_empty());
        let Some(file) = relative.or(absolute) else {
            self.warn(format!("track {track}: audio clip {clip_name:?} has no file; skipped"));
            return None;
        };
        let sample_rate = value(clip, &["SampleRef", "DefaultSampleRate"])
            .and_then(|r| r.parse::<u32>().ok())
            .filter(|r| (1..=1_000_000).contains(r));
        let frames = number(clip, &["SampleRef", "DefaultDuration"]).filter(|f| *f >= 0.0);
        let duration_seconds = match (frames, sample_rate) {
            (Some(frames), Some(rate)) => Some(frames / f64::from(rate)),
            _ => None,
        };

        let mut warp_markers: Vec<WarpMarker> = path(clip, &["WarpMarkers"])
            .map(|markers| {
                markers
                    .children()
                    .filter(|n| n.has_tag_name("WarpMarker"))
                    .filter_map(|m| {
                        let time = m.attribute("BeatTime").and_then(parse_number)?;
                        let audio_time = m.attribute("SecTime").and_then(parse_number)?;
                        Some(WarpMarker { time, audio_time })
                    })
                    .collect()
            })
            .unwrap_or_default();
        warp_markers.sort_by(|a, b| a.time.total_cmp(&b.time));

        let coarse = number(clip, &["PitchCoarse"]).unwrap_or(0.0);
        let fine = number(clip, &["PitchFine"]).unwrap_or(0.0);
        let stretch = match value(clip, &["WarpMode"]) {
            // Observed: Live showed "Complex" for a clip saved with WarpMode 4 (docs/spec/als-format.md).
            Some("4") | None => StretchMode::Smooth,
            Some(code) => {
                self.warn(format!(
                    "track {track}: audio clip {clip_name:?} uses warp mode {code}, not mapped yet; using Smooth"
                ));
                StretchMode::Smooth
            }
        };
        Some(AudioRegion {
            file: file.to_string(),
            absolute_file: absolute.map(str::to_string),
            sample_rate,
            channels: None,
            duration_seconds,
            warped: boolean(clip, &["IsWarped"]).unwrap_or(true),
            stretch,
            warp_markers,
            gain: number(clip, &["SampleVolume"]).filter(|g| *g >= 0.0).unwrap_or(1.0),
            transpose: coarse + fine / 100.0,
            stream: None,
        })
    }

    fn notes(&mut self, clip: Node, clip_name: &str, track: &str) -> Vec<Note> {
        let mut notes = Vec::new();
        let mut skipped = 0usize;
        let Some(key_tracks) = path(clip, &["Notes", "KeyTracks"]) else { return notes };
        for key_track in key_tracks.children().filter(|n| n.has_tag_name("KeyTrack")) {
            let Some(key) = value(key_track, &["MidiKey"]).and_then(|k| k.parse::<u8>().ok()).filter(|k| *k <= 127)
            else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            let Some(events) = path(key_track, &["Notes"]) else { continue };
            for event in events.children().filter(|n| n.has_tag_name("MidiNoteEvent")) {
                // A deactivated note is kept and read as muted, not dropped
                // (docs/spec/als-format.md › Notes).
                let muted = event.attribute("IsEnabled") == Some("false");
                let time = event.attribute("Time").and_then(parse_number);
                let duration = event.attribute("Duration").and_then(parse_number);
                let (Some(time), Some(duration)) = (time, duration) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                let velocity = event.attribute("Velocity").and_then(parse_number).unwrap_or(100.0);
                let release = event.attribute("OffVelocity").and_then(parse_number).unwrap_or(64.0);
                notes.push(Note {
                    time,
                    duration: duration.max(0.0),
                    key,
                    velocity: (velocity / 127.0).clamp(0.0, 1.0) as f32,
                    release_velocity: (release / 127.0).clamp(0.0, 1.0) as f32,
                    channel: 0,
                    muted,
                });
            }
        }
        if skipped > 0 {
            self.warn(format!("track {track}: clip {clip_name:?}: {skipped} malformed notes skipped"));
        }
        notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.key.cmp(&b.key)));
        notes
    }
}

/// A VST3 class id from Live's four signed 32-bit `Fields`, as 32 upper-case hex digits.
fn vst3_uid(info: Node) -> Option<String> {
    let uid = child(info, "Uid")?;
    let mut hex = String::with_capacity(32);
    for i in 0..4 {
        let field = value(uid, &[&format!("Fields.{i}")])?.parse::<i64>().ok()?;
        hex.push_str(&format!("{:08X}", field as u32));
    }
    Some(hex)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs.iter().map(|[hi, lo]| Some((hex_digit(*hi)? << 4) | hex_digit(*lo)?)).collect()
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// The value of a main-track parameter such as `Tempo` or `TimeSignature`. Live keeps the
/// current value in the default event (time `-63072000`) of the parameter's automation envelope;
/// for the time signature the parameter's own `Manual` value is not updated, so the envelope wins.
fn parameter_value<'a>(main: Node<'a, '_>, mixer: Node<'a, '_>, name: &str) -> Option<&'a str> {
    let parameter = child(mixer, name)?;
    let from_envelope = parameter
        .children()
        .find(|n| n.has_tag_name("AutomationTarget"))
        .and_then(|target| target.attribute("Id"))
        .and_then(|target| {
            path(main, &["AutomationEnvelopes", "Envelopes"])?
                .children()
                .filter(|n| n.has_tag_name("AutomationEnvelope"))
                .find(|env| value(*env, &["EnvelopeTarget", "PointeeId"]) == Some(target))
        })
        .and_then(|env| path(env, &["Automation", "Events"]))
        .and_then(|events| events.children().find(|e| e.is_element() && e.attribute("Time") == Some("-63072000")))
        .and_then(|event| event.attribute("Value"));
    from_envelope.or_else(|| value(parameter, &["Manual"]))
}

/// The breakpoints of a main-track parameter's automation envelope, after the default event: each
/// as its time in beats and its `Value`, in time order (breakpoints on one time keep the file's
/// order). Events that are not finite are left out; at most [`MAX_ENVELOPE_EVENTS`] are read.
fn parameter_events<'a>(main: Node<'a, '_>, mixer: Node<'a, '_>, name: &str) -> Vec<(f64, &'a str)> {
    let Some(target) = child(mixer, name)
        .and_then(|p| p.children().find(|n| n.has_tag_name("AutomationTarget")))
        .and_then(|t| t.attribute("Id"))
    else {
        return Vec::new();
    };
    let events = path(main, &["AutomationEnvelopes", "Envelopes"])
        .into_iter()
        .flat_map(|envs| envs.children())
        .filter(|n| n.has_tag_name("AutomationEnvelope"))
        .find(|env| value(*env, &["EnvelopeTarget", "PointeeId"]) == Some(target))
        .and_then(|env| path(env, &["Automation", "Events"]));
    let Some(events) = events else { return Vec::new() };
    let mut found: Vec<(f64, &str)> = events
        .children()
        .filter(|e| e.is_element() && e.attribute("Time") != Some("-63072000"))
        .take(MAX_ENVELOPE_EVENTS)
        .filter_map(|e| {
            let time = e.attribute("Time").and_then(parse_number)?;
            Some((time, e.attribute("Value")?))
        })
        .filter(|(time, _)| time.is_finite() && *time > -63_072_000.0)
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found
}

/// Most breakpoints read from one envelope, so a hostile file cannot make the import unbounded.
const MAX_ENVELOPE_EVENTS: usize = 100_000;

/// Live's time-signature code: `99 * log2(denominator) + numerator - 1` (4/4 is 201, 7/8 is 303).
fn time_signature(code: u32) -> Option<TimeSignature> {
    let numerator = u8::try_from(code % 99 + 1).ok()?;
    let denominator = u8::try_from(2u32.checked_pow(code / 99)?).ok()?;
    (denominator <= 64).then_some(TimeSignature { numerator, denominator })
}

fn empty_channel(id: ChannelId, name: &str, role: ChannelRole) -> Channel {
    Channel {
        id,
        name: name.to_string(),
        role,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
        automation: Vec::new(),
    }
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|n| n.has_tag_name(name))
}

pub(crate) fn path<'a, 'input>(node: Node<'a, 'input>, names: &[&str]) -> Option<Node<'a, 'input>> {
    names.iter().try_fold(node, |n, name| child(n, name))
}

/// The `Value` attribute of the element at `names` below `node`.
pub(crate) fn value<'a>(node: Node<'a, '_>, names: &[&str]) -> Option<&'a str> {
    path(node, names)?.attribute("Value")
}

pub(crate) fn number(node: Node, names: &[&str]) -> Option<f64> {
    value(node, names).and_then(parse_number)
}

pub(crate) fn boolean(node: Node, names: &[&str]) -> Option<bool> {
    match value(node, names)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// What `VstPreset` says about its `Buffer` besides the bytes.
fn vst2_preset(preset: Node, id: u32) -> Result<suhina_plugins::vst2::Preset, String> {
    let int = |name: &str| value(preset, &[name]).and_then(|v| v.trim().parse::<i64>().ok());
    let count = |name: &str| int(name).and_then(|n| usize::try_from(n).ok()).ok_or(format!("no {name}"));
    let kind = int("Type").ok_or("no Type")? as u32;
    // `ByteOrder` 2 is little-endian, the only value seen so far (docs/spec/als-format.md).
    let little_endian = match int("ByteOrder") {
        Some(2) | None => true,
        Some(1) => false,
        Some(other) => return Err(format!("unknown ByteOrder {other}")),
    };
    Ok(suhina_plugins::vst2::Preset {
        kind,
        id,
        version: int("PluginVersion").unwrap_or(1) as i32,
        programs: count("ProgramCount")?,
        current: int("ProgramNumber").unwrap_or(0) as i32,
        parameters: count("ParameterCount")?,
        little_endian,
    })
}
