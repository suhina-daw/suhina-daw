// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the Arrangement view shows, derived from the project and the engine's monitor: one lane
//! per track with its clips placed in beats, and marks for the notes inside each clip. Pure data,
//! like [`crate::interaction::grid`].

use std::sync::Arc;
use suhina_engine::LauncherMonitor;

use suhina_engine::model::{ChannelRole, Clip, ClipContent, ClipId, FadeCurve, LaneData, Meter, Project, Rgb, TrackId};

use crate::interaction::grid::{ColumnKind, default_track_colour, gain_to_db};
use crate::interaction::nesting::depth;
use crate::kit::waveform::Waveforms;

/// Note marks drawn per clip at most; the rest are left out.
const MAX_MARKS: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct NoteMark {
    /// Beats from the clip's start.
    pub start: f64,
    pub length: f64,
    pub key: u8,
}

/// How an audio clip reads its file: which file, and the file position at each clip beat.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioView {
    /// The region's pool key (`AudioRegion::pool_key`), the key of the decoded audio and its waveform.
    pub file: String,
    /// Where the clip starts in its content, in beats, and its loop.
    pub offset: f64,
    pub loop_range: Option<(f64, f64)>,
    /// Warp markers as (beat, seconds), sorted; with fewer than two, a fixed rate.
    pub markers: Vec<(f64, f64)>,
    pub seconds_per_beat: f64,
    /// Clip beats per project beat (transpose on an unwarped clip).
    pub rate: f64,
    pub gain_db: f64,
}

