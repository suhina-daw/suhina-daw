// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The strip over the piano roll while several MIDI clips are picked (› Several clips picked): each
//! clip as a bar in its track's colour where it lies in time, a click giving it the focus, and the
//! Focus switch that keeps the focus on the clip picked first.

use super::*;

/// How strongly a clip's bar shows its colour while another has the focus.
const OTHER_BAR: f32 = 0.5;

/// Draws the strip in `row`, the whole width of the piano roll, and takes its clicks.
pub(super) fn show(
    ui: &mut Ui,
    row: Rect,
    layout: &Layout,
    clip: &PianoClip,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    let painter = ui.painter_at(row);
    let style = part_style("piano-clip-strip", &[]);
    painter.rect_filled(
        row,
        0.0,
        style.background.unwrap_or_else(|| crate::kit::style_class::surface_or(theme::panel_background())),
    );

    // The Focus switch, over the keys.
    let switch = Rect::from_min_max(row.min, Pos2::new(layout.grid.left(), row.bottom())).shrink(1.0);
    let response = ui.interact(switch, Id::unique("piano-roll-focus-first"), Sense::click());
    if response.clicked() {
        state.focus_first = !state.focus_first;
    }
    let on = state.focus_first;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Focus on the first clip"));
    if on {
        painter.rect_filled(switch, 0.0, theme::accent());
    }
    painter.text(
        switch.center(),
        Align2::CENTER_CENTER,
        "Focus",
        theme::mono_font(theme::TEXT_TINY),
        if on { theme::text_inverse() } else { theme::text_secondary() },
    );

    // The clips, over the grid.
    let bars = Rect::from_x_y_ranges(layout.grid.x_range(), row.y_range());
    let painter = ui.painter_at(bars);
    let height = row.height() / clip.ghosts.strip_rows().max(1) as f32;
    for (index, bar) in clip.ghosts.picked.iter().enumerate() {
        let rect = Rect::from_x_y_ranges(
            layout.x_of(bar.start)..=layout.x_of(bar.start + bar.length).max(layout.x_of(bar.start) + 2.0),
            row.top() + bar.row as f32 * height..=row.top() + (bar.row + 1) as f32 * height,
        )
        .shrink2(Vec2::new(0.0, 1.0));
        let color = if bar.edited { bar.color } else { bar.color.gamma_multiply(OTHER_BAR) };
        painter.rect_filled(rect, 0.0, color);
        let name_color = if bar.edited { theme::text_inverse() } else { theme::text_primary() };
        painter.with_clip_rect(rect.intersect(bars)).text(
            Pos2::new(rect.left().max(bars.left()) + 3.0, rect.center().y),
            Align2::LEFT_CENTER,
            &bar.name,
            theme::mono_font(theme::TEXT_TINY),
            name_color,
        );
        let visible = rect.intersect(bars);
        if !visible.is_positive() {
            continue;
        }
        let response = ui.interact(visible, Id::unique(("piano-roll-clip-bar", index)), Sense::click());
        response.widget_info(|| {
            let label = format!("Clip {} in the piano roll", bar.name);
            egui::WidgetInfo::selected(egui::Role::Button, true, bar.edited, label)
        });
        if response.clicked() && !bar.edited {
            actions.extend(focus_action(bar.clip));
        }
    }
}

/// What gives `clip`, one of the clips picked, the piano roll's focus.
pub(super) fn focus_action(clip: ClipRef) -> Option<Action> {
    let ClipRef::Arrangement(track, id) = clip else { return None };
    Some(Action::new("view.focusClip", json!({ "track": track.0, "clip": id.0 })))
}
