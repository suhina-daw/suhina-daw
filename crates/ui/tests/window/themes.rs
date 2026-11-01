// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Themes: style sheets, skins, layouts and theme packages in the window.

use super::mixer::gain_of;
use super::*;
use egui::Vec2;

#[test]
fn private_background_removes_the_opaque_workspace_backdrop_and_keeps_native_panels() {
    use suhina_ui::settings::themes::ThemeEntry;
    fn covers_workspace(shape: &egui::Shape, ink: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| covers_workspace(shape, ink)),
            egui::Shape::Rect(rect) => {
                rect.fill == ink && rect.rect.width() > SIZE.0 * 0.9 && rect.rect.height() > SIZE.1 * 0.5
            }
            _ => false,
        }
    }
    let mut h = window(demo_app());
    let border = theme::border();
    assert!(h.output().shapes.iter().any(|shape| covers_workspace(&shape.shape, border)));
    let before = h.state().session().unwrap().project().clone();
    let ctx = h.ctx.clone();
    let mut paper = theme::Theme { name: "Private paper".into(), ..Default::default() };
    paper.colors.window_background = egui::Color32::TRANSPARENT;
    paper.skin.background = Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNQ6nD5DwADrAHuJHH6EQAAAABJRU5ErkJggg==".into());
    paper.skin.background_opacity = 100;
    h.state_mut().put_theme(&ctx, ThemeEntry { id: "private-paper".into(), theme: paper });
    h.state_mut().use_theme(&ctx, "private-paper");
    for _ in 0..100 {
        h.step();
        if suhina_ui::kit::liquid::background_image(&ctx).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let image = suhina_ui::kit::liquid::background_image(&ctx).expect("background worker completed");
    assert!(h.output().shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Mesh(mesh) if mesh.texture_id == image.id && mesh.calc_bounds().width() > SIZE.0 * 0.9
    )));
    assert!(
        !h.output().shapes.iter().any(|shape| covers_workspace(&shape.shape, border)),
        "separator ink must not conceal the image"
    );
    assert!(
        h.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.fill == theme::panel_background() && rect.rect.height() > 10.0
        )),
        "native panel surfaces remain opaque"
    );
    assert_eq!(h.state().session().unwrap().project(), &before);
}

#[test]
fn textured_empty_slots_do_not_clear_their_artwork_on_native_alpha_surfaces() {
    use std::sync::{Arc, Mutex};
    struct Replacement(Arc<Mutex<Vec<(egui::Rect, egui::Color32)>>>);
    impl suhina_ui::kit::surface::Provider for Replacement {
        fn shape(&self, rect: egui::Rect, radius: egui::CornerRadius, color: egui::Color32) -> egui::Shape {
            if let Ok(mut fills) = self.0.lock() {
                fills.push((rect, color));
            }
            egui::Shape::rect_filled(rect, radius, color)
        }
    }
    let mut h = window(demo_app());
    let ctx = h.ctx.clone();
    h.state_mut().use_theme(&ctx, "suhina-blocks");
    let fills = Arc::new(Mutex::new(Vec::new()));
    suhina_ui::kit::surface::install(&ctx, Arc::new(Replacement(fills.clone())));
    for _ in 0..4 {
        fills.lock().unwrap().clear();
        h.step();
    }
    let slot = h.get_by_label("Stop Keys (scene 1)").rect();
    assert!(
        !fills.lock().unwrap().iter().any(|(rect, color)| color.a() == 0
            && rect.min.distance(slot.min) < 1.0
            && rect.max.distance(slot.max) < 1.0),
        "transparent replacement fills erase the column artwork on a native GPU surface"
    );
}

