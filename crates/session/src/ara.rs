// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ARA document a plug-in that edits clips is loaded with (ADR 0033): the audio clips on
//! its track's arrangement lanes as playback regions, each file they play as an audio source in
//! shared memory.
//!
//! For now a clip is described as it plays at its file's own speed: a warped clip's start and
//! end follow the song, but the plug-in reads it unstretched. A looped clip is one region for
//! each pass through its loop, all of the same modification, so an edit plays in every repeat.

use suhina_engine::AudioPool;
use suhina_model::{AudioRegion, Clip, LaneData, Project, TrackId};
use suhina_plugins::AudioSource;
use suhina_plugins::protocol::{AraDocument, AraNote, AraRegion, AraSource, MAX_ARA_OBJECTS};

/// The notes an ARA plug-in found in a clip (#720), for the arrangement to draw over its
/// waveform: times in seconds from the clip's start, and the lowest and highest pitch.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipNotes {
    pub notes: std::sync::Arc<[AraNote]>,
    pub low: u8,
    pub high: u8,
}

impl ClipNotes {
    /// `None` for no notes.
    pub(crate) fn new(notes: Vec<AraNote>) -> Option<Self> {
        let low = notes.iter().map(|n| n.pitch).min()?;
        let high = notes.iter().map(|n| n.pitch).max()?;
        Some(Self { notes: notes.into(), low, high })
    }
}

/// What a track's document holds, without the samples: its name, the files its clips play (by
/// audio pool key) and the regions, whose `source` is an index into those files. Cheap to work
/// out, so a change in the clips can be noticed ([`Layout::of`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    name: String,
    /// The track's place among the project's tracks.
    order: u32,
    tempo_bpm: f64,
    /// The song's tempo map and time signatures for the plug-in's musical context, so its
    /// timeline counts bars as the song does.
    tempo: Vec<(f64, f64)>,
    bars: Vec<(u32, u32, f64)>,
    files: Vec<(String, String)>,
    regions: Vec<AraRegion>,
}

impl Layout {
    /// The layout for a plug-in on `track`: its enabled audio clips whose files the pool holds.
    pub(crate) fn of(project: &Project, audio: &AudioPool, track: TrackId) -> Self {
        let name = project.shown_track_name(track);
        let order =
            project.tracks.iter().position(|t| t.id == track).map_or(0, |i| u32::try_from(i).unwrap_or(u32::MAX));
        let map = project.transport.tempo_map(1.0);
        let mut files: Vec<(String, String)> = Vec::new();
        let mut regions = Vec::new();
        let clips = project.arrangement.lanes.iter().filter(|l| l.track == Some(track)).flat_map(|l| &l.clips);
        // With a clip host only the clips that name it are its regions (#990); a track-wide ARA
        // plug-in from before has every audio clip of its track.
        let hosted = crate::clip_ara::has_host(project, track);
        for clip in clips.filter(|c| c.enabled && (c.ara || !hosted)) {
            if regions.len() >= MAX_ARA_OBJECTS || files.len() >= MAX_ARA_OBJECTS {
                break;
            }
            let Some(content) = project.contents.iter().find(|c| c.id == clip.content) else { continue };
            let lane = content.lanes.iter().find_map(|lane| match &lane.data {
                LaneData::Audio(region) if lane.target.is_none() || lane.target == Some(track) => Some(region),
                _ => None,
            });
            let Some(region) = lane else { continue };
            let key = region.pool_key().into_owned();
            if !audio.files.get(&key).is_some_and(|d| d.channels > 0 && d.sample_rate > 0) {
                continue;
            }
            let mut source = None;
            for (pass, (from, beat, length)) in passes(clip).into_iter().enumerate() {
                if regions.len() >= MAX_ARA_OBJECTS {
                    break;
                }
                let start = map.time_at(clip.start + from);
                let duration = map.time_at(clip.start + from + length) - start;
                let offset = file_seconds(region, beat, project.transport.tempo_bpm);
                if !(start.is_finite() && duration.is_finite() && duration > 0.0 && offset.is_finite()) {
                    continue;
                }
                let index = *source.get_or_insert_with(|| {
                    files.iter().position(|(k, _)| *k == key).unwrap_or_else(|| {
                        files.push((key.clone(), file_name(&region.file)));
                        files.len() - 1
                    })
                });
                // The first pass is the clip's own region (its notes, its selection); a repeat
                // of its loop is named after it.
                let id = if pass == 0 { format!("clip-{}", clip.id.0) } else { format!("clip-{}-{pass}", clip.id.0) };
                regions.push(AraRegion {
                    id,
                    name: content.name.clone(),
                    source: index,
                    modification: format!("content-{}", content.id.0),
                    start,
                    duration,
                    offset: offset.max(0.0),
                    clone_of: None,
                });
            }
        }
        let (tempo, bars) = musical_context(project);
        Self { name, order, tempo_bpm: project.transport.tempo_bpm, tempo, bars, files, regions }
    }

