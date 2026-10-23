// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    Arrangement, ArrangementLane, Channel, ChannelId, ChannelRole, Clip, ClipContent, ClipId, ClipPlace, ContentId,
    Extensions, IdsExhausted, LaneId, Launcher, LauncherColumn, Project, Scene, SceneId, Slot, Track, TrackId,
    ValidationError,
};

fn clip(start: f64) -> Clip {
    Clip::new(ContentId(1), start, 4.0)
}

/// Two lanes of two clips, two scenes and a column with one clip, none with an id.
fn project() -> Project {
    let lane = |track| ArrangementLane {
        track: Some(TrackId(track)),
        clips: vec![clip(0.0), clip(4.0)],
        ..ArrangementLane::default()
    };
    Project {
        tracks: vec![Track::new(TrackId(1), "1"), Track::new(TrackId(2), "2")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: String::new(),
            length: 4.0,
            lanes: Vec::new(),
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement { lanes: vec![lane(1), lane(2)], markers: Vec::new() },
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot::default(), Slot { clip: Some(clip(0.0)), has_stop: false }],
            }],
            mappings: Vec::new(),
        },
        ..Project::default()
    }
}

#[test]
fn ids_are_given_in_document_order_and_kept_on_a_second_pass() {
    let mut p = project();
    p.assign_ids().unwrap();
    let lanes: Vec<_> = p.arrangement.lanes.iter().map(|l| l.id).collect();
    let clips: Vec<_> = p.all_clips().map(|c| c.id).collect();
    let scenes: Vec<_> = p.launcher.scenes.iter().map(|s| s.id).collect();
    assert_eq!(lanes, [LaneId(1), LaneId(4)]);
    assert_eq!(clips, [ClipId(2), ClipId(3), ClipId(5), ClipId(6), ClipId(9)]);
    assert_eq!(scenes, [SceneId(7), SceneId(8)]);
    assert_eq!(p.next_id, 10);
    assert_eq!(p.validate(), Ok(()));

    let again = p.clone();
    p.assign_ids().unwrap();
    assert_eq!(p, again, "objects with ids keep them");
    assert_eq!(project_with_ids(), p, "the same project always gets the same ids");
}

fn project_with_ids() -> Project {
    let mut p = project();
    p.assign_ids().unwrap();
    p
}

#[test]
fn a_new_id_is_never_one_handed_out_before() {
    let mut p = project_with_ids();
    let last = p.arrangement.lanes[1].clips.pop().unwrap();
    let new = p.new_clip_id().unwrap();
    assert!(new > last.id, "a deleted clip's id {} is not handed out again ({new})", last.id);
    assert!(p.new_scene_id().unwrap().0 > new.0, "lanes, clips and scenes share the counter");

    // An object added with an id the counter has not reached moves the counter on.
    p.arrangement.lanes[0].clips.push(Clip { id: ClipId(50), ..clip(8.0) });
    assert_eq!(p.validate(), Err(vec![ValidationError::IdAhead(50)]));
    p.assign_ids().unwrap();
    assert_eq!(p.next_id, 51);
    assert_eq!(p.new_lane_id(), Ok(LaneId(51)));
}

#[test]
fn the_counter_never_hands_out_the_largest_value() {
    let mut p = Project { next_id: u32::MAX - 1, ..Project::default() };
    assert_eq!(p.new_clip_id(), Ok(ClipId(u32::MAX - 1)));
    assert_eq!(p.new_clip_id(), Err(IdsExhausted));
    assert_eq!(p.next_id, u32::MAX);

    // Too few ids left for the objects that need one: as many as fit get one, then it fails.
    let mut p = project();
    p.next_id = u32::MAX - 2;
    assert_eq!(p.assign_ids(), Err(IdsExhausted));
    assert_eq!(p.arrangement.lanes[0].id, LaneId(u32::MAX - 2));
    assert_eq!(p.arrangement.lanes[0].clips[0].id, ClipId(u32::MAX - 1));
    assert_eq!(p.arrangement.lanes[0].clips[1].id, ClipId::NONE);

    // An object that already has the largest value leaves nothing to hand out.
    let mut p = project();
    p.launcher.scenes[0].id = SceneId(u32::MAX);
    assert_eq!(p.assign_ids(), Err(IdsExhausted));
}

