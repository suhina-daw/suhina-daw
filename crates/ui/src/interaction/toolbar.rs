// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the views' toolbars share: the icon buttons drawn in code, and the arrow on a toolbar's
//! menu button. The piano roll's toolbar, the Arrangement's tools and the detail area use them.

use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::kit::surface::Backgrounds as _;
use crate::kit::theme;

/// The toolbar's buttons: one shape each, drawn in code with our own strokes, as
/// [`crate::browser::browser_view`]'s icons are. `active` marks the note clipboard's buttons while there
/// is something to act on, and the quantise block while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconButton {
    Select,
    Draw,
    Paint,
    Erase,
    Slice,
    Mute,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Quantise,
    MidiTools,
    /// Headphones: drawn and moved notes sound once (› Playing the keys).
    Audition,
    /// The × at the right end: hides the editor.
    Hide,
    /// Fold: only the rows that have notes (or the scale's, while the clip has one).
    Fold,
    /// The clip's scale switch, the same as the clip panel's.
    Scale,
    /// Highlight scale: the scale's rows tinted and its keys marked.
    Highlight,
    /// Three dots above the keys: the piano roll's options (workflow, keys, stacked notes).
    More,
}

impl IconButton {
    /// What the button is called, for its tooltip and its accessible label.
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Draw => "Draw",
            Self::Paint => "Paint",
            Self::Erase => "Erase",
            Self::Slice => "Slice",
            Self::Mute => "Mute",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Duplicate => "Duplicate",
            Self::Quantise => "Quantise",
            Self::MidiTools => "MIDI tools",
            Self::Audition => "Audition",
            Self::Hide => "Hide editor",
            Self::Fold => "Fold",
            Self::Scale => "Scale",
            Self::Highlight => "Highlight scale",
            Self::More => "Piano roll settings",
        }
    }

    /// The id a theme selects it by (`#piano-draw`) and the name of its artwork (`piano.draw`).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Select => "piano-select",
            Self::Draw => "piano-draw",
            Self::Paint => "piano-paint",
            Self::Erase => "piano-erase",
            Self::Slice => "piano-slice",
            Self::Mute => "piano-mute",
            Self::Cut => "piano-cut",
            Self::Copy => "piano-copy",
            Self::Paste => "piano-paste",
            Self::Duplicate => "piano-duplicate",
            Self::Quantise => "piano-quantise",
            Self::MidiTools => "piano-midi-tools",
            Self::Audition => "piano-audition",
            Self::Hide => "piano-hide",
            Self::Fold => "piano-fold",
            Self::Scale => "piano-scale",
            Self::Highlight => "piano-highlight",
            Self::More => "piano-more",
        }
    }

    /// The action id whose key the tooltip shows ("Draw (2)"), when the button has one.
    pub fn action(self) -> Option<&'static str> {
        Some(match self {
            Self::Select => "pianoRoll.tool1",
            Self::Draw => "pianoRoll.tool2",
            Self::Paint => "pianoRoll.tool3",
            Self::Erase => "pianoRoll.tool4",
            Self::Slice => "pianoRoll.tool5",
            Self::Mute => "pianoRoll.tool6",
            Self::Cut => "selection.cut",
            Self::Copy => "selection.copy",
            Self::Paste => "selection.paste",
            Self::Duplicate => "selection.duplicate",
            Self::Hide => "view.editor",
            Self::Quantise
            | Self::MidiTools
            | Self::Audition
            | Self::Fold
            | Self::Scale
            | Self::Highlight
            | Self::More => return None,
        })
    }

    /// The button's icon, drawn from Suhina's own artwork (`assets/icons/piano-roll/`) in
    /// `currentColor`; `None` for the ones still drawn in code.
    pub(crate) fn svg(self) -> Option<&'static str> {
        Some(match self {
            Self::Select => include_str!("../../../../assets/icons/piano-roll/select.svg"),
            Self::Draw => include_str!("../../../../assets/icons/piano-roll/draw.svg"),
            Self::Paint => include_str!("../../../../assets/icons/piano-roll/paint.svg"),
            Self::Erase => include_str!("../../../../assets/icons/piano-roll/erase.svg"),
            Self::Slice => include_str!("../../../../assets/icons/piano-roll/slice.svg"),
            Self::Mute => include_str!("../../../../assets/icons/piano-roll/mute.svg"),
            Self::Cut => include_str!("../../../../assets/icons/piano-roll/cut.svg"),
            Self::Copy => include_str!("../../../../assets/icons/piano-roll/copy.svg"),
            Self::Paste => include_str!("../../../../assets/icons/piano-roll/paste.svg"),
            Self::Duplicate => include_str!("../../../../assets/icons/piano-roll/duplicate.svg"),
            Self::Quantise => include_str!("../../../../assets/icons/piano-roll/quantise.svg"),
            Self::MidiTools => include_str!("../../../../assets/icons/piano-roll/midi-tools.svg"),
            Self::Audition => include_str!("../../../../assets/icons/piano-roll/audition.svg"),
            Self::Hide | Self::Fold | Self::Scale | Self::Highlight | Self::More => return None,
        })
    }

    /// The shape in a 14 px box around `rect`'s centre, scaled with the toolbar (Toolbar size):
    /// the button's artwork in `color`, or, for the × and where the artwork does not raster, our
    /// own strokes, `scale` multiplying the shape and its strokes together.
    pub fn draw(self, painter: &egui::Painter, rect: Rect, color: Color32, scale: f32) {
        let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) } else { 1.0 };
        let c = rect.center();
        if let Some(svg) = self.svg()
            && crate::kit::skin::paint_icon(painter, Rect::from_center_size(c, Vec2::splat(14.0 * scale)), svg, color)
        {
            return;
        }
        let stroke = Stroke::new(1.1 * scale, color);
        let at = |x: f32, y: f32| c + Vec2::new(x * scale, y * scale);
        let size = |w: f32, h: f32| Vec2::new(w * scale, h * scale);
        let line = |a: (f32, f32), b: (f32, f32)| painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
        match self {
            // An arrow pointing up and to the left, its head filled.
            Self::Select => {
                line((-1.5, 6.0), (-1.5, -4.0));
                line((-1.5, -4.0), (-1.5, -6.0));
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-4.0, -5.0), at(5.0, 0.5), at(-4.0, 5.0)],
                    color,
                    Stroke::NONE,
                ));
            }
            // A pencil, lying along the diagonal.
            Self::Draw => {
                line((-6.0, 6.0), (3.5, -3.5));
                line((3.5, -3.5), (6.0, -1.0));
                line((6.0, -1.0), (-3.5, 8.5));
                line((-3.5, 8.5), (-6.0, 6.0));
                line((-6.0, 6.0), (-3.0, 6.5));
                line((-3.0, 6.5), (-3.5, 8.5));
            }
            // A brush: a short handle and a wide head.
            Self::Paint => {
                line((-6.5, 6.5), (-2.5, 2.5));
                line((-2.5, 2.5), (1.5, 6.5));
                line((1.5, 6.5), (6.5, 1.5));
                line((6.5, 1.5), (2.5, -2.5));
                line((2.5, -2.5), (-2.5, 2.5));
                line((2.5, -2.5), (6.0, -6.0));
            }
            // An eraser, tilted, with its rubbing end lighter.
            Self::Erase => {
                line((-6.0, 2.0), (2.0, -6.0));
                line((2.0, -6.0), (6.0, -2.0));
                line((6.0, -2.0), (-2.0, 6.0));
                line((-2.0, 6.0), (-6.0, 2.0));
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-6.0, 2.0), at(-2.0, 6.0), at(-0.5, 4.5), at(-4.5, 0.5)],
                    color.gamma_multiply(0.35),
                    Stroke::NONE,
                ));
                line((-1.0, 3.0), (3.0, 7.0));
            }
            // A blade: a handle with a straight edge under it.
            Self::Slice => {
                line((-5.5, -5.5), (1.5, 1.5));
                line((1.5, 1.5), (5.5, 1.5));
                line((5.5, 1.5), (5.5, 5.5));
                line((5.5, 5.5), (-5.5, 5.5));
                line((-5.5, 5.5), (-5.5, 1.5));
            }
            // A speaker with a cross beside it: mute.
            Self::Mute => {
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-6.5, -2.0), at(-3.0, -2.0), at(0.5, -6.0), at(0.5, 6.0), at(-3.0, 2.0), at(-6.5, 2.0)],
                    color.gamma_multiply(0.45),
                    stroke,
                ));
                line((3.0, -3.5), (6.5, 0.0));
                line((3.0, 3.5), (6.5, 0.0));
                line((3.0, 0.0), (6.5, -3.5));
                line((3.0, 0.0), (6.5, 3.5));
            }
            // A sheet with a tab: copy.
            Self::Copy => {
                let page = Rect::from_center_size(at(0.5, 1.0), size(9.0, 11.0));
                painter.rect_stroke(page, 0.0, stroke, StrokeKind::Inside);
                painter.line_segment([at(-2.5, -6.0), at(-2.5, -4.5)], stroke);
                painter.line_segment([at(-2.5, -6.0), at(3.5, -6.0)], stroke);
                painter.line_segment([at(3.5, -6.0), at(3.5, -4.5)], stroke);
            }
            // Two sheets, one behind the other: duplicate.
            Self::Duplicate => {
                painter.rect_stroke(
                    Rect::from_center_size(at(-1.5, -1.5), size(8.0, 8.0)),
                    0.0,
                    stroke,
                    StrokeKind::Inside,
                );
                let front = Rect::from_center_size(at(1.5, 1.5), size(8.0, 8.0));
                painter.fill_background(front, 0.0, theme::panel_background());
                painter.rect_stroke(front, 0.0, stroke, StrokeKind::Inside);
            }
            // Scissors: two rings and the blades crossing over them.
            Self::Cut => {
                painter.circle_stroke(at(-2.0, 4.0), 2.0 * scale, stroke);
                painter.circle_stroke(at(2.0, 4.0), 2.0 * scale, stroke);
                line((-3.0, 2.5), (4.0, -5.0));
                line((3.0, 2.5), (-4.0, -5.0));
            }
            // A clipboard with a page being laid onto it: paste.
            Self::Paste => {
                let board = Rect::from_center_size(at(0.0, 1.0), size(10.0, 12.0));
                painter.rect_stroke(board, 0.0, stroke, StrokeKind::Inside);
                painter.rect_filled(Rect::from_center_size(at(0.0, -5.5), size(5.0, 2.5)), 0.0, color);
                line((-3.0, 0.0), (3.0, 0.0));
                line((-3.0, 3.0), (3.0, 3.0));
            }
            // Notes snapping onto a line: the quantise button.
            Self::Quantise => {
                line((-6.0, 5.5), (6.0, 5.5));
                for (x, y) in [(-3.5, -1.0), (0.0, -4.0), (3.5, -1.5)] {
                    painter.rect_filled(Rect::from_center_size(at(x, y), size(4.0, 3.0)), 0.0, color);
                    line((x, y + 1.5), (x, 5.5));
                }
            }
            // A cross: hides the editor.
            Self::Hide => {
                line((-4.0, -4.0), (4.0, 4.0));
                line((-4.0, 4.0), (4.0, -4.0));
            }
            // Headphones: a band over the head and a cup at each end.
            Self::Audition => {
                let band: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let angle = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                        at(5.0 * angle.cos(), 1.5 + 5.5 * angle.sin())
                    })
                    .collect();
                painter.add(egui::Shape::line(band, stroke));
                painter.rect_filled(Rect::from_center_size(at(-5.0, 3.5), size(3.0, 5.0)), 0.0, color);
                painter.rect_filled(Rect::from_center_size(at(5.0, 3.5), size(3.0, 5.0)), 0.0, color);
            }
            // Two rows with arrows pointing in between them: fewer rows.
            Self::Fold => {
                line((-6.0, -6.0), (6.0, -6.0));
                line((-6.0, 6.0), (6.0, 6.0));
                line((-3.0, -3.5), (0.0, -1.0));
                line((0.0, -1.0), (3.0, -3.5));
                line((-3.0, 3.5), (0.0, 1.0));
                line((0.0, 1.0), (3.0, 3.5));
            }
            // A little keyboard: three white keys and two black ones.
            Self::Scale => {
                painter.rect_stroke(
                    Rect::from_center_size(at(0.0, 0.0), size(12.0, 10.0)),
                    1.0,
                    stroke,
                    StrokeKind::Inside,
                );
                for x in [-2.0, 2.0] {
                    line((x, -5.0), (x, 5.0));
                    painter.rect_filled(Rect::from_center_size(at(x, -1.5), size(2.5, 5.0)), 0.0, color);
                }
            }
            // A highlighter pen, tilted, over the band it leaves: the scale's rows tinted.
            Self::Highlight => {
                painter.rect_filled(Rect::from_min_max(at(-6.5, 3.5), at(6.5, 6.5)), 1.0, color.gamma_multiply(0.5));
                line((-3.0, 1.5), (3.0, -4.5));
                line((3.0, -4.5), (5.5, -2.0));
                line((5.5, -2.0), (-0.5, 4.0));
                line((-0.5, 4.0), (-3.0, 1.5));
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-3.0, 1.5), at(-0.5, 4.0), at(-3.5, 4.5)],
                    color,
                    Stroke::NONE,
                ));
            }
            // Three dots: more.
            Self::More => {
                for x in [-4.5, 0.0, 4.5] {
                    painter.circle_filled(at(x, 0.0), 1.3 * scale, color);
                }
            }
            // Three horizontal sliders: the MIDI tools column's toggle.
            Self::MidiTools => {
                for (y, knob) in [(-4.0, -2.0), (0.0, 3.0), (4.0, -1.0)] {
                    line((-6.0, y), (6.0, y));
                    painter.rect_filled(Rect::from_center_size(at(knob, y), size(3.0, 5.0)), 0.0, color);
                }
            }
        }
    }
}

/// The small triangle at the right end of a toolbar menu's button, drawn as a shape: the UI font
/// has no glyph for it.
pub fn menu_arrow(ui: &Ui, button: Rect, color: Color32) {
    let c = Pos2::new(button.right() - 7.0, button.center().y);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, -1.5), c + Vec2::new(0.0, 2.0)],
        color,
        Stroke::NONE,
    ));
}