#[test]
fn sheet_menu_and_toolbar_paint_their_scoped_backgrounds_without_editing_the_project() {
    let mut h = window(demo_app());
    let before = h.state().session().unwrap().project().clone();
    let ctx = h.ctx.clone();
    h.state_mut().use_theme(&ctx, "suhina-sheet");
    for _ in 0..4 {
        h.step();
    }
    let mut previous_bottom: Option<f32> = None;
    fn find_bar(shape: &egui::Shape, ink: egui::Color32, height: f32) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find_bar(shape, ink, height)),
            egui::Shape::Rect(rect)
                if rect.fill == ink
                    && rect.rect.width() >= SIZE.0 * 0.9
                    && (rect.rect.height() - height).abs() <= 1.0 =>
            {
                Some(rect.rect)
            }
            _ => None,
        }
    }
    for (color, height) in [(0x217346, theme::MENU_BAR), (0xf3f5f3, theme::CONTROL_BAR)] {
        let ink = egui::Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        let bar = h
            .output()
            .shapes
            .iter()
            .find_map(|shape| find_bar(&shape.shape, ink, height))
            .unwrap_or_else(|| panic!("missing scoped bar background {color:x}"));
        if let Some(bottom) = previous_bottom {
            assert!((bar.top() - bottom).abs() <= 1.0);
        }
        previous_bottom = Some(bar.bottom());
    }
    assert_eq!(h.state().session().unwrap().project(), &before);
}

#[test]
fn blocks_artwork_covers_slot_columns_and_keeps_native_clip_launches() {
    use suhina_ui::kit::skin;
    let mut h = view();
    let (_, mut blocks) = theme::built_in_themes().into_iter().find(|(id, _)| *id == "suhina-blocks").unwrap();
    skin::configure(&blocks.skin);
    theme::set_palette(&h.ctx, blocks.colors);
    h.run_steps(3);
    let slot = h.get_by_label("Launch Root on Bass (scene 1)").rect();
    let has_body = |h: &Harness<'static, SessionHarness>| {
        h.output().shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Mesh(mesh) if mesh.calc_bounds().contains(slot.center()) && mesh.calc_bounds().height() > slot.height() * 3.0
    ))
    };
    assert!(has_body(&h), "colour-keyed body artwork spans the slot column");
    assert_eq!(
        clicked(&mut h, "Launch Walk on Bass (scene 2)"),
        [Action::new("clip.launch", json!({"track":2,"scene":2}))]
    );
    blocks.skin.styles.get_mut(".track-column").unwrap().insert("--suhina-track-artwork".into(), "off".into());
    skin::configure(&blocks.skin);
    h.run_steps(3);
    assert!(!has_body(&h), "opting out restores native column fills");
    skin::configure(&skin::Skin::default());
    theme::set_palette(&h.ctx, theme::Palette::DARK);
}

#[test]
fn a_horizontal_session_fader_drags_on_its_drawn_axis_and_keeps_the_native_command() {
    use suhina_ui::kit::{skin, theme_css};
    let mut harness = view();
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, ".control-mixer-volume { --suhina-direction: horizontal; }").is_empty());
    skin::configure(&theme.skin);
    harness.run_steps(3);
    let rect = harness.get_by_label("Drums volume").rect();
    assert!(rect.width() > rect.height());
    let start = rect.center();
    harness.hover_at(start);
    harness.drag_at(start);
    harness.run_steps(1);
    harness.hover_at(start - Vec2::new(24.0, 0.0));
    harness.run_steps(2);
    harness.drop_at(start - Vec2::new(24.0, 0.0));
    harness.run_steps(2);
    let changes: Vec<_> = harness.state().actions.iter().filter(|a| a.id == "track.setVolume").collect();
    assert!(!changes.is_empty());
    assert_eq!(changes.last().unwrap().params["tracks"], json!([1]));
    assert_eq!(changes.last().unwrap().params["db"], json!(-6.0));
    assert_eq!(changes.last().unwrap().params["continue"], json!(true));
    skin::configure(&skin::Skin::default());
}

#[test]
fn an_individually_placed_send_keeps_its_parameter_target_and_can_be_hidden() {
    use suhina_ui::kit::{skin, theme_css};
    let mut harness = view();
    let before = harness.get_by_label("Bass send to A Reverb").rect();
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, ".control-mixer-send-1 { left: 6px; width: 16px; }").is_empty());
    skin::configure(&theme.skin);
    harness.run_steps(3);
    let moved = harness.get_by_label("Bass send to A Reverb").rect();
    assert!((moved.left() - before.left() - 6.0).abs() < 0.01);
    assert!((moved.width() - 16.0).abs() < 0.01);
    let start = moved.center();
    harness.hover_at(start);
    harness.drag_at(start);
    harness.run_steps(1);
    harness.hover_at(start - Vec2::new(0.0, 10.0));
    harness.run_steps(1);
    harness.drop_at(start - Vec2::new(0.0, 10.0));
    harness.run_steps(2);
    assert!(harness.state().actions.iter().any(|a| a.id == "track.setSend" && a.params["track"] == 2));
    theme.skin.controls.get_mut("mixer-send-1").unwrap().hidden = true;
    skin::configure(&theme.skin);
    harness.run_steps(3);
    assert!(harness.query_by_label("Bass send to A Reverb").is_none());
    skin::configure(&skin::Skin::default());
}