impl AudioView {
    /// The file position, in seconds, `elapsed` project beats after the clip starts, as the
    /// engine plays it.
    pub fn seconds(&self, elapsed: f64) -> f64 {
        let mut beat = self.offset + elapsed * self.rate;
        if let Some((start, end)) = self.loop_range
            && beat >= end
        {
            beat = start + (beat - end).rem_euclid(end - start);
        }
        if self.markers.len() < 2 {
            return beat * self.seconds_per_beat;
        }
        let last = self.markers.len() - 1;
        let i = self.markers.iter().position(|(b, _)| *b > beat).unwrap_or(self.markers.len()).clamp(1, last);
        match (self.markers.get(i - 1), self.markers.get(i)) {
            (Some(&(b0, s0)), Some(&(b1, s1))) if b1 > b0 => s0 + (beat - b0) * (s1 - s0) / (b1 - b0),
            (Some(&(_, s0)), _) => s0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipBox {
    /// Which clip it is: commands name it by this id, so a selection or a drag keeps naming the
    /// same clip when others move (ADR 0025).
    pub id: ClipId,
    /// Where it is on the track now, in start order, as `project.clips` counts it.
    pub index: usize,
    pub start: f64,
    pub length: f64,
    pub name: String,
    pub enabled: bool,
    pub audio: bool,
    /// Fade-in and fade-out, in beats (`clip.setFades`).
    pub fade_in: f64,
    pub fade_out: f64,
    /// The fades' shapes and curves.
    pub fade_in_curve: FadeCurve,
    pub fade_out_curve: FadeCurve,
    /// For an audio clip, how to draw its waveform.
    pub audio_view: Option<AudioView>,
    pub notes: Vec<NoteMark>,
    /// Where a looping clip starts another pass through its loop, in beats from its start: the
    /// clip marks each with notches and a dashed line (#700).
    pub seams: Vec<f64>,
    /// The sub-row the clip is drawn in, and how many the lane has: more than one where clips
    /// overlap (stacked), so each stays visible.
    pub row: usize,
    pub rows: usize,
    /// How early the clip's left edge can go: where its content starts (the song's start for a
    /// looping clip), as `clip.trim` allows.
    pub earliest_start: f64,
    /// How late its right edge can go: where its audio ends, for an audio clip that does not
    /// loop; `None` when it can go on (a MIDI or looping clip).
    pub latest_end: Option<f64>,
    /// The colour it shows: its own, or its track's.
    pub color: Rgb,
    /// What the clip is known by from one build to the next (its content and its start), so a
    /// stacked clip keeps its sub-row when another goes.
    pub stack_key: (u32, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub track: TrackId,
    pub name: String,
    pub color: Rgb,
    pub kind: ColumnKind,
    /// The track plays notes: a double-click on an empty spot of its lane makes a MIDI clip.
    pub notes: bool,
    /// How many groups the track is inside, and the one it is directly in.
    pub depth: usize,
    pub parent: Option<TrackId>,
    /// The track's place in the project's track order, as `track.move` counts it.
    pub position: usize,
    pub mute: bool,
    pub solo: bool,
    /// Also heard on the cue output (`track.setCue`).
    pub cue: bool,
    /// The track plays notes, so it can be armed (ADR 0017), and whether it is: the session
    /// knows, not the project, so [`Timeline::build`] leaves it false.
    pub armable: bool,
    pub armed: bool,
    /// What the track hears while armed.
    pub midi_input: suhina_engine::model::MidiInput,
    /// The track plays notes, or audio: which of its inputs it has.
    pub plays_notes: bool,
    pub plays_audio: bool,
    pub audio_input: suhina_engine::model::AudioInput,
    pub volume_db: f64,
    pub pan: f64,
    /// The track's sends, one per return track in track order (none on a return), as the
    /// Session view's strip has them.
    pub sends: Vec<crate::interaction::grid::SendKnob>,
    /// The group track this track plays into; `None` is the main output.
    pub output: Option<TrackId>,
    /// The track's column in the launcher, for the engine's per-track state.
    pub column: Option<usize>,
    /// The track plays launched clips instead of its arrangement clips.
    pub overrides: bool,
    pub clips: Vec<ClipBox>,
}

/// A marker on the timeline, as the marker row under the ruler draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerFlag {
    /// Index in beat order, as `marker.move` and `project.markers` count it.
    pub index: usize,
    pub beat: f64,
    pub name: String,
}

/// The arrangement's loop region, as the loop brace row under the ruler draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopBrace {
    pub start: f64,
    pub end: f64,
    /// Playback repeats it.
    pub enabled: bool,
}

/// The sub-row each stacked clip was put in, by track and [`ClipBox::stack_key`], with the span
/// it had: the next build keeps it there, and a new clip inside that span (a split's parts) takes
/// that row.
pub type StackRows = std::collections::HashMap<(TrackId, (u32, u64)), (usize, f64, f64)>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// Tracks in project order, then return tracks.
    pub lanes: Vec<Lane>,
    /// The sub-row each stacked clip was put in, by track and [`ClipBox::stack_key`]: the next
    /// build keeps them there, so deleting one clip of a stack leaves the others where they are.
    pub stack_rows: StackRows,
    /// The MIDI input ports there are, for the tracks' MIDI inputs; set by the app.
    pub midi_ports: Vec<String>,
    /// The channels of the audio input device open now (0 when none is); set by the app.
    pub input_channels: u16,
    /// The main channel's volume, if the project has one.
    pub main_volume_db: Option<f64>,
    /// Where the last clip ends, in beats.
    pub end: f64,
    pub bar_beats: f64,
    /// Bars through the time-signature changes; the ruler numbers bars with it.
    pub meter: Meter,
    /// Tempo and time-signature changes, as the ruler marks them (`docs/spec/tempo-map.md` ›
    /// The Arrangement).
    pub marks: Vec<RulerMark>,
    pub running: bool,
    /// The playhead, in beats from the start.
    pub beat: f64,
    /// The play start marker: where the transport starts, in beats.
    pub start_beat: f64,
    /// The arrangement's markers, in beat order.
    pub markers: Vec<MarkerFlag>,
    /// The loop region, when the project has one (on or off).
    pub loop_brace: Option<LoopBrace>,
    /// Any lane's track plays launched clips instead of its arrangement clips: the
    /// arrangement view dims its lanes and shows Back to arrangement.
    pub overrides: bool,
    /// Waveforms of the project's audio files, set by the app once they are decoded.
    pub waveforms: Arc<Waveforms>,
    /// The notes ARA plug-ins found in audio clips, by clip, set by the app every frame (#720).
    pub ara_notes: std::collections::HashMap<ClipId, suhina_session::ClipNotes>,
    /// What an ARA plug-in is doing with a track's audio clips, by track: its name, and how far
    /// its analysis has come (`None` while it loads).
    pub ara_activity: std::collections::HashMap<TrackId, (String, Option<f32>)>,
    /// The project's tempo map in seconds, to place those notes, which come in seconds.
    pub tempo: Option<suhina_engine::model::TempoMap>,
    /// The parameters each track's automation lanes can show, with their points.
    pub automation: crate::arrangement::automation_lanes::Params,
}

impl Lane {
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

/// A change marked on the ruler: where it is and what it says ("140 bpm", "→ 140 bpm" for a ramp,
/// "3/4"); a tempo and a signature change on one beat share one mark.
#[derive(Debug, Clone, PartialEq)]
pub struct RulerMark {
    pub beat: f64,
    pub text: String,
}

/// The ruler's marks for `project`'s tempo and time-signature changes, in beat order.
fn ruler_marks(project: &Project, meter: &Meter) -> Vec<RulerMark> {
    let bpm = |bpm: f64| {
        let rounded = (bpm * 100.0).round() / 100.0;
        if rounded.fract() == 0.0 { format!("{rounded:.0} bpm") } else { format!("{rounded} bpm") }
    };
    let tempo = project
        .transport
        .tempo_changes
        .iter()
        .filter(|p| p.beat.is_finite() && p.beat > 0.0)
        .map(|p| (p.beat, if p.ramp { format!("→ {}", bpm(p.bpm)) } else { bpm(p.bpm) }));
    let signatures = meter.changes().map(|(_, beat, ts)| (beat, format!("{}/{}", ts.numerator, ts.denominator)));
    let mut all: Vec<(f64, String)> = tempo.chain(signatures).collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut marks: Vec<RulerMark> = Vec::new();
    for (beat, text) in all {
        match marks.last_mut() {
            Some(last) if last.beat == beat => {
                last.text.push_str("  ");
                last.text.push_str(&text);
            }
            _ => marks.push(RulerMark { beat, text }),
        }
    }
    marks
}

impl Timeline {
    pub fn build(project: &Project, monitor: Option<&LauncherMonitor>) -> Self {
        Self::build_after(project, monitor, &std::collections::HashMap::new())
    }

