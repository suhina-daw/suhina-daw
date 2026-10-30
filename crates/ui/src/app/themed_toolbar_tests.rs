// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use egui_kittest::{Harness, kittest::Queryable};

#[test]
fn responsive_theme_survives_live_reload_and_resize() {
    std::thread::Builder::new()
        .name("native-window-stack".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let css = include_str!("../../tests/fixtures/responsive-theme.css");
            let ctx = egui::Context::default();
            let mut app = App::new();
            app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
            app.open_project(
                suhina_session::Session::new(crate::demo::demo_project()),
                &[],
                crate::app::OpenAs::NewTab,
            );
            app.shown = View::Arrangement;
            app.detail_open = true;
            app.set_browser_open(false);
            let mut theme = theme::Theme::default();
            theme.skin.css = Some(std::sync::Arc::from(css));
            app.set_themes(&ctx, vec![crate::settings::themes::ThemeEntry { id: "responsive".into(), theme }], None);
            app.use_theme(&ctx, "responsive");
            theme::install(&ctx);
            for (browser_index, browser) in [false, true].into_iter().enumerate() {
                app.set_browser_open(browser);
                App::set_animations(&ctx, !browser);
                for (sample, size) in [
                    Vec2::new(1138.0, 1032.0),
                    Vec2::new(1000.0, 1032.0),
                    Vec2::new(820.0, 1032.0),
                    Vec2::new(800.0, 700.0),
                    Vec2::splat(1.0),
                    Vec2::ZERO,
                    Vec2::new(1138.0, 1032.0),
                ]
                .into_iter()
                .cycle()
                .take(35)
                .enumerate()
                {
                    if sample == 0 || sample == 2 {
                        let mut replacement = theme::Theme::default();
                        replacement.skin.css = Some(std::sync::Arc::from(if sample == 0 {
                            ".window { background-color:#0A1120; }"
                        } else {
                            css
                        }));
                        app.put_theme(
                            &ctx,
                            crate::settings::themes::ThemeEntry { id: "responsive".into(), theme: replacement },
                        );
                    }
                    ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                            time: Some((browser_index * 35 + sample) as f64 * 0.025),
                            events: vec![egui::Event::PointerMoved(egui::pos2(
                                if sample % 2 == 0 { 532.0 } else { 560.0 },
                                69.0,
                            ))],
                            ..Default::default()
                        },
                        |ui| app.ui(ui),
                    )
                    .textures_delta
                    .clear();
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

struct Fixture {
    app: App,
    grid: TrackGrid,
    actions: Vec<Action>,
}

fn native_bar() -> Harness<'static, Fixture> {
    let fixture =
        Fixture { app: App::new(), grid: TrackGrid::build(&crate::demo::demo_project(), None), actions: Vec::new() };
    let mut harness = Harness::builder().with_size((1920.0, 240.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let skin = fixture.app.themes.get(&fixture.app.settings.theme).map(|t| t.skin.clone()).unwrap_or_default();
            skin::configure(&skin);
            let _frame = crate::kit::native_css::frame(ui);
            let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.available_width(), 56.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                fixture.app.control_bar(ui, Some(&fixture.grid), Some(10), Some(1024), &mut fixture.actions);
            });
        },
        fixture,
    );
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn start_edit(harness: &mut Harness<'static, Fixture>) {
    let skin =
        harness.state().app.themes.get(&harness.state().app.settings.theme).map(|t| t.skin.clone()).unwrap_or_default();
    let theme_id = harness.state().app.settings.theme.clone();
    harness.ctx.data_mut(|d| d.insert_temp(edit_id(), EditDraft { theme_id, original: skin.clone(), skin }));
    harness.run();
}