#[test]
fn session_density_keeps_slot_launches_and_clip_drop_targets_on_the_drawn_cells() {
    use suhina_ui::interaction::drop::{DropProbe, DropTarget};
    use suhina_ui::kit::{skin, theme_css};
    let mut harness = view();
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, "session { track-width: 80px; return-width: 72px; scene-width: 88px; header-height: 24px; slot-height: 18px; }").is_empty());
    let (restored, warnings) = theme::Theme::from_json(&theme.to_json());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(restored, theme);
    let mut mirrored = theme::Theme::default();
    assert!(theme_css::apply(&mut mirrored, &theme_css::export(&theme)).is_empty());
    assert_eq!(mirrored.skin.session, theme.skin.session);
    skin::configure(&theme.skin);
    harness.run_steps(3);
    let first = harness.get_by_label("Launch Root on Bass (scene 1)").rect();
    let second = harness.get_by_label("Launch Walk on Bass (scene 2)").rect();
    assert!((second.top() - first.top() - 19.0).abs() < 0.01);
    assert!((first.height() - 18.0).abs() < 0.01);
    assert_eq!(
        clicked(&mut harness, "Launch Walk on Bass (scene 2)"),
        [Action::new("clip.launch", json!({"track": 2, "scene": 2}))]
    );
    harness.state_mut().state.drop = Some(DropProbe::for_clip(second.center()));
    harness.run_steps(2);
    let probe = harness.state().state.drop.as_ref().unwrap();
    assert_eq!(probe.target, Some(DropTarget::Slot { track: TrackId(2), scene: 1 }));
    assert_eq!(probe.area.unwrap().top(), second.top());
    skin::configure(&skin::Skin::default());
}

#[test]
fn session_pan_knob_keeps_the_native_drag_command_and_value() {
    use suhina_ui::kit::{skin, theme_css};
    let mut harness = view();
    assert!(!harness.state().state.knobs);
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, ".pan { --suhina-appearance: knob; }").is_empty());
    skin::configure(&theme.skin);
    harness.run_steps(3);
    let start = harness.get_by_label("Bass pan").rect().center();
    harness.hover_at(start);
    harness.drag_at(start);
    harness.run_steps(1);
    harness.hover_at(start - Vec2::new(0.0, 20.0));
    harness.run_steps(1);
    harness.drop_at(start - Vec2::new(0.0, 20.0));
    harness.run_steps(2);
    assert!(harness.state().actions.iter().any(|action| action.id == "track.setPan"
        && action.params["tracks"] == json!([2])
        && action.params["pan"] == json!(0.2)));
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_variant_swatch_in_settings_uses_that_variant_and_new_variant_makes_one() {
    let mut harness = window(demo_app());
    let sheet = "@property --glow { syntax: \"<number>\"; initial-value: 0.5; label: \"Glow\"; range: 0 1; }\n\
                 :root { --accent: #123456; }\ntheme { name: \"Ink\"; }";
    let (ink, warnings) = theme::from_sheet(sheet, "");
    assert!(warnings.is_empty(), "{warnings:?}");
    let dim = suhina_ui::kit::theme_vars::Variant {
        id: "dim".into(),
        name: "Dim".into(),
        light: Some(false),
        values: [("--accent".to_string(), "#ABCDEF".to_string())].into(),
    };
    let (ink_dim, warnings) = suhina_ui::kit::theme_vars::with_variant(&ink, Some(&dim));
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = vec![
        suhina_ui::settings::themes::ThemeEntry { id: "ink".into(), theme: ink },
        suhina_ui::settings::themes::ThemeEntry { id: "ink@dim".into(), theme: ink_dim },
    ];
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, entries, Some("themes".into()));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    // One card for the theme, with a swatch for its own values and one for the variant.
    assert!(harness.query_by_label("Theme: Ink").is_some());
    harness.get_by_label("Variant: Dim").click();
    harness.run();
    assert_eq!(theme::accent(), egui::Color32::from_rgb(0xAB, 0xCD, 0xEF));
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.theme), Some("ink@dim".into()));
    // The theme in use shows its swatches on its large card and on its card in the list; both work.
    let own: Vec<_> = harness.get_all_by_label("Variant: Theme's own").collect();
    assert_eq!(own.len(), 2, "the large card's swatch and the list card's");
    own[0].click();
    harness.run();
    assert_eq!(theme::accent(), egui::Color32::from_rgb(0x12, 0x34, 0x56));
    // A new variant starts from the values in use and is handed out to be written.
    harness.get_by_label("More").click();
    harness.run();
    harness.get_by_label("New variant").click();
    harness.run();
    let saved: Vec<_> = harness
        .state_mut()
        .take_theme_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(saved, ["ink@variant"]);
    assert_eq!(harness.state().settings().theme, "ink@variant");
}

