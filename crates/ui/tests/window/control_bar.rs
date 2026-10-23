// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control bar and the view switch: the grid value, CPU and RAM, and what a narrow window leaves out.

use super::*;

#[test]
fn launching_without_an_audio_engine_says_why() {
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Beat A on Drums (scene 1)").click();
    harness.run();
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("no audio engine"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn tab_and_the_view_switch_change_the_view() {
    let mut harness = window(demo_app());
    assert_eq!(harness.state().view(), View::Session);
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
    assert!(harness.query_by_label("Clip Beat B on Drums at bar 17").is_some());
    harness.get_by_label("Session view").click();
    harness.run();
    assert_eq!(harness.state().view(), View::Session);
}

#[test]
fn tab_only_switches_the_view() {
    let mut harness = window(demo_app());
    harness.run();
    for expected in [View::Arrangement, View::Session, View::Arrangement] {
        harness.key_press(Key::Tab);
        harness.run();
        assert_eq!(harness.state().view(), expected);
        // No widget took the keyboard, so the next Tab is a shortcut again.
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
    for key in [Key::ArrowDown, Key::ArrowRight] {
        harness.key_press(key);
        harness.run();
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
}

#[test]
fn the_control_bar_shows_the_cpu_load_only_while_an_engine_plays() {
    // With an engine attached as its player, the control bar shows the load.
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load 0.0%").is_some());

    // Without one, there is nothing to measure and nothing shown.
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load").is_none());
}

#[test]
fn the_control_bar_shows_the_ram_usage_whether_or_not_an_engine_plays() {
    // The program's own RAM is shown without an engine too: it is the process's, not the
    // engine's.
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.run();
    let ram = harness.query_by_label_contains("RAM usage");
    assert!(ram.is_some(), "the RAM shows next to the CPU's place");
    // In megabytes or gigabytes, as the issue asks.
    let text = format!("{:?}", ram.unwrap());
    assert!(text.contains("MB") || text.contains("GB"), "{text}");
}

#[test]
fn the_view_switch_is_the_last_control_at_the_right_with_the_cpu_and_ram_left_of_it() {
    // With an engine attached, all four show: RAM, CPU, then the view switch at the edge.
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    let ram = harness.get_by_label_contains("RAM usage").rect();
    let cpu = harness.get_by_label_contains("CPU load").rect();
    let arrangement = harness.get_by_label("Arrangement view").rect();
    let session = harness.get_by_label("Session view").rect();
    let switch_left = arrangement.left().min(session.left());
    let switch_right = arrangement.right().max(session.right());
    assert!(ram.right() <= cpu.left(), "RAM {ram:?} left of CPU {cpu:?}");
    assert!(cpu.right() <= switch_left, "CPU {cpu:?} left of the view switch at {switch_left}");
    // The switch keeps its place at the right edge.
    assert!(SIZE.0 - switch_right <= 16.0, "the switch ends at {switch_right}");
}

#[test]
fn a_narrow_window_leaves_out_what_would_reach_the_position_readout() {
    // At the smallest window the app allows, the blocks that show keep their order and stay
    // right of the transport in the centre (play, stop and the 64 px position).
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let size = (640.0, 400.0);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let transport_end = size.0 / 2.0 + (28.0 * 2.0 + 1.0 + theme::PAD + 64.0) / 2.0;
    let arrangement = harness.get_by_label("Arrangement view").rect();
    let session = harness.get_by_label("Session view").rect();
    let switch_left = arrangement.left().min(session.left());
    assert!(switch_left > transport_end, "the switch at {switch_left} clears the position");
    let cpu = harness.query_by_label_contains("CPU load").map(|n| n.rect());
    let ram = harness.query_by_label_contains("RAM usage").map(|n| n.rect());
    // The CPU still fits beside the switch; the RAM, further left, may not.
    assert!(cpu.is_some(), "the CPU load fits at 640 px");
    if let Some(cpu) = cpu {
        assert!(cpu.left() >= transport_end && cpu.right() <= switch_left, "CPU at {cpu:?}");
    }
    if let Some(ram) = ram {
        assert!(ram.left() >= transport_end, "RAM at {ram:?}");
        assert!(cpu.is_some_and(|cpu| ram.right() <= cpu.left()), "RAM {ram:?} left of the CPU");
    }
}

#[test]
fn the_grid_value_in_the_control_bar_sets_where_a_dragged_clip_lands() {
    let mut harness = arrangement_app();
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Bars(1)));
    // No audio engine is needed to choose the grid.
    choose_grid(&mut harness, "1/16");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
    assert!(matches!(harness.state().status(), Status::Done(s) if s == "Grid 1/16"));
    zoom_in_at_beat_b(&mut harness);

    // 1.3 beats right (83.2 points at 64 a beat): on the nearest sixteenth, beat 65.25.
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    assert_eq!(beat_b_start(&harness), Some(65.25));
    // The move is one undo step.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(beat_b_start(&harness), Some(64.0));

    // With Alt held it lands where it is let go, off the grid.
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    let free = beat_b_start(&harness).unwrap_or_default();
    assert!((free - 65.3).abs() < 0.02 && (free * 4.0).fract().abs() > 0.01, "{free}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();

    // 1 bar: 3.5 beats right lands on the next bar, beat 68, at the same zoom.
    choose_grid(&mut harness, "1 bar");
    drag_beat_b(&mut harness, egui::vec2(224.0, 0.0));
    assert_eq!(beat_b_start(&harness), Some(68.0));
}

#[test]
fn a_triplet_or_none_grid_places_a_dragged_clip_by_its_own_rule() {
    let mut harness = arrangement_app();
    choose_grid(&mut harness, "1/16T");
    zoom_in_at_beat_b(&mut harness);
    // 1.3 beats right: the nearest sixth of a beat is 64 + 8/6.
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    let start = beat_b_start(&harness).unwrap_or_default();
    assert!((start - (64.0 + 8.0 / 6.0)).abs() < 1e-9, "{start}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();

    // none: nothing snaps, as if Alt were held.
    choose_grid(&mut harness, "none");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::None));
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    let free = beat_b_start(&harness).unwrap_or_default();
    assert!((free - 65.3).abs() < 0.02 && (free * 4.0).fract().abs() > 0.01, "{free}");
}