    /// [`Timeline::build`], with the sub-rows the previous build gave stacked clips kept.
    pub fn build_after(project: &Project, monitor: Option<&LauncherMonitor>, rows_before: &StackRows) -> Self {
        let mut stack_rows = std::collections::HashMap::new();
        let ts = project.transport.time_signature;
        let bar_beats =
            if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
        let meter = project.transport.meter();
        let marks = ruler_marks(project, &meter);
        let mut lanes = Vec::new();
        let mut returns = Vec::new();
        let return_tracks: Vec<&suhina_engine::model::Track> = project
            .tracks
            .iter()
            .filter(|t| {
                let channel = t.channel.and_then(|id| project.channel(id));
                channel.is_some_and(|c| c.role == ChannelRole::Effect)
            })
            .collect();
        for (position, track) in project.tracks.iter().enumerate() {
            let channel = track.channel.and_then(|id| project.channel(id));
            let kind = match channel.map(|c| c.role) {
                Some(ChannelRole::Master) => continue,
                Some(ChannelRole::Effect) => ColumnKind::Return,
                Some(ChannelRole::Submix) => ColumnKind::Group,
                _ => ColumnKind::Track,
            };
            let track_color = track.color.unwrap_or_else(|| default_track_colour(position));
            let clips = project
                .arrangement
                .lanes
                .iter()
                .find(|l| l.track == Some(track.id))
                .map(|lane| {
                    lane.clips
                        .iter()
                        .enumerate()
                        .map(|(index, clip)| {
                            let content = project.content(clip.content);
                            let looped = clip.loop_range.is_some();
                            let audio =
                                content.is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
                            ClipBox {
                                id: clip.id,
                                color: clip.color.unwrap_or(track_color),
                                stack_key: (clip.content.0, clip.start.to_bits()),
                                earliest_start: if looped { 0.0 } else { (clip.start - clip.offset).max(0.0) },
                                latest_end: content
                                    .filter(|_| audio && !looped)
                                    .map(|c| clip.start - clip.offset + c.length),
                                index,
                                start: clip.start,
                                length: clip.length,
                                name: content.map(|c| c.name.clone()).unwrap_or_default(),
                                enabled: clip.enabled,
                                fade_in: clip.fade_in,
                                fade_out: clip.fade_out,
                                fade_in_curve: clip.fade_in_curve,
                                fade_out_curve: clip.fade_out_curve,
                                audio: content
                                    .is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)))),
                                audio_view: content.and_then(|c| audio_view(c, clip, project.transport.tempo_bpm)),
                                notes: content.map(|c| note_marks(c, clip)).unwrap_or_default(),
                                seams: loop_seams(clip),
                                row: 0,
                                rows: 1,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let clips = sub_rows(clips, track.id, rows_before, &mut stack_rows);
            let lane = Lane {
                track: track.id,
                name: track.name.clone(),
                color: track.color.unwrap_or_else(|| default_track_colour(position)),
                kind,
                notes: kind == ColumnKind::Track && track.content.notes,
                depth: depth(project, track.parent),
                parent: track.parent,
                position,
                mute: channel.is_some_and(|c| c.mute),
                solo: channel.is_some_and(|c| c.solo),
                cue: channel.is_some_and(|c| c.cue),
                armable: kind == ColumnKind::Track && (track.content.notes || track.content.audio),
                armed: false,
                midi_input: track.midi_input.clone(),
                plays_notes: kind == ColumnKind::Track && track.content.notes,
                plays_audio: kind == ColumnKind::Track && track.content.audio,
                audio_input: track.audio_input,
                volume_db: channel.map_or(0.0, |c| gain_to_db(c.volume)),
                pan: channel.map_or(0.0, |c| c.pan),
                sends: match (kind, channel) {
                    (ColumnKind::Return, _) | (_, None) => Vec::new(),
                    (_, Some(channel)) => return_tracks
                        .iter()
                        .map(|r| {
                            let send = channel.sends.iter().find(|s| Some(s.target) == r.channel);
                            crate::interaction::grid::SendKnob {
                                target: r.id,
                                name: r.name.clone(),
                                level_db: send.map_or(f64::NEG_INFINITY, |s| gain_to_db(s.level)),
                                pre_fader: send.is_some_and(|s| s.pre_fader),
                            }
                        })
                        .collect(),
                },
                output: channel
                    .and_then(|c| c.output)
                    .and_then(|id| project.tracks.iter().find(|t| t.channel == Some(id)))
                    .map(|t| t.id),
                column: project.launcher.columns.iter().position(|c| c.track == track.id),
                overrides: false,
                clips,
            };
            if kind == ColumnKind::Return { returns.push(lane) } else { lanes.push(lane) }
        }
        lanes.extend(returns);
        let markers: Vec<MarkerFlag> = project
            .arrangement
            .markers
            .iter()
            .enumerate()
            .filter(|(_, m)| m.is_valid())
            .map(|(index, m)| MarkerFlag { index, beat: m.beat, name: m.name.clone() })
            .collect();
        let transport = &project.transport;
        let loop_brace = transport.loop_range.filter(|r| r.is_valid_region()).map(|r| LoopBrace {
            start: r.start,
            end: r.end,
            enabled: transport.loop_enabled,
        });
        // The timeline reaches past the last marker and the loop end too, so every marker can be scrolled to.
        let end = lanes
            .iter()
            .flat_map(|l| &l.clips)
            .map(|c| c.start + c.length)
            .chain(markers.iter().map(|m| m.beat))
            .chain(loop_brace.map(|l| l.end))
            .fold(0.0, f64::max);
        let main_volume_db =
            project.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| gain_to_db(c.volume));
        let mut timeline = Self {
            stack_rows,
            ara_notes: std::collections::HashMap::new(),
            ara_activity: std::collections::HashMap::new(),
            tempo: Some(project.transport.tempo_map(1.0)),
            midi_ports: Vec::new(),
            input_channels: 0,
            lanes,
            main_volume_db,
            end,
            bar_beats,
            meter,
            marks,
            running: false,
            beat: 0.0,
            start_beat: 0.0,
            markers,
            loop_brace,
            overrides: false,
            waveforms: Arc::default(),
            automation: crate::arrangement::automation_lanes::Params::build(project),
        };
        timeline.follow(monitor);
        timeline
    }