#[test]
fn ctrl_and_the_wheel_over_a_fader_change_the_volume_and_the_wheel_alone_does_not() {
    let mut harness = arrangement_app();
    let at = harness.get_by_label("Drums volume").rect().center();
    harness.hover_at(at);
    harness.run();
    let volume = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[0].volume).unwrap_or(1.0);
    // One line of a wheel turned up, as a wheel reports it (the content moves down).
    let tick = |modifiers: Modifiers| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers,
        phase: egui::TouchPhase::Move,
    };
    // The wheel alone scrolls the view: scrolling down the headers never moves a fader.
    harness.input_mut().events.push(tick(Modifiers::NONE));
    harness.run();
    assert!((volume(&harness) - 1.0).abs() < 1e-9, "the wheel alone left it: {}", volume(&harness));
    // Ctrl and the wheel move the fader, and the view stays where it is.
    let scroll = harness.state().arrangement().scroll;
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.input_mut().events.push(tick(Modifiers::COMMAND));
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert!((volume(&harness) - gain_of(1.0)).abs() < 1e-6, "one tick raised it by 1 dB: {}", volume(&harness));
    assert_eq!(harness.state().arrangement().scroll, scroll, "and the view stayed where it was");
    // Alt now belongs to height zoom, even over the same value control.
    let before = volume(&harness);
    let height = harness.get_by_label("Track Drums").rect().height();
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.input_mut().events.push(tick(Modifiers::ALT));
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(volume(&harness), before, "Alt+wheel must not change the fader");
    assert!(harness.get_by_label("Track Drums").rect().height() > height, "Alt+wheel still zooms height");
}

#[test]
fn horizontal_wheel_over_track_headers_does_not_scroll_the_timeline() {
    let mut harness = arrangement_app();
    let header = harness.get_by_label("Track Drums").rect();
    harness.hover_at(header.center_top() + egui::vec2(0.0, 6.0));
    harness.run();
    let before = harness.state().arrangement().scroll.x;
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-120.0, 0.0),
        modifiers: Modifiers::SHIFT,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    assert_eq!(harness.state().arrangement().scroll.x, before);
}

#[test]
fn shift_wheel_over_clip_gain_scrolls_timeline_without_editing_gain() {
    let mut harness = window(audio_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let before = harness.state().session().unwrap().project().clone();
    let field = harness.get_by_label("Gain of Loop on Loop").rect().center();
    harness.hover_at(field);
    harness.run();
    let scroll = harness.state().arrangement().scroll.x;
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-120.0, 0.0),
        modifiers: Modifiers::SHIFT,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    assert!(harness.state().arrangement().scroll.x > scroll);
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn waveform_colour_changes_live_in_settings_without_changing_the_project() {
    use suhina_ui::settings::view_settings::WaveformColour;
    let mut h = window(audio_app());
    h.get_by_label("Arrangement view").click();
    h.run();
    let custom = egui::Color32::from_rgb(123, 210, 57);
    let mut settings = h.state().settings().clone();
    settings.view.waveform.custom = custom;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Search settings").focus();
    h.run();
    h.get_by_label("Search settings").type_text("waveform colour");
    h.run();
    fn has_ink(shape: &egui::Shape, clip: egui::Rect, ink: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| has_ink(s, clip, ink)),
            egui::Shape::Mesh(mesh) => mesh.vertices.iter().any(|v| clip.contains(v.pos) && v.color == ink),
            egui::Shape::Rect(rect) => rect.fill == ink && clip.contains(rect.rect.center()),
            _ => false,
        }
    }
    for (name, mode, colour) in [
        ("Light", WaveformColour::Light, egui::Color32::from_rgb(240, 237, 232)),
        ("Dark", WaveformColour::Dark, egui::Color32::from_rgb(32, 31, 29)),
        ("Custom", WaveformColour::Custom, custom),
    ] {
        h.get_by_label(&format!("Waveform and note colour: {name}")).click();
        h.run();
        assert_eq!(h.state().settings().view.waveform.mode, mode);
        let clip = h.get_by_label("Clip Loop on Loop at bar 1").rect();
        assert!(
            h.output().shapes.iter().any(|s| has_ink(&s.shape, clip, colour)),
            "{name} applies while Settings is open"
        );
        let midi = h.get_by_label("Clip Beat A on Drums at bar 1").rect();
        assert!(h.output().shapes.iter().any(|s| has_ink(&s.shape, midi, colour)), "{name} also applies to MIDI notes");
    }
    assert!(h.query_by_label("Custom waveform colour").is_some());
    h.get_by_label("Waveform and note colour: Auto").click();
    h.run();
    assert!(h.query_by_label("Custom waveform colour").is_none());
    assert!(!h.state().session().unwrap().can_undo());
}