#[test]
fn native_toolbar_css_styles_modules_and_buttons_without_reflow_or_sibling_leaks() {
    let mut harness = native_bar();
    let play = harness.get_by_label("Play").rect();
    let tempo = harness.get_by_label("Tempo").rect();
    harness.state_mut().app.use_test_skin(skin::Skin {
            css:Some(std::sync::Arc::from("#tempo {color:rgb(12 34 56);font-size:18px;} #transport > #control-play {color:rgb(120 40 180);background:linear-gradient(90deg,red,blue);border-radius:7px;}")),
            ..Default::default()
        });
    harness.run();
    assert_eq!(harness.get_by_label("Play").rect(), play);
    assert_eq!(harness.get_by_label("Tempo").rect(), tempo);
    let tempo_text = format!("{:.3}", harness.state().grid.tempo_bpm);
    let position_text = harness.state().grid.position();
    let text = |label: &str| {
        harness
            .output()
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(&text.galley.job.sections[0].format),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(text(&tempo_text).color, egui::Color32::from_rgb(12, 34, 56));
    assert_eq!(text(&tempo_text).font_id.size, 18.0);
    assert_eq!(text(&position_text).font_id.size, theme::TEXT, "font must not leak into the next module");
    // `background` sets the colour too (transparent), so the gradient is the button's only fill:
    // Suhina's own button artwork is not drawn under it.
    assert!(!harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == play && rect.brush.is_some())));
    assert!(
        harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.calc_bounds() == play && mesh.vertices.len() > 4)),
        "gradient must remain visible above the button's surface"
    );
    harness.get_by_label("Play").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| action.id == "transport.play"));
    skin::configure(&skin::Skin::default());
}

#[test]
fn dark_gloss_example_keeps_native_layout_and_transport_commands() {
    assert!(crate::kit::native_css::inspect(theme::GLOSS_EXAMPLE).unwrap().is_empty());
    let mut harness = native_bar();
    let play = harness.get_by_label("Play").rect();
    let tempo = harness.get_by_label("Tempo").rect();
    harness
        .state_mut()
        .app
        .use_test_skin(skin::Skin { css: Some(std::sync::Arc::from(theme::GLOSS_EXAMPLE)), ..Default::default() });
    harness.run();
    assert_eq!(play, harness.get_by_label("Play").rect());
    assert_eq!(tempo, harness.get_by_label("Tempo").rect());
    assert!(!crate::kit::native_css::toolbar_layout_requested());
    harness.get_by_label("Play").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| action.id == "transport.play"));
    skin::configure(&skin::Skin::default());
}

#[test]
fn editing_keeps_real_controls_and_done_without_changes_preserves_native_layout() {
    let mut harness = native_bar();
    let play = harness.get_by_label("Play").rect();
    let tempo = harness.get_by_label("Tempo").rect();
    let theme_id = harness.state().app.settings.theme.clone();
    start_edit(&mut harness);
    assert_eq!(play, harness.get_by_label("Play").rect());
    assert_eq!(tempo, harness.get_by_label("Tempo").rect());
    harness.get_by_label("Move toolbar transport").click();
    harness.run();
    assert!(harness.state().actions.is_empty(), "editing must not play audio");
    harness.get_by_label("Done").click();
    harness.run();
    assert_eq!(harness.state().app.settings.theme, theme_id);
    assert_eq!(play, harness.get_by_label("Play").rect());
    assert_eq!(tempo, harness.get_by_label("Tempo").rect());
    assert!(harness.state().app.themes.get(&theme_id).unwrap().skin.toolbar.is_none());
    skin::configure(&skin::Skin::default());
}

#[test]
fn edit_overlay_preserves_css_grid_geometry_without_writing_a_layout() {
    let mut harness = native_bar();
    harness.state_mut().app.use_test_skin(skin::Skin {
            css: Some(std::sync::Arc::from(".toolbar {display:grid;grid-template-columns:100px 120px 90px;gap:8px;} #tempo {grid-column:1;grid-row:1;} #transport {grid-column:2;grid-row:1;} #position {grid-column:3;grid-row:1;} #browser,#signature,#grid,#count-in,#metronome,#mapping,#performance,#views,#agents {display:none;}")),
            ..Default::default()
        });
    harness.run();
    let play = harness.get_by_label("Play").rect();
    let tempo = harness.get_by_label("Tempo").rect();
    let theme_id = harness.state().app.settings.theme.clone();
    start_edit(&mut harness);
    assert_eq!(play, harness.get_by_label("Play").rect());
    assert_eq!(tempo, harness.get_by_label("Tempo").rect());
    harness.get_by_label("Done").click();
    harness.run();
    assert_eq!(play, harness.get_by_label("Play").rect());
    assert_eq!(harness.state().app.settings.theme, theme_id);
    assert!(harness.state().app.themes.get(&theme_id).unwrap().skin.toolbar.is_none());
    skin::configure(&skin::Skin::default());
}