    /// Takes the transport, playhead and play start marker from the engine, leaving the rest as
    /// it was built.
    /// The headers show what automation plays while the transport runs, the project's values
    /// otherwise: volume and pan, on returns and Main too (#706). Called every frame, after
    /// [`Timeline::follow`].
    pub fn follow_automation(&mut self, project: &Project) {
        let channel_of = |track: TrackId| {
            let id = project.track(track)?.channel?;
            project.channel(id)
        };
        for lane in &mut self.lanes {
            let Some(channel) = channel_of(lane.track) else { continue };
            let (volume, pan) =
                if self.running { crate::interaction::grid::played_mixer(channel, self.beat) } else { (None, None) };
            lane.volume_db = volume.unwrap_or_else(|| gain_to_db(channel.volume));
            lane.pan = pan.unwrap_or(channel.pan);
        }
        if let Some(main) = project.channels.iter().find(|c| c.role == ChannelRole::Master) {
            let played = if self.running { crate::interaction::grid::played_mixer(main, self.beat).0 } else { None };
            self.main_volume_db = Some(played.unwrap_or_else(|| gain_to_db(main.volume)));
        }
    }

    pub fn follow(&mut self, monitor: Option<&LauncherMonitor>) {
        let (running, beat, start) = monitor.map_or((false, 0.0, 0.0), |m| (m.running(), m.beat(), m.start_beat()));
        let beats = |b: f64| if b.is_finite() { b.max(0.0) } else { 0.0 };
        self.running = running;
        self.beat = beats(beat);
        self.start_beat = beats(start);
        // Which tracks play launched clips instead of their arrangement clips: their lanes
        // dim, and the arrangement shows Back to arrangement while any does.
        for lane in &mut self.lanes {
            lane.overrides = lane
                .column
                .and_then(|column| monitor.and_then(|m| m.column(column)))
                .is_some_and(|s| s.overrides_arrangement);
        }
        self.overrides = self.lanes.iter().any(|l| l.overrides);
    }
}

/// Which groups the Arrangement view has folded, and the track each folded group's row stands in
/// for (`docs/spec/arrangement-editing.md` › Folded groups). View state: a project does not keep
/// it, so opening a project shows every group open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    /// The groups whose tracks are folded away.
    folded: Vec<TrackId>,
    /// What each of them shows, in the same order: the track whose clips and colour its row
    /// takes. Worked out by [`Folds::reconcile`], never set by a click.
    shown: Vec<(TrackId, TrackId)>,
}

impl Folds {
    /// Whether `group`'s tracks are folded away.
    pub fn is_folded(&self, group: TrackId) -> bool {
        self.folded.contains(&group)
    }