#[test]
fn ara_note_overlays_use_the_same_content_colour_as_midi_and_waveforms() {
    use suhina_ui::settings::view_settings::WaveformColour;
    let (project, waveforms) = project_with_audio();
    let mut timeline = Timeline::build(&project, None);
    timeline.waveforms = std::sync::Arc::new(waveforms);
    let clip = timeline.lanes.iter().find(|lane| lane.track == TrackId(30)).unwrap().clips[0].id;
    timeline.ara_notes.insert(
        clip,
        suhina_session::ClipNotes {
            notes: vec![suhina_plugins::protocol::AraNote { start: 0.5, length: 1.0, pitch: 60, volume: 1.0 }].into(),
            low: 60,
            high: 60,
        },
    );
    let mut h = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, state: &mut (Timeline, ArrangementViewState, Vec<Action>)| {
            arrangement_view::show(ui, &state.0, &mut state.1, &mut state.2)
        },
        (timeline, ArrangementViewState::default(), Vec::new()),
    );
    theme::install(&h.ctx);
    for (mode, colour) in [
        (WaveformColour::Dark, egui::Color32::from_rgb(32, 31, 29)),
        (WaveformColour::Custom, egui::Color32::from_rgb(212, 88, 50)),
    ] {
        h.state_mut().1.waveform_appearance.mode = mode;
        h.state_mut().1.waveform_appearance.custom = colour;
        h.run();
        let area = h.get_by_label("Clip Loop on Loop at bar 1").rect();
        fn note(shape: &egui::Shape, area: egui::Rect, colour: egui::Color32) -> bool {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| note(s, area, colour)),
                egui::Shape::Rect(r) => r.fill == colour && r.rect.height() <= 6.0 && area.contains(r.rect.center()),
                _ => false,
            }
        }
        assert!(h.output().shapes.iter().any(|s| note(&s.shape, area, colour)), "ARA notes follow {mode:?}");
    }
}

#[test]
fn audio_and_header_colour_strengths_apply_live_and_match_at_equal_strength() {
    let mut h = window(audio_app());
    h.get_by_label("Arrangement view").click();
    h.run();
    fn surface(h: &Harness<'static, App>, label: &str) -> egui::Color32 {
        let area = h.get_by_label(label).rect();
        fn find(shape: &egui::Shape, area: egui::Rect) -> Option<egui::Color32> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, area)),
                egui::Shape::Rect(r)
                    if (r.rect.width() - area.width()).abs() < 3.0
                        && (r.rect.height() - area.height()).abs() < 3.0
                        && r.rect.center().distance(area.center()) < 2.0
                        && r.fill.a() > 0 =>
                {
                    Some(r.fill)
                }
                _ => None,
            }
        }
        h.output().shapes.iter().find_map(|s| find(&s.shape, area)).expect("painted surface")
    }
    let clip = "Clip Loop on Loop at bar 1";
    let original = surface(&h, clip);
    assert_eq!(surface(&h, "Track Loop"), original, "the same source colour uses the same default strength");
    let mut settings = h.state().settings().clone();
    settings.view.audio_colour_strength = 25;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings.clone());
    h.run();
    let softer = surface(&h, clip);
    assert_ne!(softer, original);
    assert_eq!(surface(&h, "Track Loop"), original, "audio tuning leaves the header's chosen strength alone");
    settings.view.header_colour_strength = 25;
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert_eq!(surface(&h, "Track Loop"), softer, "matched strengths keep the source colour consistent");
    assert!(!h.state().session().unwrap().can_undo());
}

