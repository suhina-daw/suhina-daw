// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scroll bar of a list a view lays out itself. A list built from `ui.interact` rows at
//! offsets it keeps (the browser's rows, so that only the visible ones are made) has no
//! [`egui::ScrollArea`] around it, so it gets the bar of one here: the same bar as every scroll
//! area in the window (a device's panel, the settings), which is dragged, clicked in its track
//! and hovered like theirs (issue #867). A bar only painted would show where the list is and
//! take no pointer.

use std::hash::Hash;

use egui::scroll_area::{ScrollBarVisibility, ScrollSource};
use egui::{Rect, Ui, UiBuilder};

/// Gives `area`, whose content is `content` points tall and scrolled `offset` points down, a
/// vertical scroll bar on its right edge while the content is taller than it, and writes back
/// the offset the bar moved it to.
///
/// Call it after the list's rows, so the bar is on top of them where they meet and takes the
/// pointer there; a move shows on the next frame, which it asks for. The wheel stays the list's
/// own: the bar does not take it, so the list keeps any rule it has for the wheel.
pub fn vertical_bar(ui: &mut Ui, area: Rect, id_salt: impl Hash + std::fmt::Debug, content: f32, offset: &mut f32) {
    let most = (content - area.height()).max(0.0);
    *offset = offset.clamp(0.0, most);
    if most <= 0.0 {
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(area).id_salt(id_salt));
    let output = egui::ScrollArea::vertical()
        .id_salt("list-scroll-bar")
        .auto_shrink([false, false])
        .scroll_source(ScrollSource::SCROLL_BAR)
        .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
        .vertical_scroll_offset(*offset)
        .show_viewport(&mut child, |ui, _| {
            // Room for the content, nothing in it: the rows are the view's own.
            ui.set_min_height(content);
        });
    let moved = output.state.offset.y.clamp(0.0, most);
    if (moved - *offset).abs() > f32::EPSILON {
        *offset = moved;
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui::{Pos2, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    const AREA: Rect = Rect { min: Pos2::new(10.0, 10.0), max: Pos2::new(210.0, 210.0) };

    /// A list of rows that each take clicks and drags across the whole width, as the browser's
    /// do, with the bar over them.
    fn list(offset: f32) -> Harness<'static, f32> {
        Harness::builder().with_size(Vec2::new(300.0, 300.0)).build_ui_state(
            |ui, offset: &mut f32| {
                for row in 0..100 {
                    let top = AREA.top() + row as f32 * 20.0 - *offset;
                    let rect = Rect::from_min_size(Pos2::new(AREA.left(), top), Vec2::new(AREA.width(), 20.0));
                    let shown = rect.intersect(AREA);
                    if shown.height() > 0.0 {
                        let _ = ui.interact(shown, egui::Id::new(("row", row)), egui::Sense::click_and_drag());
                    }
                }
                vertical_bar(ui, AREA, "test-list", 2000.0, offset);
            },
            offset,
        )
    }

    fn bar(harness: &Harness<'static, f32>) -> Rect {
        harness.get_by_role(Role::ScrollBar).rect()
    }

    #[test]
    fn the_bar_drags_the_list_over_rows_that_take_drags() {
        let mut harness = list(0.0);
        harness.run();
        let bar = bar(&harness);
        assert!(bar.right() <= AREA.right() + 0.5 && bar.left() > AREA.center().x, "on the right edge: {bar:?}");
        let from = Pos2::new(bar.center().x, AREA.top() + 5.0);
        harness.hover_at(from);
        harness.step();
        harness.drag_at(from);
        harness.step();
        for y in [20.0, 40.0, 60.0, 80.0] {
            harness.hover_at(from + Vec2::new(0.0, y));
            harness.step();
        }
        harness.drop_at(from + Vec2::new(0.0, 80.0));
        harness.run();
        // The thumb is a tenth of the track (200 of 2000 points): 80 points along it are 800.
        let offset = *harness.state();
        assert!((offset - 800.0).abs() < 20.0, "the list follows the thumb: {offset}");
    }

    #[test]
    fn a_click_in_the_track_moves_the_list_towards_it() {
        let mut harness = list(0.0);
        harness.run();
        let bar = bar(&harness);
        let at = Pos2::new(bar.center().x, AREA.bottom() - 5.0);
        harness.hover_at(at);
        harness.step();
        harness.drag_at(at);
        harness.step();
        harness.drop_at(at);
        harness.run();
        assert!(*harness.state() > 0.0, "a click under the thumb scrolls down: {}", harness.state());
    }

    #[test]
    fn content_that_fits_has_no_bar_and_an_offset_past_the_end_is_brought_back() {
        let mut short = Harness::builder()
            .with_size(Vec2::new(300.0, 300.0))
            .build_ui_state(|ui, offset: &mut f32| vertical_bar(ui, AREA, "short", 100.0, offset), 50.0);
        short.run();
        assert!(short.query_by_role(Role::ScrollBar).is_none());
        assert_eq!(*short.state(), 0.0);
        let mut long = list(5000.0);
        long.run();
        assert_eq!(*long.state(), 1800.0);
    }
}
