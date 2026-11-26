// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mixer's crossfader (`docs/spec/crossfader.md`): the equal-power A/B blend rendered
//! offline, and a group's assign governing its children.

use suhina_model::*;
use suhina_test_support::{ProjectBuilder, peak};

use crate::render_offline;

const SR: u32 = 8_000;

/// Two in-phase notes, one A and one B, into Main; the group case routes them through a group.
fn project(value: f64, a: CrossfadeAssign, b: CrossfadeAssign) -> Project {
    ProjectBuilder::new()
        .midi_track(1, "A")
        .midi_track(2, "B")
        .notes(1, "n", 1.0, vec![Note::new(0.0, 1.0, 69, 1.0)])
        .clip(1, Clip::new(ContentId(1), 0.0, 1.0))
        .clip(2, Clip::new(ContentId(1), 0.0, 1.0))
        .channel(Channel { role: ChannelRole::Master, ..Channel::new(ChannelId(100), "Main") })
        .with(|p| {
            p.crossfader = Crossfader(value);
            p.tracks[0].crossfade_assign = a;
            p.tracks[1].crossfade_assign = b;
        })
        .build()
}

fn level(value: f64, a: CrossfadeAssign, b: CrossfadeAssign) -> f32 {
    peak(&render_offline(&project(value, a, b), SR, 256, suhina_device_registry::factory()).unwrap())
}

/// Two in-phase notes: at A only the A track, at B only B, in the middle each at √½, and an off
/// track is heard at both ends.
#[test]
fn the_crossfader_blends_the_two_sides_with_equal_power() {
    let a_only = level(0.0, CrossfadeAssign::A, CrossfadeAssign::B);
    let b_only = level(1.0, CrossfadeAssign::A, CrossfadeAssign::B);
    let center = level(0.5, CrossfadeAssign::A, CrossfadeAssign::B);
    assert!(a_only > 0.001 && (a_only - b_only).abs() < 1e-4, "{a_only} {b_only}");
    assert!((center / a_only - std::f32::consts::SQRT_2).abs() < 0.01, "both sides at √½: {center} from {a_only}");
    // An off track is heard at both ends; the A track fades with the value while it stays.
    let off_at_a = level(0.0, CrossfadeAssign::A, CrossfadeAssign::Off);
    assert!((off_at_a - 2.0 * a_only).abs() < 0.01, "both at A: {off_at_a} from {a_only}");
    let off_at_b = level(1.0, CrossfadeAssign::Off, CrossfadeAssign::B);
    assert!((off_at_b - 2.0 * b_only).abs() < 0.01, "both at B: {off_at_b} from {b_only}");
}

/// A group's assign governs its children: at B an A group is silent even though a child is B;
/// with the group off, the children's own assigns separate them again.
#[test]
fn the_outermost_assigned_group_governs_its_children() {
    let group = |group_assign: CrossfadeAssign| {
        ProjectBuilder::new()
            .midi_track(1, "A")
            .midi_track(2, "B")
            .bus_track(3, "G", ChannelRole::Submix)
            .notes(1, "n", 1.0, vec![Note::new(0.0, 1.0, 69, 1.0)])
            .clip(1, Clip::new(ContentId(1), 0.0, 1.0))
            .clip(2, Clip::new(ContentId(1), 0.0, 1.0))
            .channel(Channel { role: ChannelRole::Master, ..Channel::new(ChannelId(100), "Main") })
            .parent(1, 3)
            .parent(2, 3)
            .output(1, 3)
            .output(2, 3)
            .with(|p| {
                p.tracks[0].crossfade_assign = CrossfadeAssign::Off;
                p.tracks[1].crossfade_assign = CrossfadeAssign::B;
                p.tracks[2].crossfade_assign = group_assign;
                p.crossfader = Crossfader(1.0);
            })
            .build()
    };
    let render = |p: &Project| peak(&render_offline(p, SR, 256, suhina_device_registry::factory()).unwrap());
    let a_group = group(CrossfadeAssign::A);
    assert!(render(&a_group) < 1e-4, "the A group is silent at B: {}", render(&a_group));
    let off_group = group(CrossfadeAssign::Off);
    assert!(render(&off_group) > 1e-3, "the B child is heard at B with the group off");
}

/// The crossfader moves while the engine plays, as a message: no new graph, and the A track
/// fades out as the value reaches B.
#[test]
fn a_crossfader_move_is_heard_while_playing() {
    use crate::chain_tests::{SR as PLAY_SR, blocks};
    let mut p = project(0.0, CrossfadeAssign::A, CrossfadeAssign::B);
    // Only A plays, so the fader's move is visible rather than replaced by B's matching note.
    p.arrangement.lanes.retain(|lane| lane.track != Some(TrackId(2)));
    let mut renderer = crate::Renderer::build(
        &p,
        crate::RenderInputs::new(suhina_device_registry::factory()),
        PLAY_SR,
        crate::RenderOptions::PLAYING,
    )
    .unwrap();
    let mut controller = renderer.controller();
    let before = peak(&blocks(&mut renderer, 2, 256));
    controller.set_device_param(TrackId::MAIN, crate::MIXER_CROSSFADE, 0, 1.0).unwrap();
    // Let the smoothed move arrive; the first block after the message still holds the old side.
    let _ = blocks(&mut renderer, 8, 256);
    let after = peak(&blocks(&mut renderer, 8, 256));
    assert!(before > 0.001, "the A track plays first");
    assert!(after < before * 0.2, "the A track fades out at B: {before} to {after}");
}
