// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ARA document a plug-in that edits clips is loaded with (ADR 0028): the audio clips on
//! its track's arrangement lanes as playback regions, each file they play as an audio source in
//! shared memory.
//!
//! For now a clip is described as it plays at its file's own speed: a warped clip's start and
//! end follow the song, but the plug-in reads it unstretched, and loops are not repeated.

use suhina_engine::AudioPool;
use suhina_model::{AudioRegion, Clip, LaneData, Project, TrackId};
use suhina_plugins::AudioSource;
use suhina_plugins::protocol::{AraDocument, AraRegion, AraSource, MAX_ARA_OBJECTS};

/// What a track's document holds, without the samples: its name, the files its clips play (by
/// audio pool key) and the regions, whose `source` is an index into those files. Cheap to work
/// out, so a change in the clips can be noticed ([`Layout::of`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    name: String,
    tempo_bpm: f64,
    files: Vec<(String, String)>,
    regions: Vec<AraRegion>,
}

impl Layout {
    /// The layout for a plug-in on `track`: its enabled audio clips whose files the pool holds.
    pub(crate) fn of(project: &Project, audio: &AudioPool, track: TrackId) -> Self {
        let name = project.tracks.iter().find(|t| t.id == track).map_or_else(String::new, |t| t.name.clone());
        let map = project.transport.tempo_map(1.0);
        let mut files: Vec<(String, String)> = Vec::new();
        let mut regions = Vec::new();
        let clips = project.arrangement.lanes.iter().filter(|l| l.track == Some(track)).flat_map(|l| &l.clips);
        for clip in clips.filter(|c| c.enabled) {
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
            let start = map.time_at(clip.start);
            let duration = map.time_at(clip.start + clip.length) - start;
            let offset = file_seconds(region, clip, project.transport.tempo_bpm);
            if !(start.is_finite() && duration.is_finite() && duration > 0.0 && offset.is_finite()) {
                continue;
            }
            let source = files.iter().position(|(k, _)| *k == key).unwrap_or_else(|| {
                files.push((key, file_name(&region.file)));
                files.len() - 1
            });
            regions.push(AraRegion {
                id: format!("clip-{}", clip.id.0),
                name: content.name.clone(),
                source,
                modification: format!("content-{}", content.id.0),
                start,
                duration,
                offset: offset.max(0.0),
            });
        }
        Self { name, tempo_bpm: project.transport.tempo_bpm, files, regions }
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
            sequence: self.name.clone(),
            tempo_bpm: self.tempo_bpm,
            sources: described,
            regions: self.regions.clone(),
        })
    }
}

/// Seconds into the file where `clip` starts playing, as the engine reads it: through the warp
/// markers of a warped region that has two or more, otherwise at the song's starting tempo.
fn file_seconds(region: &AudioRegion, clip: &Clip, tempo_bpm: f64) -> f64 {
    let mut markers: Vec<(f64, f64)> = region
        .warp_markers
        .iter()
        .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
        .map(|m| (m.time, m.audio_time))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    markers.dedup_by(|a, b| a.0 == b.0);
    if region.warped && markers.len() >= 2 {
        let beat = clip.offset;
        let i = markers.partition_point(|(b, _)| *b <= beat).clamp(1, markers.len() - 1);
        let (Some(&(b0, s0)), Some(&(b1, s1))) = (markers.get(i - 1), markers.get(i)) else { return 0.0 };
        return if b1 > b0 { s0 + (beat - b0) * (s1 - s0) / (b1 - b0) } else { s0 };
    }
    let bpm = if tempo_bpm.is_finite() && tempo_bpm > 0.0 { tempo_bpm } else { 120.0 };
    clip.offset * 60.0 / bpm
}

