// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A small self-made project: four tracks, two return tracks, six scenes with clips and a
//! 48-bar arrangement. The app opens it with `--demo`, and the UI tests draw it.

use suhina_engine::model::{
    Arrangement, ArrangementLane, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds,
    ContentLane, Extensions, LaneData, Launcher, LauncherColumn, LoopRange, Note, Project, Scene, Send, Slot, Track,
    TrackId, Transport,
};

/// Clip names per track and scene; `None` is an empty slot with a stop button.
const CLIPS: [(&str, [Option<&str>; 6]); 4] = [
    ("Drums", [Some("Beat A"), Some("Beat A"), Some("Beat B"), Some("Fill"), None, Some("Beat B")]),
    ("Bass", [Some("Root"), Some("Walk"), Some("Walk"), None, Some("Drone"), Some("Root")]),
    ("Keys", [None, Some("Chords 1"), Some("Chords 2"), Some("Chords 2"), Some("Pad"), None]),
    ("Vox", [None, None, Some("Hook"), Some("Hook dub"), None, Some("Verse")]),
];

const SCENES: [&str; 6] = ["Intro", "Groove", "Build", "Break", "Drop", "Outro"];

/// Arrangement clips per track: the scene whose clip content they place, start and length in
/// beats. Clips longer than their one-bar content loop it.
const ARRANGEMENT: [&[(usize, f64, f64)]; 4] = [
    &[(0, 0.0, 64.0), (2, 64.0, 48.0), (3, 112.0, 16.0), (2, 128.0, 64.0)],
    &[(0, 32.0, 32.0), (1, 64.0, 64.0), (4, 128.0, 32.0), (0, 160.0, 32.0)],
    &[(1, 64.0, 32.0), (2, 96.0, 32.0), (4, 128.0, 64.0)],
    &[(2, 96.0, 32.0), (3, 128.0, 16.0), (5, 160.0, 32.0)],
];

pub fn demo_project() -> Project {
    let mut project = Project {
        transport: Transport { tempo_bpm: 122.0, ..Transport::default() },
        arrangement: Arrangement::default(),
        ..Project::default()
    };
    let main = ChannelId(100);
    let returns = [(ChannelId(90), "A Reverb"), (ChannelId(91), "B Delay")];

    let mut columns = Vec::new();
    for (index, (name, clips)) in CLIPS.iter().enumerate() {
        let id = index as u32 + 1;
        project.tracks.push(Track {
            id: TrackId(id),
            name: (*name).into(),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(id)),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        });
        let mut channel = channel(ChannelId(id), name, ChannelRole::Regular, Some(main));
        channel.volume = [1.0, 0.71, 0.5, 0.84][index];
        channel.pan = [0.0, 0.0, -0.3, 0.2][index];
        channel.sends = vec![Send { target: returns[0].0, level: 0.3, pre_fader: false }];
        project.channels.push(channel);

        let slots = clips
            .iter()
            .enumerate()
            .map(|(scene, clip)| match clip {
                None => Slot { clip: None, has_stop: true },
                Some(clip_name) => {
                    let content = ContentId(id * 10 + scene as u32);
                    project.contents.push(content_of(content, clip_name, index as u8));
                    Slot { clip: Some(clip_of(content)), has_stop: true }
                }
            })
            .collect();
        columns.push(LauncherColumn { track: TrackId(id), slots });
        let placed = ARRANGEMENT.get(index).copied().unwrap_or_default();
        let clips = placed
            .iter()
            .map(|&(scene, start, length)| Clip {
                start,
                length,
                loop_range: Some(LoopRange { start: 0.0, end: 4.0 }),
                ..clip_of(ContentId(id * 10 + scene as u32))
            })
            .collect();
        project.arrangement.lanes.push(ArrangementLane {
            id: Default::default(),
            name: (*name).into(),
            track: Some(TrackId(id)),
            clips,
        });
    }
    for (offset, (channel_id, name)) in returns.iter().enumerate() {
        let id = 10 + offset as u32;
        project.tracks.push(Track {
            id: TrackId(id),
            name: (*name).into(),
            color: None,
            content: ContentKinds::default(),
            channel: Some(*channel_id),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        });
        project.channels.push(channel(*channel_id, name, ChannelRole::Effect, Some(main)));
    }
    project.channels.push(channel(main, "Main", ChannelRole::Master, None));
    project.launcher = Launcher {
        mappings: Vec::new(),
        scenes: SCENES.iter().map(|name| Scene { name: (*name).into(), ..Scene::default() }).collect(),
        columns,
    };
    // Lanes, clips and scenes get their ids in order, as a session would give them.
    let _ = project.assign_ids();
    project
}

fn channel(id: ChannelId, name: &str, role: ChannelRole, output: Option<ChannelId>) -> Channel {
    Channel {
        id,
        name: name.into(),
        role,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
        automation: Vec::new(),
    }
}

/// A one-bar clip: a short figure around a root that differs per track.
fn content_of(id: ContentId, name: &str, track: u8) -> ClipContent {
    let root = [36u8, 40, 60, 67][usize::from(track) % 4];
    let notes = (0..4)
        .map(|beat| Note {
            time: f64::from(beat),
            duration: 0.5,
            key: root + [0u8, 7, 3, 5][beat as usize % 4],
            velocity: 0.8,
            release_velocity: 0.5,
            channel: 0,
            muted: false,
        })
        .collect();
    ClipContent {
        id,
        name: name.into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
        scale: None,
    }
}

fn clip_of(content: ContentId) -> Clip {
    Clip {
        id: Default::default(),
        content,
        start: 0.0,
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
        extensions: Extensions::default(),
    }
}

/// The demo project with Drums and Bass inside a group, "Group", standing in front of them, as
/// `track.group` leaves them. The view tests that fold or route a group build on it.
#[cfg(test)]
pub(crate) fn grouped_project() -> Project {
    use suhina_engine::model::{Channel, ChannelId, ChannelRole, Track, TrackId};
    let mut project = demo_project();
    let (group, channel) = (TrackId(50), ChannelId(150));
    project.channels.push(Channel {
        id: channel,
        name: "Group".into(),
        role: ChannelRole::Submix,
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    let mut group_track = project.tracks[0].clone();
    group_track.id = group;
    group_track.name = "Group".into();
    group_track.channel = Some(channel);
    group_track.parent = None;
    group_track.content = Default::default();
    let held: Vec<Track> = project.tracks.iter().take(2).cloned().collect();
    project.tracks.retain(|t| !held.iter().any(|h| h.id == t.id));
    project.tracks.insert(0, group_track);
    for (at, mut track) in held.into_iter().enumerate() {
        track.parent = Some(group);
        project.tracks.insert(1 + at, track);
    }
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_project_validates() {
        assert_eq!(demo_project().validate(), Ok(()));
    }
}
