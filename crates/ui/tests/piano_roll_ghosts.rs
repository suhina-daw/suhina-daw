// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's ghost notes (`docs/spec/ui-clip-editor.md` › Ghost notes): the notes of other
//! clips drawn behind the edited clip's, the toolbar's ghost menu, and clicks that go through
//! ghost notes to the grid.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Modifiers, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_engine::model::{LaneData, Note, TrackId};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::detail_view::ClipRef;
use suhina_ui::detail::piano_roll_ghosts::GhostSource;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64). Walk on Bass and Chords 1 on
/// Keys start with it.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;
/// Walk's first note, E1 (MIDI 40), under the edited clip's first beat.
const BASS_GHOST: &str = "Ghost note E1 at beat 0 from Bass";
/// Beat B at bar 17: the second clip on Drums' lane.
fn edited() -> ClipRef {
    let project = demo_project();
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap();
    ClipRef::Arrangement(TrackId(1), lane.clips[1].id)
}

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap()
}

fn piano_app(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings::default());
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness
}

/// The edited clip's notes in the project.
fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.clone(),
        LaneData::Audio(_) => Vec::new(),
    }
}

/// How many ghost notes from track `from` the shown clip has (in view or not), with the source
/// the toolbar's menu picked.
fn ghosts(h: &Harness<'static, App>, from: &str) -> usize {
    let project = h.state().session().unwrap().project();
    let source = h.state().piano_roll().ghost_source;
    let ghosts = suhina_ui::detail::piano_roll_ghosts::ghosts(project, edited(), source);
    ghosts.notes.iter().filter(|g| ghosts.sources.get(g.source).is_some_and(|s| s == from)).count()
}

/// How many ghost notes are drawn in view with a label naming track `from`.
fn drawn(h: &Harness<'static, App>, from: &str) -> usize {
    h.query_all_by_label_contains(&format!(" from {from}"))
        .filter(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with("Ghost note ")))
        .count()
}

fn pointer(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn clicks(h: &mut Harness<'static, App>, pos: Pos2, count: usize) {
    h.hover_at(pos);
    h.step();
    for _ in 0..count {
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    h.run();
}

/// Waits long enough that the next click is not part of a double-click.
fn pause(h: &mut Harness<'static, App>) {
    for _ in 0..10 {
        h.step();
    }
}

/// Picks `entry` in the toolbar's ghost menu, whose button reads `now`.
fn pick(h: &mut Harness<'static, App>, now: &str, entry: &str) {
    h.get_by_label(&format!("Ghost notes {now}")).click();
    h.run();
    h.get_by_label(entry).click();
    h.run();
}

#[test]
fn the_clips_playing_together_show_by_default_in_their_track_colours() {
    let mut renderer = engine();
    let h = piano_app(&mut renderer);
    assert_eq!(h.state().piano_roll().ghost_source, GhostSource::Together);
    assert!(h.query_by_label("Ghost notes Playing together").is_some(), "the menu shows what is shown");
    assert!(h.query_by_label(BASS_GHOST).is_some(), "Walk's notes are drawn behind Beat B's");
    assert_eq!(ghosts(&h, "Bass"), 4, "a one-bar figure in the clip's first pass");
    assert_eq!(ghosts(&h, "Keys"), 4);
    assert!(drawn(&h, "Bass") > 0);
    assert_eq!(ghosts(&h, "Drums"), 0, "the clip's own track has nothing playing with it");
    // The ghosts sit beside the real notes on the same beats, never as notes of the clip.
    assert_eq!(notes(&h).len(), 4);
    assert_eq!(h.query_all_by_label_contains("Note ").count(), 4);
}

#[test]
fn none_hides_them_this_track_and_one_clip_choose_others() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    pick(&mut h, "Playing together", "Ghost notes: None");
    assert_eq!(h.state().piano_roll().ghost_source, GhostSource::None);
    assert!(h.query_all_by_label_contains("Ghost note ").next().is_none(), "no ghost notes");
    assert!(h.query_by_label("Ghost notes None").is_some());

    // This track: Drums' other clips, each from the edited clip's start, on its own keys.
    pick(&mut h, "None", "Ghost notes: This track");
    assert_eq!(ghosts(&h, "Drums"), 12, "three other clips on Drums");
    assert!(drawn(&h, "Drums") > 0);
    assert_eq!((ghosts(&h, "Bass"), drawn(&h, "Bass")), (0, 0));

    // One clip on another track, from the Clip submenu.
    h.get_by_label("Ghost notes This track").click();
    h.run();
    h.get_by_label("Ghost notes: Clip\u{2026}").click();
    h.run();
    h.get_by_label("Ghost notes from Walk on Bass at bar 17").click();
    h.run();
    assert!(h.query_by_label("Ghost notes Walk on Bass at bar 17").is_some());
    assert_eq!(ghosts(&h, "Bass"), 4);
    assert!(h.query_by_label(BASS_GHOST).is_some());
    assert_eq!((ghosts(&h, "Drums"), drawn(&h, "Drums")), (0, 0));
    assert_eq!(ghosts(&h, "Keys"), 0);
    // Nothing of this is a command: the project and the undo history are as they were.
    assert!(!h.state().session().unwrap().can_undo());

    // The choice is kept when another clip is shown.
    h.get_by_label("Clip Chords 1 on Keys at bar 17").click();
    h.run();
    assert!(h.query_by_label("Ghost notes Walk on Bass at bar 17").is_some());
    assert!(matches!(h.state().piano_roll().ghost_source, GhostSource::Clip(_)));
}

#[test]
fn a_click_on_a_ghost_note_acts_on_the_empty_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let ghost: Rect = h.get_by_label(BASS_GHOST).rect();
    let at = ghost.center();
    // A note selected first, so a single click on the ghost shows it clears the selection.
    let real = h.query_all_by_label_contains("Note ").next().unwrap().rect().center();
    clicks(&mut h, real, 1);
    assert_eq!(h.state().piano_roll().selected.len(), 1);
    pause(&mut h);
    clicks(&mut h, at, 1);
    assert!(h.state().piano_roll().selected.is_empty(), "the click went to the empty grid");
    assert_eq!(notes(&h), before, "and changed nothing");
    pause(&mut h);
    // A double-click there adds a note of the clip's own, on the ghost's key.
    clicks(&mut h, at, 2);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1);
    assert!(after.iter().any(|n| n.key == 40 && !before.contains(n)), "a new E1: {after:?}");
    assert_eq!(h.state().piano_roll().selected.len(), 1, "the new note is selected, never a ghost");
    assert!(h.query_by_label(BASS_GHOST).is_some(), "the ghost is still drawn behind it");
}

/// Draws the piano roll with ghost notes and the open menu to `target/ui-snapshots/`, for
/// looking at: `cargo test -p suhina-ui --test piano_roll_ghosts -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_ghost_notes_and_menu() {
    let mut renderer = engine();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&h.ctx);
    h.run();
    h.get_by_label("Arrangement view").click();
    h.run();
    h.get_by_label(CLIP).click();
    h.run();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    h.render().unwrap().save(out.join("ghost-notes.png")).unwrap();
    h.get_by_label("Ghost notes Playing together").click();
    h.run();
    h.get_by_label("Ghost notes: Clip\u{2026}").hover();
    h.run();
    h.render().unwrap().save(out.join("ghost-menu.png")).unwrap();
}
