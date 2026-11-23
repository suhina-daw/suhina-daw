// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn find<'a>(entries: &'a [Entry], id: &str) -> Option<&'a Item> {
    entries.iter().find_map(|entry| match entry {
        Entry::Item(item) if item.id == id => Some(item),
        Entry::Submenu { entries, .. } => find(entries, id),
        _ => None,
    })
}

#[test]
fn an_open_menu_s_background_covers_the_menu_not_the_window() {
    let ctx = egui::Context::default();
    let skin = crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from("menu { background-color: #FF0000; }")),
        ..Default::default()
    };
    crate::kit::skin::configure(&skin);
    let mut biggest = 0.0f32;
    for frame in 0..5 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                crate::kit::native_css::begin_frame(ui);
                let _frame = crate::kit::native_css::frame(ui);
                let shown = bar_menu(ui, "Options", |ui| {
                    let _ = menu::item(ui, "Settings…", false);
                });
                if frame == 0 {
                    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&shown.response));
                }
            },
        );
        output.textures_delta.clear();
        for shape in &output.shapes {
            // Red, faded while the menu opens.
            if let egui::Shape::Rect(rect) = &shape.shape
                && let [r, 0, 0, _] = rect.fill.to_array()
                && r > 0
            {
                biggest = biggest.max(rect.rect.area());
            }
        }
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    assert!(biggest > 0.0, "the menu has its background");
    assert!(biggest < 1280.0 * 800.0 / 4.0, "only the menu's area: {biggest}");
}

/// An open menu of the bar is one box, `menu`'s, in place of egui's frame: its background
/// painted once, however see-through, and its outline.
#[test]
fn an_open_menu_is_one_box_the_menu_s() {
    let ctx = egui::Context::default();
    crate::kit::skin::configure(&crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(
            "menu { background-color: #FF000080; outline: 2px solid #00FF00; backdrop-filter: none; }",
        )),
        ..Default::default()
    });
    let (mut backgrounds, mut outline) = (0, false);
    for frame in 0..5 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                time: Some(f64::from(frame)),
                ..Default::default()
            },
            |ui| {
                crate::kit::native_css::begin_frame(ui);
                let _frame = crate::kit::native_css::frame(ui);
                crate::kit::liquid::begin_frame(ui.ctx(), Default::default());
                let shown = bar_menu(ui, "Options", |ui| {
                    let _ = menu::item(ui, "Settings…", false);
                });
                if frame == 0 {
                    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&shown.response));
                }
                crate::kit::liquid::finish_backdrops(ui.ctx());
            },
        );
        output.textures_delta.clear();
        (backgrounds, outline) = (0, false);
        for shape in &output.shapes {
            if let egui::Shape::Rect(rect) = &shape.shape {
                backgrounds += usize::from(matches!(rect.fill.to_array(), [r, 0, 0, _] if r > 0));
                outline |= matches!(rect.stroke.color.to_array(), [0, g, 0, _] if g > 0);
            }
        }
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    assert_eq!(backgrounds, 1, "the menu's background, once");
    assert!(outline, "the menu's outline");
}

#[test]
fn without_a_project_saving_is_grayed_out_and_opening_is_not() {
    let menus = crate::App::new().menus();
    let titles: Vec<_> = menus.iter().map(|menu| menu.title).collect();
    assert_eq!(titles, ["File", "Edit", "View", "Projects", "Options", "Help"]);
    let file = &menus[0].entries;
    assert!(find(file, "file.open").is_some_and(|item| item.enabled));
    assert!(find(file, "file.save").is_some_and(|item| !item.enabled));
    assert!(
        file.iter()
            .any(|entry| matches!(entry, Entry::Submenu { label, enabled: false, .. } if label == "Open recent"))
    );
    assert!(find(&menus[1].entries, "edit.undo").is_some_and(|item| !item.enabled));
}

/// One frame of two titles side by side, with `events` as the frame's input: the menus
/// open afterwards.
fn titles_frame(ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<&'static str> {
    let mut open = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0))),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for title in ["First", "Second"] {
                bar_menu(ui, title, |ui| {
                    open.push(title);
                    ui.label(title);
                });
            }
        });
    });
    output.textures_delta.clear();
    open
}

fn press_and_release(at: egui::Pos2) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}

/// With a later menu open, a click that reaches an earlier title in the frame the pointer
/// arrives (a press and a release with the move) opens that title's menu and leaves it
/// open: hover switching does not take the click's toggle.
#[test]
fn a_click_in_the_frame_the_pointer_arrives_leaves_the_earlier_menu_open() {
    let ctx = egui::Context::default();
    crate::kit::theme::install(&ctx);
    titles_frame(&ctx, vec![]);
    // Open the second title with a click on it: the pointer arrives a frame before.
    let first_at = egui::pos2(15.0, 12.0);
    let second_at = egui::pos2(80.0, 12.0);
    titles_frame(&ctx, vec![egui::Event::PointerMoved(second_at)]);
    titles_frame(&ctx, press_and_release(second_at));
    let open = titles_frame(&ctx, vec![]);
    assert_eq!(open, ["Second"], "the second title is open first");
    // The pointer arrives at the first title and clicks in the same frame.
    let mut events = vec![egui::Event::PointerMoved(first_at)];
    events.extend(press_and_release(first_at));
    titles_frame(&ctx, events);
    let open = titles_frame(&ctx, vec![]);
    assert_eq!(open, ["First"], "the first title's menu is open after its click, the second's is closed");
}

/// Moving onto another title while a menu is open switches to its menu with no click, and
/// a click on the open title closes it.
#[test]
fn hovering_another_title_switches_menus_and_a_click_on_the_open_one_closes_it() {
    let ctx = egui::Context::default();
    crate::kit::theme::install(&ctx);
    let first_at = egui::pos2(15.0, 12.0);
    let second_at = egui::pos2(80.0, 12.0);
    titles_frame(&ctx, vec![]);
    titles_frame(&ctx, vec![egui::Event::PointerMoved(second_at)]);
    titles_frame(&ctx, press_and_release(second_at));
    assert_eq!(titles_frame(&ctx, vec![]), ["Second"]);
    titles_frame(&ctx, vec![egui::Event::PointerMoved(first_at)]);
    titles_frame(&ctx, vec![]);
    assert_eq!(titles_frame(&ctx, vec![]), ["First"], "the pointer alone switched the menu");
    titles_frame(&ctx, press_and_release(first_at));
    assert!(titles_frame(&ctx, vec![]).is_empty(), "a click on the open title closed it");
}
