// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The follow action of a slot clip in the detail area, driven like a user would: each control
//! runs `clip.setFollowAction`, one undo step per change.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{FollowAction, FollowKind, TrackId};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::settings::{DeviceLayout, Settings};
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

/// The demo project in the window, the browser hidden, a frame every sixtieth of a second (drags
/// need frames close together).
fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// The follow action of Walk, the clip in scene 2 on Bass.
fn walk(harness: &Harness<'static, App>) -> Option<FollowAction> {
    let project = harness.state().session()?.project();
    let column = project.launcher.columns.iter().find(|c| c.track == TrackId(2))?;
    column.slots.get(1)?.clip.as_ref().map(|c| c.follow_action)
}

fn click(harness: &mut Harness<'static, App>, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

fn undo(harness: &mut Harness<'static, App>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
}

/// Drags the value field `label` up by `dy` points.
fn drag_up(harness: &mut Harness<'static, App>, label: &str, dy: f32) {
    let from = harness.get_by_label(label).rect().center();
    let to = from - egui::vec2(0.0, dy);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from.lerp(to, 0.5));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
}

/// Launching Walk selects it, so the detail area shows its follow action.
fn with_walk_selected() -> Harness<'static, App> {
    let mut harness = window();
    assert!(harness.query_by_label("Follow action off").is_none(), "nothing selected, no follow action");
    click(&mut harness, "Launch Walk on Bass (scene 2)");
    assert!(harness.query_by_label("Follow action off").is_some());
    harness
}

#[test]
fn the_switch_turns_the_follow_action_on_in_one_undo_step() {
    let mut harness = with_walk_selected();
    assert_eq!(walk(&harness), Some(FollowAction::default()));
    click(&mut harness, "Follow action off");
    assert_eq!(walk(&harness).map(|f| f.enabled), Some(true));
    assert!(harness.query_by_label("Follow action on").is_some());
    // Only `enabled` changed.
    assert_eq!(walk(&harness), Some(FollowAction { enabled: true, ..FollowAction::default() }));
    undo(&mut harness);
    assert_eq!(walk(&harness), Some(FollowAction::default()));
    assert!(harness.query_by_label("Follow action off").is_some());
}

#[test]
fn linked_and_the_time_set_when_it_happens() {
    let mut harness = with_walk_selected();
    click(&mut harness, "Linked to the clip's length");
    assert_eq!(walk(&harness).map(|f| f.linked), Some(false));
    assert!(harness.query_by_label("Not linked to the clip's length").is_some());

    // A follow time of one bar; 16 points up is 2 beats more.
    let time = harness.get_by_label("Follow time");
    assert_eq!(time.value().as_deref(), Some("1.0"));
    drag_up(&mut harness, "Follow time", 16.0);
    assert_eq!(walk(&harness).map(|f| f.time), Some(6.0));
    assert_eq!(harness.get_by_label("Follow time").value().as_deref(), Some("1.2"));
    undo(&mut harness);
    assert_eq!(walk(&harness).map(|f| (f.time, f.linked)), Some((4.0, false)));
    undo(&mut harness);
    assert_eq!(walk(&harness), Some(FollowAction::default()));
}

#[test]
fn actions_are_picked_from_their_dropdowns_with_a_jump_to_a_scene() {
    let mut harness = with_walk_selected();
    click(&mut harness, "Action A: Next");
    click(&mut harness, "Jump to scene 3");
    assert_eq!(walk(&harness).map(|f| f.action_a), Some(FollowKind::Jump(2)));
    assert!(harness.query_by_label("Action A: Jump to scene 3").is_some());

    click(&mut harness, "Action B: Nothing");
    click(&mut harness, "Other");
    assert_eq!(walk(&harness).map(|f| (f.action_a, f.action_b)), Some((FollowKind::Jump(2), FollowKind::Other)));
    undo(&mut harness);
    assert_eq!(walk(&harness).map(|f| f.action_b), Some(FollowKind::Nothing));
}

#[test]
fn chances_are_dragged_from_0_to_100() {
    let mut harness = with_walk_selected();
    // Half a point a point of drag: 40 points up is 20.
    drag_up(&mut harness, "Chance B", 40.0);
    assert_eq!(walk(&harness).map(|f| f.chance_b), Some(20));
    assert_eq!(harness.get_by_label("Chance B").value().as_deref(), Some("20"));
    // Never over 100.
    drag_up(&mut harness, "Chance A", 80.0);
    assert_eq!(walk(&harness).map(|f| f.chance_a), Some(100));
    drag_up(&mut harness, "Chance A", -60.0);
    assert_eq!(walk(&harness).map(|f| (f.chance_a, f.chance_b)), Some((70, 20)));
    undo(&mut harness);
    assert_eq!(walk(&harness).map(|f| (f.chance_a, f.chance_b)), Some((100, 20)));
}

#[test]
fn a_track_selected_instead_hides_it_and_stacked_it_sits_on_top() {
    let mut harness = with_walk_selected();
    click(&mut harness, "Track Drums");
    assert!(harness.query_by_label("Follow action off").is_none());

    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings { device_layout: DeviceLayout::Stack, ..Default::default() });
    harness.run();
    click(&mut harness, "Launch Walk on Bass (scene 2)");
    click(&mut harness, "Follow action off");
    assert_eq!(walk(&harness).map(|f| f.enabled), Some(true));
    // The Arrangement view shows its own selection, not the slot's.
    harness.key_press(Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Follow action on").is_none());
}

/// Renders the window with a slot clip's follow action shown, in a row and stacked, to
/// `target/ui-snapshots/follow-row.png` and `follow-stack.png`, for looking at:
/// `cargo test -p suhina-ui --test follow -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_follow_action() {
    for (layout, file) in [(DeviceLayout::Row, "follow-row.png"), (DeviceLayout::Stack, "follow-stack.png")] {
        let mut app = App::new();
        app.set_browser_open(false);
        app.set_session(Session::new(demo_project()), &[]);
        let mut harness = Harness::builder()
            .allow_missing_glyphs()
            .allow_missing_glyphs()
            .with_size(SIZE)
            .with_pixels_per_point(2.0)
            .with_step_dt(0.05)
            .with_max_steps(40)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        harness.state_mut().set_settings(&ctx, Settings { device_layout: layout, ..Default::default() });
        harness.run();
        click(&mut harness, "Launch Walk on Bass (scene 2)");
        click(&mut harness, "Follow action off");
        click(&mut harness, "Linked to the clip's length");
        drag_up(&mut harness, "Chance B", 40.0);
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
        std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
        image.save(out.join(file)).unwrap_or_else(|e| panic!("{e}"));
    }
}
