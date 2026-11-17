// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A large arrangement, as an imported song brings: every element keeps its style.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{
    ArrangementLane, Channel, ChannelId, Clip, ClipContent, ContentId, ContentKinds, ContentLane, Extensions, LaneData,
    Note, Project, Track, TrackId,
};
use suhina_session::Session;
use suhina_ui::App;
use suhina_ui::kit::theme;

/// 31 MIDI tracks of `per_lane` short clips each, all of them in view.
fn large(per_lane: u32) -> Project {
    let mut p = Project::default();
    for t in 0..31u32 {
        let id = TrackId(t + 1);
        p.tracks.push(Track {
            channel: Some(ChannelId(t + 1)),
            content: ContentKinds { notes: true, audio: false },
            ..Track::new(id, format!("Track {t}"))
        });
        p.channels.push(Channel::new(ChannelId(t + 1), format!("Track {t}")));
        p.contents.push(ClipContent {
            linked: true,
            id: ContentId(t + 1),
            name: format!("Pattern {t}"),
            length: 4.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes((0..8).map(|n| Note::new(f64::from(n) * 0.5, 0.25, 60 + n as u8, 0.8)).collect()),
            }]
            .into(),
            extensions: Extensions::default(),
            scale: None,
        });
        let clips = (0..per_lane).map(|c| Clip::new(ContentId(t + 1), f64::from(c) * 0.25, 0.25)).collect();
        p.arrangement.lanes.push(ArrangementLane { track: Some(id), clips, ..ArrangementLane::default() });
    }
    p
}

/// The style tree grows with the clips in view, one element each, not with their parts (a clip's
/// name, notes, waveform and fades are read once per kind of clip, ADR 0031): an arrangement of
/// thousands of clips keeps every element styled.
#[test]
fn a_large_arrangement_keeps_every_element_styled() {
    let nodes = |per_lane: u32| {
        let mut app = App::new();
        app.set_browser_open(false);
        app.set_session(Session::new(large(per_lane)), &[]);
        let mut harness = Harness::builder()
            .allow_missing_glyphs()
            .with_size((1600.0, 2400.0))
            .with_step_dt(0.05)
            .with_max_steps(20)
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        harness.run();
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.run();
        let budget = suhina_ui::kit::native_css::budget();
        assert_eq!(budget.unstyled, 0, "{per_lane} clips a lane: {} nodes", budget.nodes);
        budget.nodes
    };
    let (few, many) = (nodes(10), nodes(70));
    // 60 more clips on each of the lanes in view: at most one element for each.
    assert!(many > few, "the clips are elements: {few} and {many}");
    assert!(many - few <= 31 * 60, "a clip adds one element, not one for each part: {few} and {many}");
}