    /// The track the folded `group`'s row shows; `None` while the group is open.
    pub fn shown(&self, group: TrackId) -> Option<TrackId> {
        self.shown.iter().find(|(g, _)| *g == group).map(|(_, track)| *track)
    }

    /// Folds `group`'s tracks away, or opens the group again.
    pub fn toggle(&mut self, group: TrackId) {
        match self.folded.iter().position(|g| *g == group) {
            Some(at) => {
                self.folded.remove(at);
            }
            None => self.folded.push(group),
        }
    }

    /// The folded groups as a set, for the views that share them (the Session view's columns,
    /// the track list's drop targets).
    pub fn groups(&self) -> std::collections::BTreeSet<TrackId> {
        self.folded.iter().copied().collect()
    }

    /// Folds exactly `groups`: what another view folded or opened. Groups folded already keep
    /// their place; what each shows is worked out again by [`Folds::reconcile`].
    pub fn set_groups(&mut self, groups: &std::collections::BTreeSet<TrackId>) {
        self.folded.retain(|g| groups.contains(g));
        for group in groups {
            self.fold(*group);
        }
    }

    /// Folds `group`'s tracks away, as the group a Stems file arrives in does.
    pub fn fold(&mut self, group: TrackId) {
        if !self.is_folded(group) {
            self.folded.push(group);
        }
    }

