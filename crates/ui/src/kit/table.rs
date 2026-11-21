// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the dialogs' tables share (the plug-in manager, the missing files panel): the edge
//! between two headings that is dragged to resize a column (a double-click fits it), and cells
//! that cut their text to their width, a path in its middle, with the whole text on hover.

use egui::{Color32, FontId, Id, Painter, Pos2, Rect, Sense, Ui};

use super::widgets;

/// What happened to a column's edge this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Edge {
    /// How far it was dragged (0 when it was not).
    pub delta: f32,
    /// It was double-clicked: the column is fitted to what it holds.
    pub fit: bool,
    /// A drag was let go: the width is kept.
    pub released: bool,
}

/// The edge at the right of the column `name` ends in, at `x` across `header`, seen through
/// `strip` (only the part of it the strip shows takes the pointer): the resize pointer over it,
/// a line in `line`, and in `accent` while it is pointed at or dragged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resize_edge(
    ui: &Ui,
    painter: &Painter,
    header: Rect,
    strip: Rect,
    x: f32,
    id: Id,
    name: &str,
    (line, accent): (Color32, Color32),
) -> Edge {
    let edge = Rect::from_min_max(Pos2::new(x - 4.0, header.top()), Pos2::new(x + 4.0, header.bottom()));
    let edge = edge.intersect(strip);
    if edge.width() < 4.0 || edge.height() <= 0.0 {
        return Edge::default();
    }
    let response = ui.interact(edge, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeColumn);
    let label = format!("Resize the {name} column");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, &label));
    let lit = response.hovered() || response.dragged();
    let span = egui::Rangef::new(header.top() + 5.0, header.bottom() - 5.0);
    painter.vline(x - 0.5, span, egui::Stroke::new(if lit { 2.0 } else { 1.0 }, if lit { accent } else { line }));
    Edge {
        delta: if response.dragged() { response.drag_delta().x } else { 0.0 },
        fit: response.double_clicked(),
        released: response.drag_stopped(),
    }
}

/// Draws `text` in `cell`, vertically centered, cut with an ellipsis where it is wider. While
/// the pointer is over the cell, `whole` (or the text itself, where it was cut) shows in a
/// tooltip. Returns whether the text was cut.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cell(
    ui: &Ui,
    painter: &Painter,
    cell: Rect,
    id: Id,
    text: &str,
    font: FontId,
    color: Color32,
    whole: Option<&str>,
) -> bool {
    if cell.width() <= 0.0 || text.is_empty() {
        return false;
    }
    let galley = widgets::fitted(painter, text, font, color, cell.width());
    let elided = galley.elided;
    let at = Pos2::new(cell.left(), cell.center().y - galley.size().y / 2.0);
    widgets::paint_text(painter, at, galley, color);
    tip(ui, cell, id, whole.or(elided.then_some(text)));
    elided
}

/// [`cell`] for a path: cut in its middle, so its start (the drive, the first folder) and its
/// end (the file's name) both stay. The whole path shows on hover where it was cut.
#[allow(clippy::too_many_arguments)]
pub(crate) fn path_cell(
    ui: &Ui,
    painter: &Painter,
    cell: Rect,
    id: Id,
    path: &str,
    font: FontId,
    color: Color32,
) -> bool {
    if cell.width() <= 0.0 || path.is_empty() {
        return false;
    }
    let (text, cut) = middle_cut(painter, path, &font, cell.width());
    let galley = painter.layout_no_wrap(text, font, color);
    let at = Pos2::new(cell.left(), cell.center().y - galley.size().y / 2.0);
    widgets::paint_text(painter, at, galley, color);
    tip(ui, cell, id, cut.then_some(path));
    cut
}

/// The tooltip of a cell, made only while the pointer is over it.
fn tip(ui: &Ui, cell: Rect, id: Id, text: Option<&str>) {
    if let Some(text) = text
        && ui.rect_contains_pointer(cell)
    {
        let _tip = ui.interact(cell, id, Sense::hover()).on_hover_text(text);
    }
}

/// The most characters of a text [`middle_cut`] lays out: far more than a cell shows.
const MAX_SHOWN: usize = 512;

/// `text` cut in its middle to fit `width` in `font`: a third of what is kept from its start,
/// the rest from its end, an ellipsis between. Returns the text and whether it was cut.
pub(crate) fn middle_cut(painter: &Painter, text: &str, font: &FontId, width: f32) -> (String, bool) {
    let measure = |s: &str| painter.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    // A text far longer than any cell (a path a project names is not bounded) is cut to its
    // ends before anything is laid out or collected.
    let count = text.chars().count();
    let capped: String;
    let text = if count > MAX_SHOWN {
        let head = MAX_SHOWN / 3;
        let tail = MAX_SHOWN - head;
        let start: String = text.chars().take(head).collect();
        let end: String = text.chars().skip(count.saturating_sub(tail)).collect();
        capped = format!("{start}…{end}");
        capped.as_str()
    } else {
        text
    };
    if measure(text) <= width {
        return (text.to_string(), count > MAX_SHOWN);
    }
    let chars: Vec<char> = text.chars().collect();
    let joined = |kept: usize| {
        let head = kept / 3;
        let tail = kept - head;
        let start: String = chars.iter().take(head).collect();
        let end: String = chars.iter().skip(chars.len().saturating_sub(tail)).collect();
        format!("{start}…{end}")
    };
    // The most characters kept that still fit, found by halving.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if measure(&joined(mid)) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    (joined(low), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_cut_in_its_middle_and_keeps_its_ends() {
        let ctx = egui::Context::default();
        let mut cut = (String::new(), false);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let font = FontId::proportional(11.0);
            let path = "E:/Sample Library/Projects/Deep House 2/Downloads/Cloud Sounds/Drums/kick.wav";
            assert_eq!(middle_cut(&painter, "kick.wav", &font, 400.0), ("kick.wav".to_string(), false));
            cut = middle_cut(&painter, path, &font, 160.0);
            let width = painter.layout_no_wrap(cut.0.clone(), font.clone(), Color32::WHITE).size().x;
            assert!(width <= 160.0, "{} is {width} wide", cut.0);
        });
        // The frame's font atlas is never uploaded here; epaint asserts in debug builds when it is dropped unhandled.
        output.textures_delta.clear();
        assert!(cut.1);
        assert!(cut.0.starts_with("E:/"), "{}", cut.0);
        assert!(cut.0.ends_with("kick.wav"), "{}", cut.0);
        assert!(cut.0.contains('…'));
    }

    /// A text far longer than a cell is cut to its ends before it is laid out.
    #[test]
    fn an_endless_text_is_capped_before_it_is_measured() {
        let ctx = egui::Context::default();
        let long = format!("C:/{}/kick.wav", "a".repeat(1 << 20));
        let mut cut = (String::new(), false);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cut = middle_cut(ui.painter(), &long, &FontId::proportional(11.0), 10_000.0);
        });
        output.textures_delta.clear();
        assert!(cut.1);
        assert!(cut.0.chars().count() <= MAX_SHOWN + 1, "{}", cut.0.chars().count());
        assert!(cut.0.starts_with("C:/") && cut.0.ends_with("kick.wav"));
    }
}
