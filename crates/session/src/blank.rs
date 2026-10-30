// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The project a new blank project starts as: two note tracks, two audio tracks, two returns (or
//! none, Settings › Arrangement › Returns in new projects), the main channel and eight empty
//! scenes, so there is somewhere to put clips and devices before tracks can be added.

use suhina_model::{
    Arrangement, ArrangementLane, Channel, ChannelId, ChannelRole, ContentKinds, Extensions, Launcher, LauncherColumn,
    Project, Scene, Slot, Track, TrackId, Transport,
};

/// Scenes a blank project has.
const SCENES: usize = 8;

/// A blank project with the A and B return tracks.
pub fn blank_project() -> Project {
    blank_project_with(true)
}

/// A blank project, with the A and B return tracks when `returns`, or with none (#898).
pub fn blank_project_with(returns: bool) -> Project {
    let main = ChannelId(100);
    let all_returns = [(ChannelId(90), "A Return"), (ChannelId(91), "B Return")];
    let returns = if returns { &all_returns[..] } else { &[] };
    let tracks = [("# MIDI", true), ("# MIDI", true), ("# Audio", false), ("# Audio", false)];
    let channel = |id: ChannelId, name: &str, role: ChannelRole, output: Option<ChannelId>| Channel {
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
    };
    let mut project = Project { transport: Transport::default(), ..Project::default() };
    let mut lanes = Vec::new();
    let mut columns = Vec::new();
    for (index, (name, notes)) in tracks.into_iter().enumerate() {
        let id = index as u32 + 1;
        project.tracks.push(Track {
            id: TrackId(id),
            name: name.into(),
            color: None,
            content: ContentKinds { notes, audio: !notes },
            channel: Some(ChannelId(id)),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        });
        project.channels.push(channel(ChannelId(id), name, ChannelRole::Regular, Some(main)));
        lanes.push(ArrangementLane { name: name.into(), track: Some(TrackId(id)), ..ArrangementLane::default() });
        columns.push(LauncherColumn { track: TrackId(id), slots: vec![Slot { clip: None, has_stop: true }; SCENES] });
    }
    for (offset, &(id, name)) in returns.iter().enumerate() {
        let track = TrackId(10 + offset as u32);
        project.tracks.push(Track {
            id: track,
            name: name.into(),
            color: None,
            content: ContentKinds::default(),
            channel: Some(id),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        });
        project.channels.push(channel(id, name, ChannelRole::Effect, Some(main)));
    }
    project.channels.push(channel(main, "Main", ChannelRole::Master, None));
    project.arrangement = Arrangement { lanes, markers: Vec::new(), takes: Vec::new() };
    project.launcher = Launcher {
        scenes: (1..=SCENES).map(|n| Scene { name: n.to_string(), ..Scene::default() }).collect(),
        columns,
        mappings: Vec::new(),
    };
    // Its lanes and scenes get ids in order; a handful of objects cannot use up the counter.
    let _ = project.assign_ids();
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_project_holds_together() {
        let project = blank_project();
        assert_eq!(project.validate(), Ok(()));
        assert_eq!(project.tracks.len(), 6);
        assert_eq!(project.launcher.scenes.len(), SCENES);
        assert!(project.contents.is_empty());
    }

    #[test]
    fn a_blank_project_without_returns_has_none() {
        let project = blank_project_with(false);
        assert_eq!(project.validate(), Ok(()));
        assert_eq!(project.tracks.len(), 4);
        assert!(project.channels.iter().all(|c| c.role != ChannelRole::Effect));
        assert!(project.channels.iter().all(|c| c.sends.is_empty()));
    }
}