    /// Brings the folds in line with the project: groups it no longer has are forgotten (a group
    /// whose tracks are all gone stops being folded), and every folded group shows `targeted`
    /// while that is one of its own tracks, else the first track inside it.
    pub fn reconcile(&mut self, project: &Project, targeted: Option<TrackId>) {
        self.folded.retain(|group| first_inside(project, *group).is_some());
        self.shown = self
            .folded
            .iter()
            .filter_map(|group| {
                let first = first_inside(project, *group)?;
                let show = targeted.filter(|track| holds(project, *group, *track)).unwrap_or(first);
                Some((*group, show))
            })
            .collect();
    }
}

/// The first track inside `group` in project order, when the project has one.
fn first_inside(project: &Project, group: TrackId) -> Option<TrackId> {
    project.tracks.iter().find(|t| t.parent == Some(group)).map(|t| t.id)
}

/// Whether `track` is inside `group`: one of its own tracks, or of a group inside it. A broken
/// parent chain stops the walk.
fn holds(project: &Project, group: TrackId, track: TrackId) -> bool {
    let mut parent = project.track(track).and_then(|t| t.parent);
    let mut hops = 0;
    while let Some(id) = parent {
        if id == group {
            return true;
        }
        hops += 1;
        if hops > project.tracks.len() {
            break;
        }
        parent = project.track(id).and_then(|t| t.parent);
    }
    false
}

/// How many lanes follow the lane at `index` inside it. A project keeps a group's tracks next to
/// the group, so that run is its family.
pub(crate) fn inside_lanes<T: std::borrow::Borrow<Lane>>(lanes: &[T], index: usize) -> usize {
    let depth = lanes.get(index).map_or(0, |l| l.borrow().depth);
    lanes.iter().skip(index.saturating_add(1)).take_while(|l| l.borrow().depth > depth).count()
}

/// What an audio clip needs to draw its waveform: the engine's mapping from beats to the file.
fn audio_view(content: &ClipContent, clip: &Clip, tempo_bpm: f64) -> Option<AudioView> {
    let region = content.lanes.iter().find_map(|l| match &l.data {
        LaneData::Audio(region) => Some(region),
        LaneData::Notes(_) => None,
    })?;
    let mut markers: Vec<(f64, f64)> = region
        .warp_markers
        .iter()
        .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
        .map(|m| (m.time, m.audio_time))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    markers.dedup_by(|a, b| a.0 == b.0);
    let rate = if region.warped { 1.0 } else { 2f64.powf(region.transpose / 12.0) };
    Some(AudioView {
        file: region.pool_key().into_owned(),
        offset: clip.offset,
        loop_range: clip.loop_range.filter(|r| r.end > r.start).map(|r| (r.start, r.end)),
        markers: if region.warped { markers } else { Vec::new() },
        seconds_per_beat: if tempo_bpm > 0.0 { 60.0 / tempo_bpm } else { 0.5 },
        rate: if rate.is_finite() && rate > 0.0 { rate } else { 1.0 },
        gain_db: gain_to_db(region.gain),
    })
}

/// Puts overlapping clips in sub-rows: each clip in the row it had in the build before (`before`)
/// while that row is free where it plays, the others in the first row free where they start;
/// every clip learns the lane's count, and `after` what each got. A row left empty between others
/// stays, so the clips under it do not move up.
fn sub_rows(mut clips: Vec<ClipBox>, track: TrackId, before: &StackRows, after: &mut StackRows) -> Vec<ClipBox> {
    let mut rows: Vec<Vec<(f64, f64)>> = Vec::new();
    let free = |rows: &Vec<Vec<(f64, f64)>>, row: usize, from: f64, to: f64| {
        rows.get(row).is_none_or(|spans| spans.iter().all(|(s, e)| to <= *s + 1e-9 || from >= *e - 1e-9))
    };
    let mut order: Vec<usize> = (0..clips.len()).collect();
    let start = |i: &usize| clips.get(*i).map_or(0.0, |c| c.start);
    order.sort_by(|a, b| start(a).total_cmp(&start(b)));
    let mut placed = vec![false; clips.len()];
    // Where they were, first.
    for &i in &order {
        let Some(clip) = clips.get_mut(i) else { continue };
        let (from, to) = (clip.start, clip.start + clip.length);
        if let Some(&(row, _, _)) = before.get(&(track, clip.stack_key))
            && free(&rows, row, from, to)
        {
            if rows.len() <= row {
                rows.resize(row + 1, Vec::new());
            }
            rows[row].push((from, to));
            clip.row = row;
            placed[i] = true;
        }
    }
    // The rest in the first row free.
    for &i in &order {
        if placed[i] {
            continue;
        }
        let Some(clip) = clips.get_mut(i) else { continue };
        let (from, to) = (clip.start, clip.start + clip.length);
        // A new clip inside where one was on this track (a split's parts, #703) takes that
        // clip's row, when it is free; else the first row free.
        let inherited = before
            .iter()
            .filter(|((t, _), (_, start, end))| *t == track && *start <= from + 1e-9 && *end >= to - 1e-9)
            .map(|(_, (row, start, end))| (*row, end - start))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(row, _)| row)
            .filter(|&row| free(&rows, row, from, to));
        let row =
            inherited.unwrap_or_else(|| (0..=rows.len()).find(|&r| free(&rows, r, from, to)).unwrap_or(rows.len()));
        if rows.len() <= row {
            rows.resize(row + 1, Vec::new());
        }
        rows[row].push((from, to));
        clip.row = row;
    }
    // Rows only where clips overlap (#697): the clips that overlap one another, directly or
    // through others, share the lane in as many rows as they use (a row between them left empty
    // stays); a clip that overlaps none fills the lane.
    let mut order: Vec<usize> = (0..clips.len()).collect();
    let starts: Vec<f64> = clips.iter().map(|c| c.start).collect();
    let at = |i: &usize| starts.get(*i).copied().unwrap_or(0.0);
    order.sort_by(|a, b| at(a).total_cmp(&at(b)));
    let mut group: Vec<usize> = Vec::new();
    let mut group_end = f64::NEG_INFINITY;
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in &order {
        let Some(clip) = clips.get(i) else { continue };
        if !group.is_empty() && clip.start >= group_end - 1e-9 {
            groups.push(std::mem::take(&mut group));
            group_end = f64::NEG_INFINITY;
        }
        group_end = group_end.max(clip.start + clip.length);
        group.push(i);
    }
    if !group.is_empty() {
        groups.push(group);
    }
    for members in &groups {
        if let [only] = members.as_slice() {
            if let Some(clip) = clips.get_mut(*only) {
                clip.row = 0;
                clip.rows = 1;
            }
            continue;
        }
        let count = members.iter().filter_map(|i| clips.get(*i)).map(|c| c.row + 1).max().unwrap_or(1);
        for i in members {
            if let Some(clip) = clips.get_mut(*i) {
                clip.rows = count;
            }
        }
    }
    for clip in &clips {
        after.insert((track, clip.stack_key), (clip.row, clip.start, clip.start + clip.length));
    }
    clips
}