#[test]
fn paste_uses_an_empty_lane_click_and_can_replace_a_selected_clip() {
    let mut harness = arrangement_app();
    let ctx = harness.ctx.clone();
    let mut settings = harness.state().settings().clone();
    settings.view.replace_clip = true;
    harness.state_mut().set_settings(&ctx, settings);
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    let destination = harness.state().selected_clip().unwrap().1;
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    let clips = &harness.state().session().unwrap().project().arrangement.lanes[0].clips;
    assert!(clips.iter().all(|clip| clip.id != destination));
    assert!(clips.iter().any(|clip| clip.start == 0.0 && clip.length == 16.0));
    // There is now empty space between the pasted short clip and Beat B.
    let lane = harness.get_by_label("Lane Drums").rect();
    let start = harness.get_by_label("Clip Fill on Drums at bar 1").rect().right();
    let at = egui::pos2(start + 20.0, lane.center().y);
    harness.hover_at(at);
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    let (track, beat) = harness.state().arrangement().insert.unwrap();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert!(
        harness
            .state()
            .session()
            .unwrap()
            .project()
            .arrangement
            .lanes
            .iter()
            .filter(|lane| lane.track == Some(track))
            .flat_map(|lane| &lane.clips)
            .any(|clip| (clip.start - beat).abs() < 1e-6)
    );
}

#[test]
fn loop_boundaries_appear_while_extending_the_clip_before_release() {
    for (looped, period) in [(true, 4.0), (false, 4.0), (true, 1.0), (true, 8.0)] {
        let mut project = demo_project();
        let clips = &mut project.arrangement.lanes[0].clips;
        clips.retain(|c| (c.start - 64.0).abs() < 1e-6);
        clips[0].length = 4.0;
        clips[0].offset = 0.0;
        clips[0].loop_range = looped.then_some(suhina_engine::model::LoopRange { start: 0.0, end: period });
        let mut app = App::new();
        app.set_session(Session::new(project), &[]);
        let mut h = window(app);
        h.get_by_label("Arrangement view").click();
        h.run();
        let clip = h.get_by_label("Clip Beat B on Drums at bar 17").rect();
        let edge = h.get_by_label("Right edge of Beat B on Drums").rect().center();
        h.hover_at(edge);
        h.run();
        h.drag_at(edge);
        h.run();
        for dx in [16.0, 32.0, 64.0] {
            h.hover_at(edge + egui::vec2(dx, 0.0));
            h.run();
        }
        fn notch_at(shape: &egui::Shape, x: f32, y: f32) -> bool {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| notch_at(s, x, y)),
                egui::Shape::Circle(circle) => {
                    (circle.center.x - x).abs() < 1.0
                        && (circle.center.y + circle.radius * 0.5 - y).abs() < 0.5
                        && circle.radius > 0.0
                        && circle.radius <= 2.5
                }
                _ => false,
            }
        }
        for boundary in
            [1.0, 2.0].into_iter().filter(|n| n * period < 12.0).map(|n| clip.left() + (n * period * 8.0) as f32 + 0.5)
        {
            let drawn = h
                .output()
                .shapes
                .iter()
                .any(|s| notch_at(&s.shape, boundary, clip.top()) && s.clip_rect.top() >= clip.top());
            assert_eq!(
                drawn,
                looped && period * 8.0 >= 64.0,
                "live notch at {boundary}, looped={looped}, period={period}"
            );
            fn boundary_at(shape: &egui::Shape, x: f32, clip: egui::Rect) -> bool {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().any(|s| boundary_at(s, x, clip)),
                    egui::Shape::LineSegment { points, .. } => {
                        points.iter().all(|p| (p.x - x).abs() < 0.5)
                            && (points[0].y - clip.top()).abs() < 0.5
                            && (points[1].y - clip.bottom()).abs() < 0.5
                    }
                    _ => false,
                }
            }
            let line = h.output().shapes.iter().any(|s| boundary_at(&s.shape, boundary, clip));
            assert_eq!(line, looped, "boundary remains visible when distant notches are suppressed");
        }
        assert_eq!(
            h.state().session().unwrap().project().arrangement.lanes[0].clips[0].length,
            4.0,
            "the preview is checked before committing the resize"
        );
        h.drop_at(edge + egui::vec2(64.0, 0.0));
        h.run();
    }
}

