// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the Session view shows, derived from the project and the engine's monitor: columns
//! (tracks, then return tracks), scenes and the state of every slot. Pure data, so the view and
//! its tests share one source of truth.

use suhina_engine::model::{
    ChannelRole, LaunchSettings, MappingTarget, Meter, Project, Rgb, TimeSignature, Track, TrackId,
};
use suhina_engine::{LauncherMonitor, PlayingMeter, Queued};

use crate::kit::theme;

/// The colours tracks without one of their own take in turn: the theme's first four track
/// colours (`track.1` to `track.4`).
pub fn track_palette() -> [Rgb; 4] {
    [theme::track_color_1(), theme::track_color_2(), theme::track_color_3(), theme::track_color_4()].map(to_rgb)
}

/// The default colour of the track at `index` among the tracks.
pub fn default_track_colour(index: usize) -> Rgb {
    let palette = track_palette();
    palette.get(index % palette.len()).copied().unwrap_or(Rgb(0x80, 0x80, 0x80))
}

/// A theme colour as a project's track colour.
pub fn to_rgb(colour: egui::Color32) -> Rgb {
    Rgb(colour.r(), colour.g(), colour.b())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// A track with clips (notes or audio).
    Track,
    /// A group track; its slots launch the tracks inside it.
    Group,
    /// A return track, fed by sends; it has no clip slots.
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// No clip and no stop button.
    Empty,
    /// No clip; launching it stops the track.
    EmptyStop,
    Stopped,
    /// Starts at the next quantisation point.
    Queued,
    Playing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub state: SlotState,
    /// The clip's name; empty for an empty slot or an unnamed clip.
    pub name: String,
    /// A deactivated clip does not play when launched.
    pub enabled: bool,
    /// The clip's loop length in beats, for its progress while playing.
    pub length: f64,
    /// The clip's launch settings; `None` for an empty slot.
    pub launch: Option<LaunchSettings>,
    /// The clip's own colour; `None` takes its track's.
    pub color: Option<Rgb>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub track: TrackId,
    pub name: String,
    pub color: Rgb,
    pub kind: ColumnKind,
    /// The track plays notes: a double-click on an empty slot makes a MIDI clip there.
    pub notes: bool,
    /// Index into the launcher's columns and the monitor's, if the track has slots.
    pub launcher: Option<usize>,
    /// One per scene; empty for a return track.
    pub slots: Vec<Slot>,
    /// The track's stop is queued for the next quantisation point.
    pub stop_queued: bool,
    pub mute: bool,
    pub solo: bool,
    /// Also heard on the cue output (`track.setCue`).
    pub cue: bool,
    /// The track plays notes, so it can be armed to play a MIDI controller (ADR 0017), and
    /// whether it is: the session knows, not the project, so [`TrackGrid::build`] leaves it false.
    pub armable: bool,
    pub armed: bool,
    /// The scene whose slot records a take now (`docs/spec/recording.md`); set by the app.
    pub recording: Option<usize>,
    /// What the track hears while armed.
    pub midi_input: suhina_engine::model::MidiInput,
    /// The track plays notes, or audio: which of its inputs it has.
    pub plays_notes: bool,
    pub plays_audio: bool,
    pub audio_input: suhina_engine::model::AudioInput,
    pub volume_db: f64,
    /// The track's place in the project's track order, as `track.move` counts it, and how many
    /// groups it is inside.
    pub position: usize,
    pub depth: usize,
    /// The group the track is directly in.
    pub parent: Option<TrackId>,
    /// The group track this track plays into; `None` is the main output (`track.setOutput`).
    pub output: Option<TrackId>,
    /// -1 is full left, 1 full right.
    pub pan: f64,
    /// One per return track, in their order, for a track or group column; empty for a return.
    pub sends: Vec<SendKnob>,
    /// The track's devices and plug-ins in chain order, for the channel strip's list.
    pub devices: Vec<StripDevice>,
}

