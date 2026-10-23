// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`ProjectBuilder`]: the project a test needs in a few lines, in place of a literal of every
//! track, channel, content, lane and column.

use suhina_model::{
    ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds,
    ContentLane, Extensions, LaneData, LauncherColumn, Note, Project, Scene, Slot, Track, TrackId,
};

/// Builds a [`Project`] step by step. Each track gets the channel with its own number, named
/// `C<id>`, into the main output; arrangement lanes and launcher columns are made the first time
/// a clip needs them. [`build`](Self::build) gives lanes, clips and scenes their ids, as a session
/// does; [`build_without_ids`](Self::build_without_ids) leaves them unset.
///
/// ```
/// use suhina_model::{Clip, ContentId, Note};
/// use suhina_test_support::ProjectBuilder;
///
/// let project = ProjectBuilder::new()
///     .midi_track(1, "Bass")
///     .notes(1, "Riff", 4.0, vec![Note::new(0.0, 1.0, 36, 0.8)])
///     .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
///     .build();
/// assert_eq!(project.arrangement.lanes[0].clips.len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ProjectBuilder {
    project: Project,
}

impl ProjectBuilder {
    /// An empty project at the default tempo.
    pub fn new() -> Self {
        Self::default()
    }

    /// A track for notes, `id` on channel `id`.
    pub fn midi_track(self, id: u32, name: &str) -> Self {
        self.track_of(id, name, ContentKinds { notes: true, audio: false })
    }

    /// A track for audio, `id` on channel `id`.
    pub fn audio_track(self, id: u32, name: &str) -> Self {
        self.track_of(id, name, ContentKinds { notes: false, audio: true })
    }

    /// A track `id` that holds no clips (a group's or a return's), on channel `id` with `role`.
    pub fn bus_track(mut self, id: u32, name: &str, role: ChannelRole) -> Self {
        self = self.track_of(id, name, ContentKinds::default());
        self.channel_mut(id).role = role;
        self
    }

    /// `track` as it is, and `channel` when it has one.
    pub fn track(mut self, track: Track, channel: Option<Channel>) -> Self {
        self.project.tracks.push(track);
        self.project.channels.extend(channel);
        self
    }

    /// A channel without a track: the master, or one a test routes to.
    pub fn channel(mut self, channel: Channel) -> Self {
        self.project.channels.push(channel);
        self
    }

    /// Sends channel `id` into channel `to` rather than the main output.
    pub fn output(mut self, id: u32, to: u32) -> Self {
        self.channel_mut(id).output = Some(ChannelId(to));
        self
    }

    /// Puts track `id` inside the group track `parent`.
    pub fn parent(mut self, id: u32, parent: u32) -> Self {
        self.track_mut(id).parent = Some(TrackId(parent));
        self
    }

    /// A content `id` of `length` beats holding `notes`, on whichever track plays it.
    pub fn notes(self, id: u32, name: &str, length: f64, notes: Vec<Note>) -> Self {
        self.content(id, name, length, LaneData::Notes(notes))
    }

    /// A content `id` of `length` beats playing `region`.
    pub fn audio(self, id: u32, name: &str, length: f64, region: AudioRegion) -> Self {
        self.content(id, name, length, LaneData::Audio(region))
    }

    /// `clip` on track `track`'s arrangement lane, made (named after the track) the first time.
    pub fn clip(mut self, track: u32, clip: Clip) -> Self {
        let lanes = &mut self.project.arrangement.lanes;
        let index = match lanes.iter().position(|l| l.track == Some(TrackId(track))) {
            Some(index) => index,
            None => {
                // The field, not Project::track: the lanes are borrowed already.
                let name = self.project.tracks.iter().find(|t| t.id == TrackId(track)).map(|t| t.name.clone());
                lanes.push(ArrangementLane {
                    id: Default::default(),
                    name: name.unwrap_or_default(),
                    track: Some(TrackId(track)),
                    clips: Vec::new(),
                });
                lanes.len() - 1
            }
        };
        lanes[index].clips.push(clip);
        self
    }

    /// A scene called `name`, below the others; every column gets an empty slot for it.
    pub fn scene(mut self, name: &str) -> Self {
        let launcher = &mut self.project.launcher;
        launcher.scenes.push(Scene { name: name.into(), ..Scene::default() });
        for column in &mut launcher.columns {
            column.slots.push(Slot::default());
        }
        self
    }