#[test]
fn the_arrangement_track_on_toggle_is_control_track_on() {
    fn filled(shape: &egui::Shape, at: egui::Rect, ink: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| filled(shape, at, ink)),
            egui::Shape::Rect(rect) => rect.fill == ink && rect.rect.intersects(at),
            _ => false,
        }
    }
    let orange = egui::Color32::from_rgb(0xFF, 0x88, 0x00);
    suhina_ui::kit::native_css::set_user_sheet(Some("#control-track-on { background-color: #ff8800; }".into()));
    let mut h = window(demo_app());
    h.get_by_label("Arrangement view").click();
    h.run();
    let toggle = h.get_by_label("Turn Drums off").rect();
    assert!(h.output().shapes.iter().any(|s| filled(&s.shape, toggle, orange)), "the toggle takes #control-track-on");
    suhina_ui::kit::native_css::set_user_sheet(None);
}

#[test]
fn the_browser_s_places_and_rows_take_their_own_rules() {
    fn filled(shape: &egui::Shape, ink: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| filled(shape, ink)),
            egui::Shape::Rect(rect) => rect.fill == ink,
            _ => false,
        }
    }
    let magenta = egui::Color32::from_rgb(0xFF, 0x00, 0xFF);
    suhina_ui::kit::native_css::set_user_sheet(Some(".browser-place:checked { background-color: #ff00ff; }".into()));
    let mut h = window(demo_app());
    if let Some(show) = h.query_by_label("Show browser") {
        show.click();
    }
    h.run();
    let shown = h.output().shapes.iter().any(|s| filled(&s.shape, magenta));
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(shown, "the chosen place takes .browser-place:checked");
}

#[test]
fn the_loop_lines_reach_down_past_the_returns_to_main() {
    fn tallest_line(shape: &egui::Shape, ink: egui::Color32) -> f32 {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(|s| tallest_line(s, ink)).fold(0.0, f32::max),
            egui::Shape::LineSegment { points, stroke } if stroke.color == ink && points[0].x == points[1].x => {
                points[0].y.max(points[1].y)
            }
            _ => 0.0,
        }
    }
    let mut project = demo_project();
    project.transport.loop_range = Some(suhina_engine::model::LoopRange { start: 0.0, end: 4.0 });
    project.transport.loop_enabled = true;
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    suhina_ui::kit::native_css::set_user_sheet(Some(".loop-line { color: #00ff00; }".into()));
    let mut h = window(app);
    h.get_by_label("Arrangement view").click();
    h.run();
    let green = egui::Color32::from_rgb(0, 0xFF, 0);
    let tallest = h.output().shapes.iter().map(|s| tallest_line(&s.shape, green)).fold(0.0, f32::max);
    let main = h.query_all_by_label_contains("Main").map(|n| n.rect().top()).fold(0.0, f32::max);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(tallest > 0.0, "the line takes .loop-line's colour");
    assert!(tallest > main, "it reaches down past Main's row: ends at {tallest}, Main at {main}");
}

#[test]
fn choosing_another_themes_folder_asks_before_its_themes_move() {
    use suhina_ui::interaction::pick::PickField;
    let mut h = window(demo_app());
    let chosen = std::env::temp_dir().join("suhina-themes-elsewhere");
    h.state_mut().set_picked(PickField::ThemesFolder, chosen.clone());
    h.run();
    assert!(h.state().settings().themes_folder.is_empty(), "nothing changes before the answer");
    h.get_by_label("Leave them where they are").click();
    h.run();
    assert!(h.query_by_label("Move your themes").is_none());
    assert!(h.state().settings().themes_folder.is_empty() && h.state_mut().take_theme_move().is_none());
    h.state_mut().set_picked(PickField::ThemesFolder, chosen.clone());
    h.run();
    h.get_by_label("Move your themes").click();
    h.run();
    assert_eq!(h.state().settings().themes_folder, chosen.display().to_string());
    assert!(h.state_mut().take_theme_move().is_some_and(|(_, to)| to == chosen));
}