/// A device in a channel strip's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripDevice {
    pub name: String,
    pub enabled: bool,
    /// A plug-in, run outside Suhina; otherwise a built-in effect or instrument.
    pub plugin: bool,
    /// For a device group: how many devices it holds, at every depth.
    pub group: Option<usize>,
}

/// A track's send to one return track, as the mixer strip shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SendKnob {
    /// The return track.
    pub target: TrackId,
    pub name: String,
    /// Minus infinity without a send.
    pub level_db: f64,
    pub pre_fader: bool,
}

/// What a channel's enabled automation plays at `beat`: its volume in dB and its pan, each when
/// it has a lane with points (#706).
pub fn played_mixer(channel: &suhina_engine::model::Channel, beat: f64) -> (Option<f64>, Option<f64>) {
    let lane = |param: &str| channel.automation.iter().find(|l| l.param == param && l.enabled && !l.points.is_empty());
    (
        lane("volume")
            .and_then(|l| l.value_at(beat, false, &suhina_engine::model::automation::VolumeTaper))
            .map(gain_to_db),
        lane("pan").and_then(|l| l.value_at(beat, false, &suhina_engine::model::automation::Plain)),
    )
}

/// A channel's devices as its strip's list shows them.
fn strip_devices(devices: &[suhina_engine::model::Device]) -> Vec<StripDevice> {
    devices
        .iter()
        .map(|d| StripDevice {
            name: d.name.clone(),
            enabled: d.enabled,
            plugin: matches!(
                d.kind,
                suhina_engine::model::DeviceKind::Clap { .. }
                    | suhina_engine::model::DeviceKind::Vst3 { .. }
                    | suhina_engine::model::DeviceKind::Vst2 { .. }
            ),
            group: d
                .as_group()
                .map(|g| g.branches.iter().map(|b| suhina_engine::model::count_devices(&b.devices)).sum()),
        })
        .collect()
}