/// The notes a clip plays, from its start: the content from `offset`, then its loop again and
/// again until the clip ends.
/// Where `clip` starts another pass through its loop, in beats from its start, as the engine
/// plays it: the first pass runs from its offset (or the loop's start, for an offset past the
/// loop) to the loop's end, each after it through the whole loop. None when it does not loop.
pub fn loop_seams(clip: &Clip) -> Vec<f64> {
    let Some(range) = clip.loop_range.filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start) else {
        return Vec::new();
    };
    let pass = range.end - range.start;
    let mut at = if clip.offset >= range.end { pass } else { range.end - clip.offset };
    let mut seams = Vec::new();
    while at < clip.length - 1e-9 && seams.len() < MAX_MARKS {
        seams.push(at);
        at += pass;
    }
    seams
}

/// How far past a looping clip's end its note marks go, in beats: the passes a drag of its right
/// edge shows before the clip is built again at its new length.
const LOOP_DRAG_ROOM: f64 = 64.0;

pub fn note_marks(content: &ClipContent, clip: &Clip) -> Vec<NoteMark> {
    let notes: Vec<_> = content
        .lanes
        .iter()
        .filter_map(|l| match &l.data {
            LaneData::Notes(notes) => Some(notes),
            LaneData::Audio(_) => None,
        })
        .flatten()
        .collect();
    let mut marks = Vec::new();
    if notes.is_empty() || clip.length.is_nan() || clip.length <= 0.0 {
        return marks;
    }
    // Segments of content time, in the order the clip plays them.
    let (first_end, repeat) = match clip.loop_range {
        Some(r) if r.end > r.start => (r.end.max(clip.offset), Some((r.start, r.end))),
        _ => (clip.offset + clip.length, None),
    };
    let mut segment = (clip.offset, first_end);
    let mut at = 0.0;
    // A looping clip's marks go on past its end, so the passes its right edge is dragged out to
    // show their notes while it is dragged; the view draws the clip's own span only.
    let span = if repeat.is_some() { clip.length + LOOP_DRAG_ROOM } else { clip.length };
    while at < span && marks.len() < MAX_MARKS {
        let (from, to) = segment;
        for note in notes.iter().filter(|n| n.time >= from && n.time < to) {
            let start = at + note.time - from;
            if start < span && marks.len() < MAX_MARKS {
                let length = note.duration.min(to - note.time).min(span - start).max(0.0);
                marks.push(NoteMark { start, length, key: note.key });
            }
        }
        at += to - from;
        match repeat {
            Some(r) => segment = r,
            None => break,
        }
    }
    marks
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
