// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn note(time: f64, key: u8) -> Note {
    Note::new(time, 0.25, key, 0.8)
}

fn track(id: u32, channel: u32) -> Track {
    Track {
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(channel)),
        ..Track::new(TrackId(id), format!("Track {id}"))
    }
}

fn channel(id: u32) -> Channel {
    Channel::new(ChannelId(id), format!("Channel {id}"))
}

fn clip(content: u32, start: f64) -> Clip {
    Clip::new(ContentId(content), start, 4.0)
}

#[test]
fn follow_kinds_read_back_their_names() {
    for kind in FollowKind::NAMED.into_iter().chain([FollowKind::Jump(0), FollowKind::Jump(41)]) {
        assert_eq!(FollowKind::from_name(&kind.name()), Some(kind));
    }
    assert_eq!(FollowKind::Jump(2).name(), "jump 3");
    assert_eq!(FollowKind::from_name(" Jump  12 "), Some(FollowKind::Jump(11)));
    for bad in ["", "jump", "jump 0", "jump -1", "jump 1.5", "jump 99999999999", "nextt", "stop 2"] {
        assert_eq!(FollowKind::from_name(bad), None, "{bad}");
    }
}

#[test]
fn a_follow_action_needs_a_time_and_chances_in_range() {
    assert!(FollowAction::default().is_valid());
    for time in [0.0, -1.0, f64::NAN, f64::INFINITY, FollowAction::MAX_TIME * 2.0] {
        assert!(!FollowAction { time, ..FollowAction::default() }.is_valid(), "{time}");
    }
    assert!(!FollowAction { chance_a: 101, ..FollowAction::default() }.is_valid());
    assert!(!FollowAction { chance_b: 101, ..FollowAction::default() }.is_valid());
}

#[test]
fn default_transport_is_120_bpm_in_four_four() {
    let t = Project::default().transport;
    assert_eq!(t.tempo_bpm, 120.0);
    assert_eq!(t.time_signature, TimeSignature { numerator: 4, denominator: 4 });
}

/// Per-track clips: each clip has its own content with one untargeted lane, arrangement
/// lanes are bound to tracks, and the same track has a launcher column.
#[test]
fn per_track_clip_project_validates() {
    let project = Project {
        tracks: vec![track(1, 1)],
        channels: vec![channel(1)],
        contents: vec![
            ClipContent {
                id: ContentId(1),
                name: "Bass A".into(),
                length: 4.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(0.0, 36)]) }],
                extensions: Extensions::default(),
                scale: None,
            },
            ClipContent {
                id: ContentId(2),
                name: "Bass B".into(),
                length: 4.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(1.0, 38)]) }],
                extensions: Extensions::default(),
                scale: None,
            },
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1, 0.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![
                    Slot { clip: Some(clip(1, 0.0)), has_stop: true },
                    Slot { clip: Some(clip(2, 0.0)), has_stop: true },
                ],
            }],
        },
        ..Project::default()
    };
    assert_eq!(project.validate(), Ok(()));
}

/// Shared pattern: one content drives two tracks and is placed twice on a free arrangement
/// lane; both placements share the same content.
#[test]
fn shared_pattern_project_validates_and_shares_content() {
    let pattern = ClipContent {
        id: ContentId(1),
        name: "Pattern 1".into(),
        length: 4.0,
        lanes: vec![
            ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 36), note(2.0, 36)]) },
            ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(1.0, 38), note(3.0, 38)]) },
        ],
        extensions: Extensions::default(),
        scale: None,
    };
    let project = Project {
        tracks: vec![track(1, 1), track(2, 2)],
        channels: vec![channel(1), channel(2)],
        contents: vec![pattern],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Track 1".into(),
                track: None,
                clips: vec![clip(1, 0.0), clip(1, 4.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    };
    assert_eq!(project.validate(), Ok(()));

    let placements = &project.arrangement.lanes[0].clips;
    assert_eq!(placements[0].content, placements[1].content);
    assert_eq!(project.content(ContentId(1)).map(|c| c.lanes.len()), Some(2));
}

#[test]
fn dangling_references_are_reported() {
    let mut ch = channel(1);
    ch.output = Some(ChannelId(9));
    ch.sends.push(Send { target: ChannelId(8), level: 0.5, pre_fader: false });
    let project = Project {
        tracks: vec![track(1, 7)],
        channels: vec![ch],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: String::new(),
                track: Some(TrackId(5)),
                clips: vec![clip(3, 0.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    };
    let errors = project.validate().unwrap_err();
    assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(7))));
    assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(9))));
    assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(8))));
    assert!(errors.contains(&ValidationError::MissingTrack(TrackId(5))));
    assert!(errors.contains(&ValidationError::MissingContent(ContentId(3))));
}