    /// `clip` in track `track`'s slot of scene `scene` (from 0), making the column and the scenes
    /// up to it the first time.
    pub fn slot(mut self, track: u32, scene: usize, clip: Clip) -> Self {
        while self.project.launcher.scenes.len() <= scene {
            let name = format!("Scene {}", self.project.launcher.scenes.len() + 1);
            self = self.scene(&name);
        }
        let launcher = &mut self.project.launcher;
        let scenes = launcher.scenes.len();
        let index = match launcher.columns.iter().position(|c| c.track == TrackId(track)) {
            Some(index) => index,
            None => {
                launcher.columns.push(LauncherColumn { track: TrackId(track), slots: vec![Slot::default(); scenes] });
                launcher.columns.len() - 1
            }
        };
        launcher.columns[index].slots[scene].clip = Some(clip);
        self
    }

    /// The tempo, in beats per minute.
    pub fn tempo(mut self, bpm: f64) -> Self {
        self.project.transport.tempo_bpm = bpm;
        self
    }

    /// Any other change, made to the project as it stands.
    pub fn with(mut self, change: impl FnOnce(&mut Project)) -> Self {
        change(&mut self.project);
        self
    }

    /// The project, with ids on its lanes, clips and scenes as a session gives them.
    pub fn build(self) -> Project {
        with_ids(self.project)
    }

    /// The project as built, without ids: what a file from before them reads as.
    pub fn build_without_ids(self) -> Project {
        self.project
    }

    fn track_of(mut self, id: u32, name: &str, content: ContentKinds) -> Self {
        let track = Track { content, channel: Some(ChannelId(id)), ..Track::new(TrackId(id), name) };
        self.project.tracks.push(track);
        self.project.channels.push(Channel::new(ChannelId(id), format!("C{id}")));
        self
    }

    fn content(mut self, id: u32, name: &str, length: f64, data: LaneData) -> Self {
        self.project.contents.push(ClipContent {
            id: ContentId(id),
            name: name.into(),
            length,
            lanes: vec![ContentLane { target: None, data }],
            extensions: Extensions::default(),
            scale: None,
        });
        self
    }

    fn track_mut(&mut self, id: u32) -> &mut Track {
        self.project.track_mut(TrackId(id)).unwrap_or_else(|| panic!("no track {id}"))
    }

    fn channel_mut(&mut self, id: u32) -> &mut Channel {
        self.project.channel_mut(ChannelId(id)).unwrap_or_else(|| panic!("no channel {id}"))
    }
}

/// `project` with ids on its lanes, clips and scenes, as a session gives them.
pub fn with_ids(mut project: Project) -> Project {
    project.assign_ids().expect("a test project has room for its ids");
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fill_their_columns_and_scenes() {
        let project = ProjectBuilder::new()
            .midi_track(1, "A")
            .midi_track(2, "B")
            .notes(1, "N", 1.0, Vec::new())
            .slot(2, 1, Clip::new(ContentId(1), 0.0, 1.0))
            .scene("Last")
            .build();
        let launcher = &project.launcher;
        assert_eq!(launcher.scenes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Scene 1", "Scene 2", "Last"]);
        assert_eq!(launcher.columns.len(), 1);
        assert_eq!(launcher.columns[0].slots.len(), 3);
        assert!(launcher.columns[0].slots[1].clip.is_some());
        assert!(launcher.scenes.iter().all(|s| s.id != suhina_model::SceneId::NONE));
    }

    #[test]
    fn a_clip_makes_its_tracks_lane_once() {
        let clip = Clip::new(ContentId(1), 0.0, 1.0);
        let project = ProjectBuilder::new()
            .audio_track(3, "Drums")
            .bus_track(4, "Group", ChannelRole::Submix)
            .parent(3, 4)
            .output(3, 4)
            .audio(1, "Loop", 4.0, AudioRegion::new("loop.wav"))
            .clip(3, clip.clone())
            .clip(3, Clip { start: 4.0, ..clip })
            .build_without_ids();
        assert_eq!(project.arrangement.lanes.len(), 1);
        assert_eq!(project.arrangement.lanes[0].name, "Drums");
        assert_eq!(project.arrangement.lanes[0].clips.len(), 2);
        assert_eq!(project.channels[0].output, Some(ChannelId(4)));
        assert_eq!(project.tracks[0].parent, Some(TrackId(4)));
        assert_eq!(project.channels[1].role, ChannelRole::Submix);
    }
}