impl Column {
    /// Where the track sits in the track list.
    pub fn place(&self) -> crate::interaction::nesting::Place {
        crate::interaction::nesting::Place {
            track: self.track,
            parent: self.parent,
            depth: self.depth,
            position: self.position,
            group: self.kind == ColumnKind::Group,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Counted from 1, as commands take it.
    pub number: usize,
    pub name: String,
    pub tempo_bpm: Option<f64>,
    pub time_signature: Option<TimeSignature>,
    /// Every clip in the row is playing, and at least one is.
    pub playing: bool,
}

impl Scene {
    /// What the scene's menu shows; `only` when it is the project's only scene.
    pub fn menu(&self, only: bool) -> crate::interaction::menus::SceneMenu<'_> {
        crate::interaction::menus::SceneMenu {
            number: self.number,
            name: &self.name,
            tempo_bpm: self.tempo_bpm,
            time_signature: self.time_signature,
            only,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackGrid {
    pub columns: Vec<Column>,
    /// The MIDI input ports there are, for the tracks' MIDI input menus; set by the app.
    pub midi_ports: Vec<String>,
    /// The channels of the audio input device open now (0 when none is), for the tracks'
    /// audio input menus; set by the app.
    pub input_channels: u16,
    pub scenes: Vec<Scene>,
    pub running: bool,
    /// Beats since the transport started.
    pub beat: f64,
    pub tempo_bpm: f64,
    /// The bars playing: the project's meter, or, while the engine plays, the time signature a
    /// launched scene changed it to.
    pub meter: PlayingMeter,
    /// The project's meter: bars through its time-signature changes.
    pub project_meter: Meter,
    /// The keys and notes mapped to slots, scenes and stop buttons that still exist, with the
    /// label the mapping mode shows (`docs/spec/ui-session-view.md` › Launch keys).
    pub mapped: Vec<(MappingTarget, String)>,
    /// The main output's volume in dB.
    pub main_volume_db: f64,
    /// The main channel's devices, for its strip's list.
    pub main_devices: Vec<StripDevice>,
    /// The metronome: on, its level in dB, its sound; set by the app.
    pub metronome: (bool, f64, suhina_engine::ClickSound),
}

impl TrackGrid {
    /// The grid for `project`; with a monitor, slots show what the engine plays and has queued.
    pub fn build(project: &Project, monitor: Option<&LauncherMonitor>) -> Self {
        let scene_count = project.launcher.scenes.len();

        let mut columns = Vec::new();
        let mut returns = Vec::new();
        let return_tracks: Vec<&Track> = project
            .tracks
            .iter()
            .filter(|t| {
                let channel = t.channel.and_then(|id| project.channel(id));
                channel.is_some_and(|c| c.role == ChannelRole::Effect)
            })
            .collect();
        for (index, track) in project.tracks.iter().enumerate() {
            let channel = track.channel.and_then(|id| project.channel(id));
            let role = channel.map_or(ChannelRole::Regular, |c| c.role);
            let kind = match role {
                ChannelRole::Effect => ColumnKind::Return,
                ChannelRole::Submix => ColumnKind::Group,
                ChannelRole::Regular | ChannelRole::Master => ColumnKind::Track,
            };
            if role == ChannelRole::Master {
                continue;
            }
            let launcher = project.launcher.columns.iter().position(|c| c.track == track.id);
            let status = launcher.zip(monitor).and_then(|(i, m)| m.column(i));
            let playing = status.and_then(|s| s.playing);
            let queued = status.and_then(|s| s.queued);
            let slots = match (kind, launcher.and_then(|i| project.launcher.columns.get(i))) {
                (ColumnKind::Return, _) => Vec::new(),
                (_, None) => {
                    vec![
                        Slot {
                            state: SlotState::Empty,
                            name: String::new(),
                            enabled: true,
                            length: 0.0,
                            launch: None,
                            color: None
                        };
                        scene_count
                    ]
                }
                (_, Some(column)) => (0..scene_count)
                    .map(|scene| {
                        let slot = column.slots.get(scene);
                        let clip = slot.and_then(|s| s.clip.as_ref());
                        let state = match clip {
                            None if slot.is_some_and(|s| s.has_stop) => SlotState::EmptyStop,
                            None => SlotState::Empty,
                            Some(_) if playing == Some(scene) => SlotState::Playing,
                            Some(_) if queued == Some(Queued::Launch(scene)) => SlotState::Queued,
                            Some(_) => SlotState::Stopped,
                        };
                        Slot {
                            state,
                            name: clip
                                .and_then(|c| project.content(c.content))
                                .map(|c| c.name.clone())
                                .unwrap_or_default(),
                            enabled: clip.is_none_or(|c| c.enabled),
                            length: clip.map_or(0.0, |c| c.loop_range.map_or(c.length, |r| r.end - r.start)),
                            launch: clip.map(|c| c.launch),
                            color: clip.and_then(|c| c.color),
                        }
                    })
                    .collect(),
            };
            let column = Column {
                track: track.id,
                name: track.name.clone(),
                color: track.color.unwrap_or_else(|| default_track_colour(index)),
                kind,
                notes: kind == ColumnKind::Track && track.content.notes,
                launcher,
                slots,
                stop_queued: queued == Some(Queued::Stop),
                mute: channel.is_some_and(|c| c.mute),
                solo: channel.is_some_and(|c| c.solo),
                cue: channel.is_some_and(|c| c.cue),
                armable: kind == ColumnKind::Track && (track.content.notes || track.content.audio),
                armed: false,
                recording: None,
                midi_input: track.midi_input.clone(),
                plays_notes: kind == ColumnKind::Track && track.content.notes,
                plays_audio: kind == ColumnKind::Track && track.content.audio,
                audio_input: track.audio_input,
                volume_db: channel.map_or(0.0, |c| gain_to_db(c.volume)),
                position: index,
                depth: crate::interaction::nesting::depth(project, track.parent),
                parent: track.parent,
                output: channel
                    .and_then(|c| c.output)
                    .and_then(|id| project.tracks.iter().find(|t| t.channel == Some(id)))
                    .map(|t| t.id),
                pan: channel.map_or(0.0, |c| c.pan),
                sends: match (kind, channel) {
                    (ColumnKind::Return, _) | (_, None) => Vec::new(),
                    (_, Some(channel)) => return_tracks
                        .iter()
                        .map(|r| {
                            let send = channel.sends.iter().find(|s| Some(s.target) == r.channel);
                            SendKnob {
                                target: r.id,
                                name: r.name.clone(),
                                level_db: send.map_or(f64::NEG_INFINITY, |s| gain_to_db(s.level)),
                                pre_fader: send.is_some_and(|s| s.pre_fader),
                            }
                        })
                        .collect(),
                },
                devices: channel.map_or_else(Vec::new, |c| strip_devices(&c.devices)),
            };
            if kind == ColumnKind::Return { returns.push(column) } else { columns.push(column) }
        }
        columns.extend(returns);

        let scenes = project
            .launcher
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| {
                let with_clips: Vec<&Column> = columns
                    .iter()
                    .filter(|c| {
                        c.slots.get(index).is_some_and(|s| !matches!(s.state, SlotState::Empty | SlotState::EmptyStop))
                    })
                    .collect();
                let playing = !with_clips.is_empty()
                    && with_clips.iter().all(|c| c.slots.get(index).is_some_and(|s| s.state == SlotState::Playing));
                Scene {
                    number: index + 1,
                    name: scene.name.clone(),
                    tempo_bpm: scene.tempo_bpm,
                    time_signature: scene.time_signature,
                    playing,
                }
            })
            .collect();

        let (running, beat, tempo_bpm) = match monitor {
            Some(m) => (m.running(), m.beat(), m.tempo_bpm()),
            None => (false, 0.0, project.transport.tempo_bpm),
        };
        let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
        let meter = monitor.map_or(
            PlayingMeter { time_signature: project.transport.time_signature, ..PlayingMeter::default() },
            LauncherMonitor::meter,
        );
        let mut mapped: Vec<(MappingTarget, String)> = Vec::new();
        for m in project.launcher.mappings.iter().filter(|m| suhina_session::target_exists(project, &m.target)) {
            match mapped.iter_mut().find(|(t, _)| *t == m.target) {
                Some((_, label)) => *label = format!("{label} {}", m.source.label()),
                None => mapped.push((m.target, m.source.label())),
            }
        }
        let mut grid = Self {
            columns,
            midi_ports: Vec::new(),
            input_channels: 0,
            scenes,
            running,
            beat,
            tempo_bpm,
            meter,
            project_meter: project.transport.meter(),
            mapped,
            main_volume_db: project
                .channels
                .iter()
                .find(|c| c.role == ChannelRole::Master)
                .map_or(0.0, |c| gain_to_db(c.volume)),
            main_devices: project
                .channels
                .iter()
                .find(|c| c.role == ChannelRole::Master)
                .map_or_else(Vec::new, |c| strip_devices(&c.devices)),
            metronome: (false, -6.0, suhina_engine::ClickSound::Beep),
        };
        grid.follow_automation(project);
        grid
    }

    /// While the transport runs, the strips show what automation plays: volume and pan, on
    /// returns and Main too (#706).
    fn follow_automation(&mut self, project: &Project) {
        if !self.running {
            return;
        }
        let channel_of = |track: TrackId| {
            let id = project.track(track)?.channel?;
            project.channel(id)
        };
        for column in &mut self.columns {
            let Some(channel) = channel_of(column.track) else { continue };
            let (volume, pan) = played_mixer(channel, self.beat);
            if let Some(volume) = volume {
                column.volume_db = volume;
            }
            if let Some(pan) = pan {
                column.pan = pan;
            }
        }
        if let Some(main) = project.channels.iter().find(|c| c.role == ChannelRole::Master)
            && let (Some(volume), _) = played_mixer(main, self.beat)
        {
            self.main_volume_db = volume;
        }
    }

    /// The time signature playing: the project meter's at the playhead, or the one a launched
    /// scene set.
    pub fn time_signature(&self) -> TimeSignature {
        if self.meter.follows_map {
            self.project_meter.position(self.beat).time_signature
        } else {
            self.meter.time_signature
        }
    }

    /// The keys and notes mapped to `target`, as the mapping mode shows them.
    pub fn mapped_to(&self, target: MappingTarget) -> Option<&str> {
        self.mapped.iter().find(|(t, _)| *t == target).map(|(_, label)| label.as_str())
    }

    /// The transport position as bar.beat.sixteenth, counted from 1.
    pub fn position(&self) -> String {
        let (bar, in_bar) = self.meter.locate(self.beat, &self.project_meter);
        let beat = in_bar.floor() as u64 + 1;
        let sixteenth = ((in_bar.fract()) * 4.0).floor() as u64 + 1;
        format!("{bar}.{beat}.{sixteenth}")
    }
}

/// A gain factor in dB; silence is negative infinity.
pub fn gain_to_db(gain: f64) -> f64 {
    if gain > 0.0 { 20.0 * gain.log10() } else { f64::NEG_INFINITY }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_project;

    #[test]
    fn tracks_come_first_then_returns() {
        let grid = TrackGrid::build(&demo_project(), None);
        let kinds: Vec<ColumnKind> = grid.columns.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                ColumnKind::Track,
                ColumnKind::Track,
                ColumnKind::Track,
                ColumnKind::Track,
                ColumnKind::Return,
                ColumnKind::Return
            ]
        );
        assert!(grid.columns.iter().all(|c| c.kind == ColumnKind::Return || c.slots.len() == grid.scenes.len()));
        assert!(grid.columns.iter().filter(|c| c.kind == ColumnKind::Return).all(|c| c.slots.is_empty()));
    }

    #[test]
    fn slots_without_an_engine_are_stopped_or_empty() {
        let grid = TrackGrid::build(&demo_project(), None);
        let drums = &grid.columns[0];
        assert_eq!(drums.slots[0].state, SlotState::Stopped);
        assert_eq!(drums.slots[0].name, "Beat A");
        assert!(
            grid.columns
                .iter()
                .flat_map(|c| &c.slots)
                .all(|s| matches!(s.state, SlotState::Stopped | SlotState::Empty | SlotState::EmptyStop))
        );
        assert!(!grid.running);
        assert!(grid.scenes.iter().all(|s| !s.playing));
    }

    #[test]
    fn mixer_values_are_in_db() {
        let grid = TrackGrid::build(&demo_project(), None);
        assert!((grid.columns[0].volume_db - 0.0).abs() < 1e-9);
        assert!((gain_to_db(0.5) - -6.0206).abs() < 1e-3);
        assert_eq!(gain_to_db(0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn position_counts_bars_beats_and_sixteenths_from_one() {
        let grid = TrackGrid { beat: 5.5, ..TrackGrid::default() };
        assert_eq!(grid.position(), "2.2.3");
        let start = TrackGrid::default();
        assert_eq!(start.position(), "1.1.1");
        // 4/4 for bar 1, then 3/4: beat 8.5 is in bar 3, on its second beat.
        use suhina_engine::model::{TimeSignature, TimeSignatureChange};
        let changes = [TimeSignatureChange { bar: 1, time_signature: TimeSignature { numerator: 3, denominator: 4 } }];
        let meter = Meter::new(TimeSignature { numerator: 4, denominator: 4 }, &changes);
        let changing = TrackGrid { beat: 8.5, project_meter: meter, ..TrackGrid::default() };
        assert_eq!(changing.position(), "3.2.3");
        assert_eq!(changing.time_signature(), TimeSignature { numerator: 3, denominator: 4 });
    }
}