/// Each shape's gain at the ends and half-way, as the spec's table gives it, and how the
/// curve bends it (`docs/spec/arrangement-editing.md` › Fades).
#[test]
fn fade_shapes_and_curves_follow_the_spec() {
    let half = [
        (FadeShape::Linear, 0.5),
        (FadeShape::EqualPower, std::f64::consts::FRAC_1_SQRT_2),
        (FadeShape::Fast, 0.75),
        (FadeShape::Slow, 0.25),
        (FadeShape::SCurve, 0.5),
    ];
    for (shape, at_half) in half {
        for curve in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let fade = FadeCurve { shape, curve };
            let expected = at_half.powf(4f64.powf(-curve));
            assert!((fade.gain(0.5) - expected).abs() < 1e-12, "{shape:?} {curve}: {}", fade.gain(0.5));
            assert_eq!((fade.gain(0.0), fade.gain(1.0)), (0.0, 1.0), "{shape:?} {curve}");
            // It rises the whole way.
            let gains: Vec<f64> = (0..=100).map(|i| fade.gain(f64::from(i) / 100.0)).collect();
            assert!(gains.windows(2).all(|w| w[1] >= w[0]), "{shape:?} {curve}");
        }
        assert_eq!(FadeShape::from_name(shape.name()), Some(shape));
    }
    // +1 starts faster than the shape, −1 slower.
    let linear = |curve| FadeCurve { shape: FadeShape::Linear, curve };
    assert!((linear(1.0).gain(0.5) - 0.5f64.powf(0.25)).abs() < 1e-12);
    assert!((linear(-1.0).gain(0.5) - 0.0625).abs() < 1e-12);
    // Hostile input stays within 0 … 1.
    assert_eq!(linear(0.0).gain(f64::NAN), 0.0);
    assert_eq!(linear(0.0).gain(7.0), 1.0);
    assert_eq!(linear(f64::NAN).gain(0.5), 0.5);
    assert_eq!(FadeShape::from_name("wobbly"), None);
    // The curve that puts the half-way gain at a height gives back that height.
    for shape in FadeShape::ALL {
        for curve in [-0.9, -0.3, 0.0, 0.4, 0.95] {
            let gain = FadeCurve { shape, curve }.gain(0.5);
            assert!((FadeCurve::through(shape, gain) - curve).abs() < 1e-9, "{shape:?} {curve}");
        }
        assert_eq!((FadeCurve::through(shape, 0.0), FadeCurve::through(shape, 1.0)), (-1.0, 1.0));
        assert_eq!(FadeCurve::through(shape, f64::NAN), 0.0);
    }
}