    /// For each region, in order, the modification its new one is a copy of, as it was in
    /// `before` (the layout the plug-in has): a clip that got its own content keeps its edits
    /// (ADR 0033 decision 5). The same clip playing another content now (its notes copied on
    /// edit) copies what it played; a new clip copies the region of the same file whose span
    /// holds its own (a paste, a split's second part), the narrowest one. `None` for a
    /// modification the plug-in has already, or with nothing to copy.
    pub(crate) fn clone_origins(&self, before: &Layout) -> Vec<Option<String>> {
        let file = |layout: &Layout, region: &AraRegion| layout.files.get(region.source).map(|(key, _)| key.clone());
        self.regions
            .iter()
            .map(|region| {
                if before.regions.iter().any(|r| r.modification == region.modification) {
                    return None;
                }
                let key = file(self, region)?;
                let same_file = || before.regions.iter().filter(|r| file(before, r).as_ref() == Some(&key));
                if let Some(was) = same_file().find(|r| r.id == region.id) {
                    return Some(was.modification.clone());
                }
                let end = region.offset + region.duration;
                same_file()
                    .filter(|r| r.offset <= region.offset + 1e-6 && r.offset + r.duration >= end - 1e-6)
                    .min_by(|a, b| a.duration.total_cmp(&b.duration))
                    .map(|r| r.modification.clone())
            })
            .collect()
    }

    /// Whether it has the region `id` (`clip-<id>`).
    pub(crate) fn has_region(&self, id: &str) -> bool {
        self.regions.iter().any(|r| r.id == id)
    }

    /// Whether it has no regions: the plug-in then runs without a document.
    pub(crate) fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The document, with each file's samples put in shared memory from `audio`. The sources
    /// must live as long as the plug-in does. `None` when there are no regions, or a file's
    /// samples cannot be shared.
    pub(crate) fn document(&self, audio: &AudioPool) -> Option<(AraDocument, Vec<(String, AudioSource)>)> {
        if self.is_empty() {
            return None;
        }
        let mut sources = Vec::with_capacity(self.files.len());
        let document = self.document_with(audio, &mut sources)?;
        Some((document, sources))
    }

    /// The document for a plug-in already bound to one, for an update: the files `kept` holds
    /// in shared memory (by audio pool key) are named as they are, and the others are put
    /// there from `audio` and added to `kept`. It may have no regions (every clip gone).
    /// `None` when a file's samples cannot be shared.
    pub(crate) fn document_with(
        &self,
        audio: &AudioPool,
        kept: &mut Vec<(String, AudioSource)>,
    ) -> Option<AraDocument> {
        let mut described = Vec::with_capacity(self.files.len());
        for (key, name) in &self.files {
            let address = match kept.iter().find(|(k, _)| k == key) {
                Some((_, source)) => source.address(),
                None => {
                    let data = audio.files.get(key)?;
                    let made = AudioSource::from_interleaved(
                        &data.samples,
                        u32::from(data.channels),
                        f64::from(data.sample_rate),
                    )
                    .ok()?;
                    let address = made.address();
                    kept.push((key.clone(), made));
                    address
                }
            };
            described.push(AraSource { id: key.clone(), name: name.clone(), address });
        }
        Some(AraDocument {
            name: self.name.clone(),
            document: String::new(),
            sequence: self.name.clone(),
            order: self.order,
            tempo_bpm: self.tempo_bpm,
            tempo: self.tempo.clone(),
            bars: self.bars.clone(),
            sources: described,
            regions: self.regions.clone(),
        })
    }
}

/// A bar signature for ARA: numerator, denominator, and the quarter note of the bar's start.
type BarSignature = (u32, u32, f64);