#[test]
fn duplicate_ids_do_not_validate() {
    let mut p = project_with_ids();
    p.arrangement.lanes[1].clips[0].id = ClipId(2);
    p.launcher.scenes[1].id = SceneId(7);
    p.arrangement.lanes[1].id = LaneId(1);
    assert_eq!(
        p.validate(),
        Err(vec![
            ValidationError::DuplicateClip(ClipId(2)),
            ValidationError::DuplicateLane(LaneId(1)),
            ValidationError::DuplicateScene(SceneId(7)),
        ])
    );
    // A lane and a clip with the same value: the kinds share the counter, so that is refused
    // too, once, beside nothing else.
    let mut p = project_with_ids();
    p.arrangement.lanes[0].clips[0].id = ClipId(1);
    assert_eq!(p.validate(), Err(vec![ValidationError::SharedId(1)]));
    // A clip in a slot and a scene, the same.
    let mut p = project_with_ids();
    p.launcher.scenes[0].id = SceneId(9);
    assert_eq!(p.validate(), Err(vec![ValidationError::SharedId(9)]));
}

#[test]
fn a_clip_is_found_by_its_id_wherever_it_moves() {
    let mut p = project_with_ids();
    assert_eq!(p.clip_place(ClipId(9)), Some(ClipPlace::Slot { column: 0, slot: 1 }));
    assert_eq!(p.clip_place(ClipId(3)), Some(ClipPlace::Lane { lane: 0, index: 1 }));
    // The first clip of the lane goes; the second is now at index 0 and still clip 3.
    p.arrangement.lanes[0].clips.remove(0);
    assert_eq!(p.clip_place(ClipId(3)), Some(ClipPlace::Lane { lane: 0, index: 0 }));
    p.clip_mut(ClipId(3)).unwrap().start = 12.0;
    assert_eq!(p.clip(ClipId(3)).map(|c| c.start), Some(12.0));
    assert_eq!(p.clip_place(ClipId(2)), None);
    assert_eq!(p.clip_place(ClipId::NONE), None, "no id names no clip");
}

/// A track and a channel are found by their ids, to read or to change; an id nothing has finds
/// nothing.
#[test]
fn finds_tracks_and_channels_by_id() {
    let mut project = project();
    project.channels = vec![
        Channel::new(ChannelId(7), "1"),
        Channel { role: ChannelRole::Master, ..Channel::new(ChannelId(9), "Main") },
    ];
    assert_eq!(project.track(TrackId(2)).map(|t| t.name.as_str()), Some("2"));
    assert!(project.track(TrackId(3)).is_none());
    if let Some(track) = project.track_mut(TrackId(1)) {
        track.name = "renamed".into();
    }
    assert_eq!(project.tracks[0].name, "renamed");
    assert_eq!(project.channel(ChannelId(9)).map(|c| c.role), Some(ChannelRole::Master));
    assert!(project.channel(ChannelId(1)).is_none() && project.channel_mut(ChannelId(8)).is_none());
    project.channel_mut(ChannelId(7)).unwrap().pan = 0.5;
    assert_eq!(project.channels[0].pan, 0.5);
}

#[test]
fn an_output_chain_follows_outputs_and_ends_at_a_cycle_or_a_missing_channel() {
    let channel =
        |id: u32, output: Option<u32>| Channel { output: output.map(ChannelId), ..Channel::new(ChannelId(id), "") };
    let project = Project {
        channels: vec![
            channel(1, Some(2)),
            channel(2, Some(3)),
            channel(3, None),
            channel(4, Some(5)),
            channel(5, Some(4)),
            channel(6, Some(9)),
        ],
        ..Project::default()
    };
    let chain = |from: u32| project.output_chain(ChannelId(from)).map(|c| c.0).collect::<Vec<_>>();
    assert_eq!(chain(1), [2, 3]);
    assert_eq!(chain(3), Vec::<u32>::new());
    // A cycle ends where it comes back, without giving its start again.
    assert_eq!(chain(4), [5]);
    // An output that names no channel is given, and the walk ends there.
    assert_eq!(chain(6), [9]);
    assert_eq!(chain(42), Vec::<u32>::new());
}
