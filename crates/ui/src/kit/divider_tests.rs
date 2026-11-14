// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use egui::{Event, PointerButton, Pos2, pos2, vec2};

/// Runs one frame with `events`, showing a divider along the bottom of a 400-point area whose
/// panel under it is `size` tall; gives the size a drag set.
fn frame(ctx: &egui::Context, events: Vec<Event>, size: f32) -> Option<f32> {
    let mut dragged = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
        events,
        ..Default::default()
    };
    ctx.run_ui(input, |ui| {
        let strip = Rect::from_min_max(pos2(0.0, 394.0 - size), pos2(400.0, 400.0 - size));
        let divider = show(
            ui,
            Divider {
                name: "detail-splitter",
                key: Id::unique("divider"),
                strip,
                across: false,
                label: "Test divider",
                size,
                grows_forward: false,
                bounds: (20.0, 300.0),
            },
        );
        dragged = divider.size;
    })
    .textures_delta
    .clear();
    dragged
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() }
}

/// The size follows the pointer from where the drag began, within its bounds, and nothing is
/// dragged once the button is let go.
#[test]
fn a_drag_sets_the_size_from_where_it_began_within_its_bounds() {
    let ctx = egui::Context::default();
    let start = pos2(200.0, 297.0);
    assert_eq!(frame(&ctx, vec![Event::PointerMoved(start)], 100.0), None);
    frame(&ctx, vec![press(start, true)], 100.0);
    // Up grows the panel under the divider: 30 points up, 130 tall.
    assert_eq!(frame(&ctx, vec![Event::PointerMoved(start - vec2(0.0, 30.0))], 100.0), Some(130.0));
    // The size it is given each frame does not move the drag's origin.
    assert_eq!(frame(&ctx, vec![Event::PointerMoved(start - vec2(0.0, 40.0))], 130.0), Some(140.0));
    // Past its bounds, it stops there.
    assert_eq!(frame(&ctx, vec![Event::PointerMoved(start - vec2(0.0, 1000.0))], 140.0), Some(300.0));
    assert_eq!(frame(&ctx, vec![Event::PointerMoved(start + vec2(0.0, 1000.0))], 300.0), Some(20.0));
    assert_eq!(frame(&ctx, vec![press(start + vec2(0.0, 1000.0), false)], 20.0), None);
}
