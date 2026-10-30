// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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
    warped.warp_markers = vec![WarpMarker { time: 0.0, audio_time: 0.5 }, WarpMarker { time: 4.0, audio_time: 2.5 }];
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
fn a_looped_clip_is_a_region_for_each_pass_of_one_modification() {
    let (mut project, audio) = project();
    // Clip 1: four beats from content beat 1, looping beats 1 to 2.5: the first pass to the
    // loop's end, then passes of a beat and a half, the last cut at the clip's end.
    let looped = &mut project.arrangement.lanes[0].clips[0];
    looped.loop_range = Some(suhina_model::LoopRange { start: 1.0, end: 2.5 });
    let layout = Layout::of(&project, &audio, TrackId(1));
    let (document, _) = layout.document(&audio).unwrap();
    let passes: Vec<_> = document
        .regions
        .iter()
        .filter(|r| r.modification == "content-10" && r.id.starts_with("clip-1"))
        .map(|r| (r.id.as_str(), r.start, r.duration, r.offset))
        .collect();
    assert_eq!(passes, [("clip-1", 0.0, 0.75, 0.5), ("clip-1-1", 0.75, 0.75, 0.5), ("clip-1-2", 1.5, 0.5, 0.5)]);
    // The clip's notes and selection go by its first pass.
    assert!(layout.has_region("clip-1"));
}

#[test]
fn the_document_carries_the_songs_tempo_map_and_time_signatures() {
    let (mut project, audio) = project();
    // 120 bpm, then 60 from beat 8; 4/4, then 3/4 from bar 2 (beat 8).
    project.transport.tempo_changes = vec![suhina_model::TempoPoint { beat: 8.0, bpm: 60.0, ramp: false }];
    project.transport.time_signature_changes = vec![suhina_model::TimeSignatureChange {
        bar: 2,
        time_signature: suhina_model::TimeSignature { numerator: 3, denominator: 4 },
    }];
    let (document, _) = Layout::of(&project, &audio, TrackId(1)).document(&audio).unwrap();
    // Half a second a beat up to beat 8 (4 s), a second a beat after it.
    assert_eq!(document.tempo, [(0.0, 0.0), (4.0, 8.0), (5.0, 9.0)]);
    assert_eq!(document.bars, [(4, 4, 0.0), (3, 4, 8.0)]);
    // An older engine's document, without them: an even tempo in 4/4.
    let even = AraDocument { tempo: Vec::new(), bars: Vec::new(), ..document };
    assert_eq!(even.tempo_entries(), [(0.0, 0.0), (0.5, 1.0)]);
    assert_eq!(even.bar_signatures(), [(4, 4, 0.0)]);
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

#[test]
fn clip_notes_know_their_lowest_and_highest_pitch() {
    let note = |pitch| AraNote { start: 0.0, length: 0.5, pitch, volume: 1.0 };
    assert!(ClipNotes::new(Vec::new()).is_none());
    let notes = ClipNotes::new(vec![note(64), note(57), note(71)]).unwrap();
    assert_eq!((notes.low, notes.high, notes.notes.len()), (57, 71, 3));
}

#[test]
fn a_clip_with_its_own_content_copies_the_edits_of_the_one_it_came_from() {
    let (mut project, audio) = project();
    let before = Layout::of(&project, &audio, TrackId(1));
    let original = project.arrangement.lanes[0].clips[0].clone();
    let mut content = project.content(original.content).cloned().unwrap();
    let old = format!("content-{}", content.id.0);
    content.id = ContentId(project.contents.iter().map(|c| c.id.0).max().unwrap() + 1);
    let new = content.id;
    project.contents.push(content);
    // A pasted copy: a new clip, a new content of the same file, the same span of it.
    let mut pasted = original.clone();
    pasted.id = ClipId(999);
    pasted.start = original.start + 64.0;
    pasted.content = new;
    project.arrangement.lanes[0].clips.push(pasted);
    let now = Layout::of(&project, &audio, TrackId(1));
    let origins = now.clone_origins(&before);
    let at = now.regions.iter().position(|r| r.id == "clip-999").unwrap();
    assert_eq!(origins[at], Some(old.clone()));
    assert!(origins.iter().enumerate().all(|(i, o)| i == at || o.is_none()), "{origins:?}");
    // The same clip playing a content of its own now copies what it played.
    project.arrangement.lanes[0].clips.pop();
    project.arrangement.lanes[0].clips[0].content = new;
    let now = Layout::of(&project, &audio, TrackId(1));
    let at = now.regions.iter().position(|r| r.id == format!("clip-{}", original.id.0)).unwrap();
    assert_eq!(now.clone_origins(&before)[at], Some(old));
    // Nothing changed: nothing to copy.
    assert!(before.clone_origins(&before).iter().all(Option::is_none));
}

/// With a clip host only the clips that name it are its regions; a track-wide ARA plug-in from
/// before made a clip host names every audio clip of its track (#990).
#[test]
fn a_clip_host_has_only_the_clips_that_name_it() {
    let (mut project, audio) = project();
    let mut channel = suhina_model::Channel::new(suhina_model::ChannelId(1), "Vocals");
    channel.devices.push(suhina_model::Device::new(
        "Melodyne",
        suhina_model::DeviceKind::Vst3 { id: "5653544d656c6f64796e6500000000".into() },
    ));
    project.channels.push(channel);
    project.tracks[0].channel = Some(suhina_model::ChannelId(1));
    // Before: the plug-in has every audio clip of the track.
    assert!(Layout::of(&project, &audio, TrackId(1)).has_region("clip-2"));
    assert!(crate::clip_ara::make_host(&mut project, TrackId(1)));
    assert!(project.arrangement.lanes[0].clips.iter().all(|c| c.ara), "every audio clip names it");
    assert!(!crate::clip_ara::make_host(&mut project, TrackId(1)), "once");
    project.arrangement.lanes[0].clips[1].ara = false;
    let layout = Layout::of(&project, &audio, TrackId(1));
    assert!(layout.has_region("clip-1") && !layout.has_region("clip-2"));
}