#[test]
fn fades_must_be_non_negative_and_fit_the_clip() {
    let with = |fade_in: f64, fade_out: f64| Clip { fade_in, fade_out, ..clip(1, 0.0) };
    assert!(with(0.0, 0.0).fades_valid());
    assert!(with(1.5, 2.5).fades_valid(), "together exactly the clip's length");
    assert!(!with(-0.1, 0.0).fades_valid());
    assert!(!with(0.0, f64::NAN).fades_valid());
    assert!(!with(f64::INFINITY, 0.0).fades_valid());
    assert!(!with(3.0, 1.5).fades_valid(), "4.5 beats of fades on a 4-beat clip");
    let bent = |curve: f64| Clip { fade_out_curve: FadeCurve { shape: FadeShape::Slow, curve }, ..with(1.0, 1.0) };
    assert!(bent(1.0).fades_valid() && bent(-1.0).fades_valid());
    assert!(!bent(1.5).fades_valid() && !bent(f64::NAN).fades_valid(), "a curve outside -1 … +1");

    let content = ClipContent {
        id: ContentId(1),
        name: String::new(),
        length: 4.0,
        lanes: Vec::new(),
        extensions: Extensions::default(),
        scale: None,
    };
    let project = Project {
        contents: vec![content],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: String::new(),
                track: None,
                clips: vec![with(1.0, 1.0), with(3.0, 3.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    };
    let mut project = project;
    project.assign_ids().unwrap();
    // The lane is object 1, its clips 2 and 3.
    assert_eq!(project.validate(), Err(vec![ValidationError::BadFades { lane: LaneId(1), clip: ClipId(3) }]));
}

#[test]
fn fitting_fades_keeps_them_inside_the_clip() {
    let mut c = Clip { fade_in: 3.0, fade_out: 1.0, length: 2.0, ..clip(1, 0.0) };
    c.fit_fades();
    assert_eq!((c.fade_in, c.fade_out), (1.5, 0.5), "both shrink in proportion");
    let mut c = Clip { fade_in: -1.0, fade_out: f64::NAN, ..clip(1, 0.0) };
    c.fit_fades();
    assert_eq!((c.fade_in, c.fade_out), (0.0, 0.0));
    let mut c = Clip { fade_in: 1.0, fade_out: 0.5, ..clip(1, 0.0) };
    c.fit_fades();
    assert_eq!((c.fade_in, c.fade_out), (1.0, 0.5), "fades that fit are kept");
}

#[test]
fn the_loop_region_repeats_only_while_on_and_must_end_after_it_starts() {
    let mut transport = Transport { loop_range: Some(LoopRange { start: 4.0, end: 8.0 }), ..Transport::default() };
    assert_eq!(transport.looping(), None, "off: kept, not repeated");
    transport.loop_enabled = true;
    assert_eq!(transport.looping(), Some(LoopRange { start: 4.0, end: 8.0 }));

    for bad in [(4.0, 4.0), (8.0, 4.0), (-1.0, 4.0), (0.0, f64::INFINITY), (f64::NAN, 1.0)] {
        let project = Project {
            transport: Transport { loop_range: Some(LoopRange { start: bad.0, end: bad.1 }), ..Transport::default() },
            ..Project::default()
        };
        assert_eq!(project.validate(), Err(vec![ValidationError::BadLoop]), "{bad:?}");
        assert_eq!(Transport { loop_enabled: true, ..project.transport }.looping(), None);
    }
}

#[test]
fn markers_stay_in_beat_order_and_must_be_at_a_beat_from_0() {
    let mut arrangement = Arrangement::default();
    assert_eq!(arrangement.insert_marker(Marker::new(8.0, "Chorus")), 0);
    assert_eq!(arrangement.insert_marker(Marker::new(0.0, "Intro")), 0);
    assert_eq!(arrangement.insert_marker(Marker::new(8.0, "Drop")), 2, "after the one already on beat 8");
    let names: Vec<&str> = arrangement.markers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Intro", "Chorus", "Drop"]);
    arrangement.markers[0].beat = 12.0;
    arrangement.sort_markers();
    let names: Vec<&str> = arrangement.markers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Chorus", "Drop", "Intro"]);

    let project = Project {
        arrangement: Arrangement {
            lanes: Vec::new(),
            markers: vec![Marker::new(0.0, "ok"), Marker::new(-1.0, "before"), Marker::new(f64::NAN, "nan")],
            takes: Vec::new(),
        },
        ..Project::default()
    };
    assert_eq!(project.validate(), Err(vec![ValidationError::BadMarker(1), ValidationError::BadMarker(2)]));
}

#[test]
fn duplicate_ids_are_reported() {
    let project = Project { tracks: vec![track(1, 1), track(1, 1)], channels: vec![channel(1)], ..Project::default() };
    assert_eq!(project.validate(), Err(vec![ValidationError::DuplicateTrack(TrackId(1))]));
}
/// Tracks 1..=n, each inside the one before it.
fn chain_of_groups(n: u32) -> Project {
    let tracks = (1..=n).map(|id| Track { parent: (id > 1).then(|| TrackId(id - 1)), ..track(id, id) }).collect();
    Project { tracks, channels: (1..=n).map(channel).collect(), ..Project::default() }
}

#[test]
fn nested_groups_report_their_ancestors_depth_and_descendants() {
    let project = chain_of_groups(4);
    assert_eq!(project.ancestors(TrackId(4)), vec![TrackId(3), TrackId(2), TrackId(1)]);
    assert_eq!(project.group_depth(TrackId(4)), Some(3));
    assert_eq!(project.group_depth(TrackId(1)), Some(0));
    assert_eq!(project.with_descendants(TrackId(2)), vec![TrackId(2), TrackId(3), TrackId(4)]);
    assert_eq!(project.nesting_below(TrackId(2)), 2);
    assert_eq!(project.nesting_below(TrackId(4)), 0);
    assert_eq!(project.validate(), Ok(()));
    // A track that does not exist is at the top level and holds nothing.
    assert_eq!(project.ancestors(TrackId(9)), Vec::<TrackId>::new());
    assert_eq!(project.with_descendants(TrackId(9)), vec![]);
}

#[test]
fn groups_may_nest_up_to_the_cap_and_no_deeper() {
    let deepest = u32::try_from(MAX_GROUP_DEPTH).unwrap() + 1;
    assert_eq!(chain_of_groups(deepest).validate(), Ok(()));
    let too_deep = chain_of_groups(deepest + 1);
    assert_eq!(too_deep.validate(), Err(vec![ValidationError::GroupTooDeep(TrackId(deepest + 1))]));
}

#[test]
fn a_group_cycle_is_reported_and_walking_it_ends() {
    let mut project = chain_of_groups(3);
    project.tracks[0].parent = Some(TrackId(3));
    assert_eq!(project.group_depth(TrackId(2)), None);
    assert_eq!(project.ancestors(TrackId(2)), vec![TrackId(1), TrackId(3)]);
    assert_eq!(project.with_descendants(TrackId(1)).len(), 3);
    let errors = project.validate().unwrap_err();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors.iter().all(|e| matches!(e, ValidationError::GroupCycle(_))));
    // A track that is its own parent too.
    let mut own = chain_of_groups(1);
    own.tracks[0].parent = Some(TrackId(1));
    assert_eq!(own.validate(), Err(vec![ValidationError::GroupCycle(TrackId(1))]));
    assert_eq!(own.ancestors(TrackId(1)), vec![]);
}