/// The song's tempo map as ARA tempo entries (seconds, quarter notes) and its time signatures
/// as bar signatures (numerator, denominator, quarter note of the bar's start). A beat is a
/// quarter note. Between two entries ARA takes the tempo as even, so a ramp gets one a beat.
fn musical_context(project: &Project) -> (Vec<(f64, f64)>, Vec<BarSignature>) {
    let transport = &project.transport;
    let map = transport.tempo_map(1.0);
    let mut changes: Vec<&suhina_model::TempoPoint> =
        transport.tempo_changes.iter().filter(|p| p.beat.is_finite() && p.beat > 0.0).collect();
    changes.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let mut beats: Vec<f64> = vec![0.0];
    let mut ramp = false;
    for change in &changes {
        if ramp {
            let mut beat = beats.last().copied().unwrap_or(0.0).floor() + 1.0;
            while beat < change.beat && beats.len() < MAX_ARA_OBJECTS - 2 {
                beats.push(beat);
                beat += 1.0;
            }
        }
        if beats.last().is_some_and(|last| change.beat > *last) && beats.len() < MAX_ARA_OBJECTS - 1 {
            beats.push(change.beat);
        }
        ramp = change.ramp;
    }
    // One past the last, so the last tempo is known too.
    beats.push(beats.last().copied().unwrap_or(0.0) + 1.0);
    let tempo: Vec<(f64, f64)> = beats.iter().map(|&beat| (map.time_at(beat), beat)).collect();
    let tempo = if tempo.windows(2).all(|w| w[1].0 > w[0].0) && tempo.iter().all(|(s, _)| s.is_finite()) {
        tempo
    } else {
        Vec::new()
    };
    let meter = transport.meter();
    let first = transport.time_signature;
    let mut bars = vec![(u32::from(first.numerator), u32::from(first.denominator), 0.0)];
    for (_, beat, signature) in meter.changes().take(MAX_ARA_OBJECTS - 1) {
        if bars.last().is_some_and(|(_, _, at)| beat > *at) {
            bars.push((u32::from(signature.numerator), u32::from(signature.denominator), beat));
        }
    }
    (tempo, bars)
}

/// The stretches a clip plays, as the engine plays them: each as (beats from the clip's start,
/// the content beat it starts at, its length in beats). One without a loop; with one, the part
/// up to the loop's end and then a pass through the loop for each repeat, the last cut at the
/// clip's end.
fn passes(clip: &Clip) -> Vec<(f64, f64, f64)> {
    let length = clip.length;
    let looped = clip.loop_range.filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start);
    let Some(range) = looped else { return vec![(0.0, clip.offset, length)] };
    let (start, end) = (range.start, range.end);
    let mut beat = if clip.offset >= end { start } else { clip.offset };
    let mut passes = Vec::new();
    let mut from = 0.0;
    while from < length - 1e-9 && passes.len() < MAX_ARA_OBJECTS {
        let take = (end - beat).min(length - from);
        if take <= 1e-9 {
            break;
        }
        passes.push((from, beat, take));
        from += take;
        beat = start;
    }
    passes
}

/// Seconds into the file at the content's `beat`, as the engine reads it: through the warp
/// markers of a warped region that has two or more, otherwise at `tempo_bpm`.
fn file_seconds(region: &AudioRegion, beat: f64, tempo_bpm: f64) -> f64 {
    let mut markers: Vec<(f64, f64)> = region
        .warp_markers
        .iter()
        .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
        .map(|m| (m.time, m.audio_time))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    markers.dedup_by(|a, b| a.0 == b.0);
    if region.warped && markers.len() >= 2 {
        let i = markers.partition_point(|(b, _)| *b <= beat).clamp(1, markers.len() - 1);
        let (Some(&(b0, s0)), Some(&(b1, s1))) = (markers.get(i - 1), markers.get(i)) else { return 0.0 };
        return if b1 > b0 { s0 + (beat - b0) * (s1 - s0) / (b1 - b0) } else { s0 };
    }
    let bpm = if tempo_bpm.is_finite() && tempo_bpm > 0.0 { tempo_bpm } else { 120.0 };
    beat * 60.0 / bpm
}

fn file_name(file: &str) -> String {
    file.rsplit(['/', '\\']).next().unwrap_or(file).to_string()
}

#[cfg(test)]
#[path = "ara_tests.rs"]
mod tests;
