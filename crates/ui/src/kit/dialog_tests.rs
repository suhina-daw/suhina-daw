// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Vec2};

use super::*;

/// What `draw` gives in the third frame under the theme `css` (the tree takes frames to resolve),
/// on the base sheet when `base`.
fn frames_on<R>(base: bool, css: &str, mut draw: impl FnMut(&egui::Ui) -> R) -> R {
    crate::kit::skin::configure(&crate::kit::skin::Skin { css: Some(Arc::from(css)), base, ..Default::default() });
    let ctx = egui::Context::default();
    let mut result = None;
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = crate::kit::native_css::frame_with_motion(ui, false);
            result = Some(draw(ui));
        })
        .textures_delta
        .clear();
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    result.unwrap()
}

/// [`frames_on`] the base sheet.
fn frames<R>(css: &str, draw: impl FnMut(&egui::Ui) -> R) -> R {
    frames_on(true, css, draw)
}

const OUTER: Rect = Rect { min: Pos2::new(100.0, 100.0), max: Pos2::new(500.0, 300.0) };

/// The title row runs along the top inside the border, whatever the padding; what the dialog
/// holds goes inside the border and the padding, under the title row. A rule for one dialog by its
/// name reaches that one only.
#[test]
fn the_padding_is_the_room_inside_and_the_title_row_lies_outside_it() {
    let css = "dialog { padding: 12px; border-width: 2px; border-color: #808080; } \
               dialog[name=\"export\"] { padding: 20px; }";
    let (other, export) = frames(css, |ui| {
        let painted = |name: &str| {
            let dialog = enter(name);
            let (painted, title) = paint(ui.painter(), &dialog, OUTER, Color32::BLACK, Some("Title"));
            drop(title);
            (painted.title, painted.content)
        };
        (painted("save"), painted("export"))
    });
    let (title, content) = other;
    assert_eq!(title, Rect::from_min_max(Pos2::new(102.0, 102.0), Pos2::new(498.0, 102.0 + TITLE)));
    assert_eq!(content, Rect::from_min_max(Pos2::new(114.0, 102.0 + TITLE + 12.0), Pos2::new(486.0, 286.0)));
    let (title, content) = export;
    assert_eq!(title.top(), 102.0, "the padding does not move the title row");
    assert_eq!(content.min, Pos2::new(122.0, 102.0 + TITLE + 20.0));
}

/// A dialog's box is its contents and the room around them: the title row, the border and the
/// padding.
#[test]
fn a_dialog_is_as_big_as_its_contents_and_the_room_around_them() {
    let size = frames("dialog { padding: 10px; border-width: 1px; border-color: #808080; }", |_| {
        let dialog = enter("save");
        size(&dialog, Rect::from_min_size(Pos2::ZERO, Vec2::splat(1000.0)), Vec2::new(300.0, 100.0), true)
    });
    assert_eq!(size, Vec2::new(322.0, 100.0 + TITLE + 22.0));
}

/// Where the buttons of a `dialog-actions` row go, under `css` on the user-agent sheet alone, in a
/// content box 400 wide.
fn placed(css: &str, buttons: &[Action<'_>]) -> Vec<Option<Rect>> {
    frames_on(false, css, |ui| {
        let dialog = enter("save");
        let content = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 100.0));
        let element = DIALOG_ACTIONS.enter();
        let bar = Rect::from_min_max(Pos2::new(0.0, 100.0 - BUTTON_HEIGHT), content.max);
        let items: Vec<Item> = buttons
            .iter()
            .map(|b| {
                if b.space {
                    Item::new("flexible-space")
                } else {
                    Item::new("button").width(b.width).height(BUTTON_HEIGHT)
                }
            })
            .collect();
        let laid = row::row(&Bar::of(&element, bar.size()), &items).unwrap_or_default();
        drop(element);
        drop(dialog);
        let _ = actions(ui, ui.painter(), content, buttons);
        (0..buttons.len()).map(|i| laid.at(i, bar.min)).collect()
    })
}

/// The user-agent sheet puts the buttons at the end of the row, 8 points apart, in the order
/// given; a flexible space sends the ones before it to the start; a theme can place and space them
/// otherwise.
#[test]
fn the_sheet_places_a_dialog_s_buttons() {
    let two = [Action::new("Cancel", "Cancel"), Action::new("Save", "Save")];
    let at = placed("", &two);
    let (cancel, save) = (at[0].unwrap(), at[1].unwrap());
    assert_eq!(save.right(), 400.0, "at the end of the row");
    assert_eq!(save.left() - cancel.right(), 8.0);
    assert_eq!(save.size(), Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));

    let split = [Action::new("Check now", "Check now"), Action::space(), Action::new("Install", "Install").wide(150.0)];
    let at = placed("", &split);
    assert_eq!(at[0].unwrap().left(), 0.0, "before the space, at the start");
    assert_eq!(at[2].unwrap().right(), 400.0);
    assert_eq!(at[2].unwrap().width(), 150.0);

    let at = placed("dialog-actions { justify-content: flex-start; gap: 20px; }", &two);
    assert_eq!(at[0].unwrap().left(), 0.0);
    assert_eq!(at[1].unwrap().left(), BUTTON_WIDTH + 20.0);
}

/// On the user-agent sheet alone a dialog is filled in its opaque background, not in the panel
/// fill such a theme leaves clear.
#[test]
fn without_the_base_sheet_a_dialog_is_opaque() {
    let fills = frames_on(false, "", |ui| {
        let dialog = enter("save");
        let layer = ui.layer_id();
        let start = ui.ctx().graphics_mut(|layers| layers.entry(layer).next_idx().0);
        let (_, title) = paint(ui.painter(), &dialog, OUTER, Color32::TRANSPARENT, Some("Title"));
        drop(title);
        ui.ctx().graphics(|layers| {
            layers
                .get(layer)
                .map(|list| {
                    list.all_entries()
                        .skip(start)
                        .filter_map(|clipped| match &clipped.shape {
                            egui::Shape::Rect(rect) if rect.rect == OUTER => Some(rect.fill),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    assert!(fills.contains(&Color32::from_rgb(0x1e, 0x1e, 0x1e)), "{fills:?}");
}