#[test]
fn dragging_real_transport_snaps_and_saves_only_on_done_and_cancel_discards() {
    let mut harness = native_bar();
    let original = harness.get_by_label("Play").rect();
    start_edit(&mut harness);
    let from = harness.get_by_label("Move toolbar transport").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    let to = from + Vec2::new(53.0, 0.0);
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    let moved = harness.get_by_label("Play").rect();
    let bar_left = 8.0; // egui-kittest's default central-panel margin.
    assert!((moved.left() - original.left()).abs() > 40.0);
    assert!(((moved.left() - bar_left) / 8.0).fract().abs() < 0.001);
    assert!(harness.state().actions.is_empty());
    harness.get_by_label("Done").click();
    harness.run();
    assert_eq!(harness.get_by_label("Play").rect(), moved);
    let saved = harness.state().app.themes.get(&harness.state().app.settings.theme).unwrap().skin.clone();
    assert!(saved.toolbar.is_none(), "moving native modules must preserve native layout");
    assert!(saved.toolbar_offsets.contains_key("transport"));
    harness.set_size(Vec2::new(1600.0, 240.0));
    harness.run();
    assert!(
        (harness.get_by_label("Play").rect().left() - (moved.left() - 160.0)).abs() < 0.01,
        "transport keeps its centre anchor when the window narrows"
    );
    harness.set_size(Vec2::new(1920.0, 240.0));
    harness.run();
    start_edit(&mut harness);
    harness.get_by_label("Reset positions").click();
    harness.run();
    assert_eq!(harness.get_by_label("Play").rect(), original);
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(harness.get_by_label("Play").rect(), moved);
    harness.get_by_label("Play").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|a| a.id == "transport.play"));
    skin::configure(&skin::Skin::default());
}

#[test]
fn css_toolbar_targets_keep_native_commands_with_svg_artwork() {
    let mut app = App::new();
    let layout = Toolbar {
        hidden: skin::MODULES
            .iter()
            .filter(|id| !["tempo", "transport", "mapping"].contains(id))
            .map(|id| (*id).into())
            .collect(),
        ..Default::default()
    };
    app.use_test_skin(skin::Skin { assets:skin::example_assets(),toolbar:Some(layout),css:Some(std::sync::Arc::from(".toolbar { display:grid; grid-template-columns:80px 120px; gap:8px; } #tempo { grid-column:1; grid-row:1; width:auto; } #transport { grid-column:2; width:auto; } #mapping { display:none; }")),..Default::default() });
    let fixture = Fixture { app, grid: TrackGrid::build(&crate::demo::demo_project(), None), actions: Vec::new() };
    let mut harness = Harness::builder().with_size((640.0, 56.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let skin =
                fixture.app.themes.get(&fixture.app.settings.theme).map(|theme| theme.skin.clone()).unwrap_or_default();
            skin::configure(&skin);
            let _frame = crate::kit::native_css::frame(ui);
            fixture.app.control_bar(ui, Some(&fixture.grid), None, None, &mut fixture.actions);
        },
        fixture,
    );
    theme::install(&harness.ctx);
    harness.run();
    let tempo = harness.get_by_label("Tempo").rect();
    let play = harness.get_by_label("Play").rect();
    assert!(play.left() > tempo.right());
    assert!(harness.query_by_label("Map keys to slots and scenes").is_none());
    harness.get_by_label("Play").click();
    harness.run();
    assert!(harness.state().actions.contains(&Action::new("transport.play", json!({}))));
    harness.state_mut().actions.clear();
    harness.get_by_label("Stop").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| action.id == "transport.stop"));
    skin::configure(&skin::Skin::default());
}

#[test]
fn toolbar_module_backgrounds_follow_the_sheet() {
    let mut harness = native_bar();
    let tempo = harness.get_by_label("Tempo").rect();
    harness.state_mut().app.use_test_skin(skin::Skin {
        css: Some(std::sync::Arc::from(":root { --well: #102030; } #tempo, #position { background-color: var(--well); } #performance { background-color: transparent; }")),
        ..Default::default()
    });
    harness.run();
    let fills: Vec<_> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.rect.intersects(tempo) => Some((rect.rect, rect.fill)),
            _ => None,
        })
        .collect();
    skin::configure(&skin::Skin::default());
    assert!(fills.iter().any(|(_, fill)| *fill == egui::Color32::from_rgb(0x10, 0x20, 0x30)), "{fills:?}");
}