fn file_name(file: &str) -> String {
    file.rsplit(['/', '\\']).next().unwrap_or(file).to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use suhina_engine::AudioData;
    use suhina_model::{ArrangementLane, ClipContent, ClipId, ContentId, ContentLane, Track, WarpMarker};

    use super::*;

    fn region(file: &str) -> AudioRegion {
        AudioRegion {
            file: file.into(),
            absolute_file: None,
            sample_rate: Some(48_000),
            channels: Some(2),
            duration_seconds: None,
            warped: false,
            stretch: Default::default(),
            warp_markers: Vec::new(),
            gain: 1.0,
            transpose: 0.0,
            stream: None,
        }
    }

    fn content(id: u32, region: AudioRegion) -> ClipContent {
        ClipContent {
            id: ContentId(id),
            name: format!("take {id}"),
            length: 8.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Default::default(),
            scale: None,
        }
    }

    fn clip(id: u32, content: u32, start: f64, length: f64, offset: f64) -> Clip {
        let mut clip = Clip::new(ContentId(content), start, length);
        clip.id = ClipId(id);
        clip.offset = offset;
        clip
    }

    fn project() -> (Project, AudioPool) {
        let mut project = Project::default();
        project.transport.tempo_bpm = 120.0;
        let mut track = Track::new(TrackId(1), "Vocals");
        track.content.audio = true;
        project.tracks.push(track);
        let mut warped = region("audio/vocal.wav");
        warped.warped = true;
        warped.warp_markers =
            vec![WarpMarker { time: 0.0, audio_time: 0.5 }, WarpMarker { time: 4.0, audio_time: 2.5 }];
        project.contents =
            vec![content(10, region("audio/vocal.wav")), content(11, warped), content(12, region("gone.wav"))];
        project.arrangement.lanes.push(ArrangementLane {
            track: Some(TrackId(1)),
            clips: vec![
                clip(1, 10, 0.0, 4.0, 1.0),
                clip(2, 10, 8.0, 2.0, 0.0),
                clip(3, 11, 4.0, 4.0, 2.0),
                clip(4, 12, 0.0, 1.0, 0.0),
            ],
            ..ArrangementLane::default()
        });
        // A clip of another track is not in the document.
        project.arrangement.lanes.push(ArrangementLane {
            track: Some(TrackId(2)),
            clips: vec![clip(5, 10, 0.0, 4.0, 0.0)],
            ..ArrangementLane::default()
        });
        let mut audio = AudioPool::new();
        let data = AudioData { sample_rate: 48_000, channels: 2, samples: vec![0.25; 48_000 * 2 * 4] };
        audio.files.insert("audio/vocal.wav".into(), Arc::new(data));
        (project, audio)
    }

    #[test]
    fn the_tracks_audio_clips_become_regions_of_one_source() {
        let (project, audio) = project();
        let (document, sources) = Layout::of(&project, &audio, TrackId(1)).document(&audio).unwrap();
        assert_eq!((document.name.as_str(), document.sequence.as_str()), ("Vocals", "Vocals"));
        // One file, one source; the clip whose file the pool lacks is left out.
        assert_eq!(sources.len(), 1);
        assert_eq!(document.sources.len(), 1);
        assert_eq!(document.sources[0].name, "vocal.wav");
        assert_eq!(AudioSource::open(&document.sources[0].address).unwrap().frames(), 48_000 * 4);
        let regions: Vec<_> = document
            .regions
            .iter()
            .map(|r| (r.id.as_str(), r.modification.as_str(), r.start, r.duration, r.offset))
            .collect();
        assert_eq!(
            regions,
            [
                // At 120 bpm a beat is half a second.
                ("clip-1", "content-10", 0.0, 2.0, 0.5),
                ("clip-2", "content-10", 4.0, 1.0, 0.0),
                // Warped: two beats into the markers is 1.5 s into the file.
                ("clip-3", "content-11", 2.0, 2.0, 1.5),
            ]
        );
    }

    #[test]
    fn a_track_without_audio_clips_has_no_document_and_moves_are_noticed() {
        let (mut project, audio) = project();
        assert!(Layout::of(&project, &audio, TrackId(3)).document(&audio).is_none());
        let before = Layout::of(&project, &audio, TrackId(1));
        // Moving a clip changes the layout; a clip of another track does not.
        project.arrangement.lanes[1].clips[0].start = 16.0;
        assert_eq!(Layout::of(&project, &audio, TrackId(1)), before);
        project.arrangement.lanes[0].clips[0].start = 1.0;
        assert_ne!(Layout::of(&project, &audio, TrackId(1)), before);
        project.arrangement.lanes[0].clips.iter_mut().for_each(|c| c.enabled = false);
        assert!(Layout::of(&project, &audio, TrackId(1)).is_empty());
    }

    #[test]
    fn an_update_names_the_files_already_shared_and_shares_only_new_ones() {
        let (mut project, audio) = project();
        let (first, mut kept) = Layout::of(&project, &audio, TrackId(1)).document(&audio).unwrap();
        let shared = kept.len();
        let addresses: Vec<_> = first.sources.iter().map(|s| s.address.clone()).collect();
        // A moved clip: the same files, at the same addresses, nothing shared again.
        project.arrangement.lanes[0].clips[0].start += 4.0;
        let moved = Layout::of(&project, &audio, TrackId(1)).document_with(&audio, &mut kept).unwrap();
        assert_eq!(kept.len(), shared);
        assert_eq!(moved.sources.iter().map(|s| s.address.clone()).collect::<Vec<_>>(), addresses);
        assert_ne!(moved.regions[0].start, first.regions[0].start);
        assert_eq!(moved.regions[0].id, first.regions[0].id);
        // Every clip gone: a document with no regions, for the update that takes them out.
        project.arrangement.lanes[0].clips.clear();
        let empty = Layout::of(&project, &audio, TrackId(1)).document_with(&audio, &mut kept).unwrap();
        assert!(empty.regions.is_empty());
        assert_eq!(kept.len(), shared, "the files stay shared, for an undo");
    }
}
