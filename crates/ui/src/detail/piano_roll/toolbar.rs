// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's toolbar: its tool buttons, the Rows group, and the grid menu; and the
//! workflow entries of the options menu above the keys.

use super::*;

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
    pub(super) fn name(self) -> &'static str {
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
    pub(super) fn action(self) -> Option<&'static str> {
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
            Self::Select => include_str!("../../../../../assets/icons/piano-roll/select.svg"),
            Self::Draw => include_str!("../../../../../assets/icons/piano-roll/draw.svg"),
            Self::Paint => include_str!("../../../../../assets/icons/piano-roll/paint.svg"),
            Self::Erase => include_str!("../../../../../assets/icons/piano-roll/erase.svg"),
            Self::Slice => include_str!("../../../../../assets/icons/piano-roll/slice.svg"),
            Self::Mute => include_str!("../../../../../assets/icons/piano-roll/mute.svg"),
            Self::Cut => include_str!("../../../../../assets/icons/piano-roll/cut.svg"),
            Self::Copy => include_str!("../../../../../assets/icons/piano-roll/copy.svg"),
            Self::Paste => include_str!("../../../../../assets/icons/piano-roll/paste.svg"),
            Self::Duplicate => include_str!("../../../../../assets/icons/piano-roll/duplicate.svg"),
            Self::Quantise => include_str!("../../../../../assets/icons/piano-roll/quantise.svg"),
            Self::MidiTools => include_str!("../../../../../assets/icons/piano-roll/midi-tools.svg"),
            Self::Audition => include_str!("../../../../../assets/icons/piano-roll/audition.svg"),
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

/// What the toolbar's buttons draw from: the clip they act on, the keys they show, how many notes
/// a paste would add, and the Toolbar size they are drawn at.
#[derive(Clone, Copy)]
pub(super) struct ToolbarCx<'a> {
    pub(super) clip: &'a PianoClip,
    pub(super) keymap: &'a crate::settings::keymap::Keymap,
    pub(super) copied: Option<usize>,
    pub(super) scale: f32,
}

/// The toolbar along the top of the piano roll (› Layout): the tools, the note clipboard, quantise
/// and the MIDI tools at the left; the Rows group (Fold, Scale, Highlight), the ghost menu, the
/// grid value and the × at the right. With less room it gives way in steps, never hiding a part
/// outright: first the Grid caption, then the Rows words (their icons stay), then the clipboard
/// folds into an Edit menu, then the ghost menu loses its word. A button that runs something adds
/// to `actions`.
pub(super) fn toolbar(
    ui: &mut Ui,
    rect: Rect,
    cx: ToolbarCx<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    let ToolbarCx { clip, keymap, copied, scale } = cx;
    if rect.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.fill_background(rect, 0.0, theme::panel_background());
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let row = rect.shrink2(Vec2::new(4.0, 3.0));
    // The icon box, the gap and the strokes scale with Toolbar size; the box never outgrows the
    // band, so a small detail area shows the icons and not a clipped toolbar.
    let icon = (TOOL_ICON * scale).min(row.height());
    let gap = TOOL_GAP * scale;
    let side = row.height().min(icon);
    // `.piano-tool { width; height }` sizes the buttons, so stretched artwork has room. Unstyled,
    // a button is wider than its icon by a little room at each side, so the icons do not crowd.
    let size = {
        let _style = crate::kit::style_class::enter("piano-tool");
        let own = crate::kit::style_class::computed();
        Vec2::new(
            own.as_ref().and_then(|c| c.width).map_or(side + BUTTON_ROOM * scale, |w| w.clamp(8.0, 96.0)),
            own.as_ref().and_then(|c| c.height).map_or(side, |h| h.clamp(8.0, row.height().max(8.0))),
        )
    };
    let step = layout_step(ui, row, size, gap, side);
    let paint = ButtonPaint { ui, painter: &painter, keymap, scale };
    let x = std::cell::Cell::new(row.left() + TOOLBAR_INSET);
    let place = || {
        let area = Rect::from_min_size(Pos2::new(x.get(), row.top() + (row.height() - size.y) / 2.0), size);
        x.set(area.right() + gap);
        area
    };
    let divider = || {
        x.set(x.get() + 2.0);
        if x.get() < row.right() {
            painter.vline(x.get(), row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
        }
        x.set(x.get() + 5.0);
    };
    for (tool, button) in [
        (Tool::Select, IconButton::Select),
        (Tool::Draw, IconButton::Draw),
        (Tool::Paint, IconButton::Paint),
        (Tool::Erase, IconButton::Erase),
        (Tool::Slice, IconButton::Slice),
        (Tool::Mute, IconButton::Mute),
    ] {
        if paint.place(place(), button, tool == state.tool, true) {
            state.tool = tool;
        }
    }
    divider();
    // The note clipboard: four buttons, or one Edit menu when room is short.
    let selected = !state.acting().is_empty();
    let clipboard = [IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate];
    if step >= Step::Edit {
        let area = Rect::from_min_size(
            Pos2::new(x.get(), row.top() + (row.height() - size.y) / 2.0),
            size + Vec2::new(10.0, 0.0),
        );
        x.set(area.right() + gap);
        if let Some(button) = edit_menu(&paint, area, selected) {
            actions.extend(state.clipboard_action(button, clip, copied));
        }
    } else {
        for button in clipboard {
            let active = matches!(button, IconButton::Paste) || selected;
            if paint.place(place(), button, false, active) {
                actions.extend(state.clipboard_action(button, clip, copied));
            }
        }
    }
    divider();
    // Quantise (which shows Quantise in the clip panel's tools) and the tools part's own toggle.
    if paint.place(place(), IconButton::Quantise, state.quantize_open(), true) {
        let open = !state.quantize_open();
        state.set_quantize_open(open);
    }
    if paint.place(place(), IconButton::MidiTools, state.tools_open, true) {
        state.tools_open = !state.tools_open;
    }
    let left_end = x.get();

    // From the right end: the ×, the grid value and its caption, a divider, the ghost menu and
    // the Rows group.
    let hide =
        Rect::from_min_size(Pos2::new(row.right() - side, row.top() + (row.height() - side) / 2.0), Vec2::splat(side));
    if hide.left() >= left_end && paint.place(hide, IconButton::Hide, false, true) {
        state.hide_requested = true;
    }
    let menu = Rect::from_min_max(
        Pos2::new(hide.left() - gap - 2.0 - GRID_MENU, row.top()),
        Pos2::new(hide.left() - gap - 2.0, row.bottom()),
    );
    if menu.left() < left_end + 6.0 {
        return;
    }
    if let Some(picked) = grid_menu(ui, menu, clip, keymap) {
        state.grid_picked = Some(picked);
    }
    let mut right = menu.left();
    if step == Step::Full {
        let label = Rect::from_min_max(Pos2::new(right - 34.0, row.top()), Pos2::new(right - 4.0, row.bottom()));
        widgets::text_in(
            &painter,
            label,
            Align2::RIGHT_CENTER,
            "Grid",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        right -= 40.0;
    }
    if step == Step::Squeezed {
        return;
    }
    painter.vline(right - 5.0, row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
    right -= 10.0;
    let words = step < Step::Icons;
    let named = step < Step::Arrows;
    let ghost_width = if named { ghost_view::MENU_WIDTH } else { MENU_ARROW };
    let ghosts = Rect::from_min_max(Pos2::new(right - ghost_width, row.top()), Pos2::new(right, row.bottom()));
    ghost_view::menu(ui, ghosts, &clip.ghosts, &mut state.ghost_source, named);
    right = ghosts.left() - 4.0;
    let workflow_width = if named { WORKFLOW_MENU } else { MENU_ARROW };
    let workflow = Rect::from_min_max(Pos2::new(right - workflow_width, row.top()), Pos2::new(right, row.bottom()));
    workflow_menu(ui, workflow, keymap, state, named);
    right = workflow.left() - 6.0;
    rows_group(ui, &painter, right, row, side, words, cx, state, actions);
}

/// How far the toolbar gives way this frame (› Layout): each step leaves out one more thing the
/// row has no room for, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    /// Everything, with the grid value's caption.
    Full,
    /// Without the Grid caption.
    NoCaption,
    /// The Rows group as icons.
    Icons,
    /// The clipboard in one Edit menu.
    Edit,
    /// The workflow and ghost menus as their arrows.
    Arrows,
    /// Not even that fits: only the tools, the grid value and the ×.
    Squeezed,
}

/// The room a toolbar button has beside its icon, both sides together, at Normal size, in points.
const BUTTON_ROOM: f32 = 8.0;

/// The width of a toolbar menu as its arrow alone, in points.
const MENU_ARROW: f32 = 22.0;

/// How wide the toolbar's workflow menu is with the mode's name, in points.
const WORKFLOW_MENU: f32 = 96.0;

/// The words of the Rows group: Fold, Scale (the clip's scale switch) and Highlight.
const ROWS: [(IconButton, &str); 3] =
    [(IconButton::Fold, "Fold"), (IconButton::Scale, "Scale"), (IconButton::Highlight, "Highlight")];

/// The width of one of the Rows group's toggles: its icon, and its word when `word`.
fn row_toggle_width(ui: &Ui, side: f32, word: Option<&str>) -> f32 {
    match word {
        Some(word) => side + 10.0 + widgets::text_width(ui, word, theme::ui_font(theme::TEXT_SMALL)),
        None => side + 6.0,
    }
}

/// The least the toolbar gives way to fit `row` (› Layout).
fn layout_step(ui: &Ui, row: Rect, size: Vec2, gap: f32, side: f32) -> Step {
    let button = size.x + gap;
    let rows = |words: bool| -> f32 {
        ROWS.iter().map(|(_, word)| row_toggle_width(ui, side, words.then_some(*word)) + 2.0).sum::<f32>() + 4.0
    };
    let need = |step: Step| {
        let clipboard = if step >= Step::Edit { size.x + 10.0 + gap } else { 4.0 * button };
        let caption = if step == Step::Full { 40.0 } else { 0.0 };
        let menus = if step >= Step::Arrows { 2.0 * MENU_ARROW } else { ghost_view::MENU_WIDTH + WORKFLOW_MENU };
        TOOLBAR_INSET
            + 8.0 * button
            + clipboard
            + 14.0
            + 8.0
            + rows(step < Step::Icons)
            + 6.0
            + menus
            + 4.0
            + 10.0
            + caption
            + GRID_MENU
            + gap
            + 2.0
            + side
    };
    [Step::Full, Step::NoCaption, Step::Icons, Step::Edit, Step::Arrows]
        .into_iter()
        .find(|step| need(*step) <= row.width())
        .unwrap_or(Step::Squeezed)
}

/// The Rows group, its right end at `right`, framed: Fold, Scale and Highlight, each lit while
/// on, as an icon and a word or, when `words` is off, the icon alone (its tooltip names it).
/// Fold shows the rows with notes, or the scale's while the clip has one; Scale is the clip's
/// scale switch, the same as the clip panel's; Highlight tints the scale's rows (› Scale and key).
#[allow(clippy::too_many_arguments)]
fn rows_group(
    ui: &mut Ui,
    painter: &egui::Painter,
    right: f32,
    row: Rect,
    side: f32,
    words: bool,
    cx: ToolbarCx<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    let clip = cx.clip;
    let widths: Vec<f32> = ROWS.iter().map(|(_, word)| row_toggle_width(ui, side, words.then_some(*word))).collect();
    let total = widths.iter().map(|w| w + 2.0).sum::<f32>() + 2.0;
    let group = Rect::from_min_max(Pos2::new(right - total, row.top()), Pos2::new(right, row.bottom()));
    painter.rect_stroke(group, 4.0, theme::hairline(theme::border_strong()), StrokeKind::Inside);
    let folded = state.fold != Fold::Off;
    let hints = [
        "Fold: shows only the rows that have notes",
        "Scale: turns the clip's scale on, showing only its rows and those with notes, or off; the clip panel's switch is the same",
        "Highlight scale: tints the rows of the clip's scale and marks its keys",
    ];
    let ons = [folded, clip.scale.is_some(), state.highlight_scale];
    let mut x = group.left() + 2.0;
    for (index, ((button, word), width)) in ROWS.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x, group.top() + 1.0), Vec2::new(*width, group.height() - 2.0));
        x = rect.right() + 2.0;
        let on = ons.get(index).copied().unwrap_or(false);
        if !row_toggle(
            ui,
            painter,
            rect,
            (*button, words.then_some(*word)),
            on,
            hints.get(index).copied().unwrap_or(""),
            cx.scale,
        ) {
            continue;
        }
        match button {
            IconButton::Fold => state.fold = if folded { Fold::Off } else { Fold::Notes },
            IconButton::Scale => {
                let mut params = crate::detail::piano_roll::scale_switch_params(ui.ctx(), &clip.address, clip.scale);
                if let (Some(params), Some(address)) = (params.as_object_mut(), clip.address.as_object()) {
                    params.extend(address.clone());
                }
                actions.push(Action::new("clip.setScale", params));
            }
            _ => {
                state.highlight_scale = !on;
                state.view_picked = Some(super::ViewPick::HighlightScale(!on));
            }
        }
        if matches!(button, IconButton::Fold | IconButton::Scale) {
            // Another set of rows: the notes are centred again in them.
            state.scrolled_by_hand = false;
            state.centred_for = 0.0;
        }
    }
}

/// One of the Rows group's toggles: lit while on, its icon and, given one, its word; its accessible
/// label is the button's name, its tooltip `hint`. Returns whether it was clicked.
fn row_toggle(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    (button, word): (IconButton, Option<&str>),
    on: bool,
    hint: &str,
    scale: f32,
) -> bool {
    let response = widgets::button_area(ui, rect, Id::unique(("piano-roll-rows", button.name())), button.name());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, button.name()));
    // Lit as a selected word is (the selection's fill and ink), as the toolbar's words were.
    let visuals = ui.visuals();
    let (fill, ink) = if on {
        (visuals.selection.bg_fill, visuals.selection.stroke.color)
    } else if response.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, theme::text_primary())
    } else {
        (Color32::TRANSPARENT, theme::text_secondary())
    };
    painter.rect_filled(rect, 3.0, fill);
    let icon = Rect::from_min_size(rect.min + Vec2::new(3.0, 0.0), Vec2::new(rect.height(), rect.height()));
    button.draw(painter, icon, ink, scale * 0.9);
    if let Some(word) = word {
        let text = Rect::from_min_max(Pos2::new(icon.right() + 2.0, rect.top()), rect.max);
        widgets::text_in(painter, text, Align2::LEFT_CENTER, word, theme::ui_font(theme::TEXT_SMALL), ink);
    }
    let clicked = response.clicked();
    response.on_hover_text(hint);
    clicked
}

/// The note clipboard as one menu, when the toolbar is short of room: the Copy icon and an arrow,
/// its list Cut, Copy, Paste and Duplicate (greyed out with nothing to act on, Paste aside).
/// Returns the button picked.
fn edit_menu(paint: &ButtonPaint<'_>, area: Rect, selected: bool) -> Option<IconButton> {
    let ui = paint.ui;
    let response = widgets::button_area(ui, area, Id::unique("piano-roll-edit-menu"), "Edit");
    let fill = crate::kit::motion::mix(Color32::TRANSPARENT, theme::control_hover(), widgets::hover(ui, &response));
    let ink = theme::text_secondary();
    paint.painter.rect_filled(area, 3.0, fill);
    let icon = Rect::from_min_size(area.min, Vec2::splat(area.height()));
    IconButton::Copy.draw(paint.painter, icon, ink, paint.scale);
    menu_arrow(ui, area, ink);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        for button in [IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate] {
            let enabled = matches!(button, IconButton::Paste) || selected;
            let text = egui::RichText::new(icon_hint(button, paint.keymap)).font(theme::ui_font(theme::TEXT_SMALL));
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                picked = Some(button);
                ui.close();
            }
        }
    });
    response.on_hover_text("Edit: cut, copy, paste, duplicate");
    picked
}

/// How wide the toolbar's grid menu button is, in points.
pub(super) const GRID_MENU: f32 = 76.0;

/// The toolbar's grid menu (› Grid and snapping): its button shows the grid value; its menu holds
/// Snap to grid, the adaptive widths, the fixed values, Narrow grid, Widen grid and Triplet grid,
/// each with its key, the chosen ones ticked. Returns the grid picked this frame.
pub(super) fn grid_menu(
    ui: &mut Ui,
    rect: Rect,
    clip: &PianoClip,
    keymap: &crate::settings::keymap::Keymap,
) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let shown = grid.label();
    let mut picked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = egui::RichText::new(format!("{shown}   ")).font(mono.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            let keys = grid_key_labels(keymap, &PIANO_GRID_KEYS);
            picked = grid_entries(ui, grid, &keys, &|_| true);
        });
        let label = format!("Grid value {shown}");
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        menu.response.on_hover_text("The grid: snapping, adaptive or fixed lines, triplets");
    });
    picked.filter(|p| *p != grid)
}

/// The piano roll's grid keys: snap, narrower, wider, triplets.
pub(crate) const PIANO_GRID_KEYS: [&str; 4] =
    ["pianoRoll.gridSnap", "pianoRoll.gridNarrow", "pianoRoll.gridWiden", "pianoRoll.gridTriplet"];

/// A grid menu's entries, all in the menu itself, compact: Snap to grid; the adaptive widths in
/// rows of three and the fixed values in two rows of five under small headings; Narrow grid,
/// Widen grid and Triplet grid. `keys` are the key labels of snap, narrow, widen and triplet
/// (empty for none). The chosen ones carry a tick; an entry `enabled` refuses is greyed out. The
/// piano roll's toolbar, the Arrangement's toolbar and its menus show the same entries. Returns
/// the grid picked.
pub(crate) fn grid_entries(
    ui: &mut Ui,
    grid: PianoGrid,
    keys: &[String; 4],
    enabled: &dyn Fn(&PianoGrid) -> bool,
) -> Option<PianoGrid> {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let picked = std::cell::Cell::new(None);
    ui.set_min_width(220.0);
    // A tick before a chosen entry, the room for one before the others, so names line up.
    let ticked = |on: bool, name: &str| format!("{} {name}", if on { "\u{2713}" } else { "\u{2002}" });
    // One row: its text, its key at the right, a tick when `on` says so.
    let row = |ui: &mut Ui, name: &str, key: &str, on: Option<bool>, result: PianoGrid| {
        let allowed = enabled(&result);
        let text = egui::RichText::new(ticked(on.unwrap_or(false), name)).font(font.clone());
        let button = egui::Button::new(text)
            .frame(false)
            .shortcut_text(egui::RichText::new(key).font(mono.clone()).color(theme::text_hint()));
        let response = ui.add_enabled(allowed, button);
        let label = if key.is_empty() { name.to_string() } else { format!("{name} ({key})") };
        response.widget_info(|| match on {
            Some(on) => egui::WidgetInfo::selected(egui::Role::Button, allowed, on, &label),
            None => egui::WidgetInfo::labeled(egui::Role::Button, allowed, &label),
        });
        if response.clicked() {
            picked.set(Some(result));
            ui.close();
        }
    };
    // A heading, then `cells` in rows of `columns`: a short name, ticked when chosen, the grid
    // it picks and its accessible label.
    let table = |ui: &mut Ui, title: &str, columns: usize, cells: Vec<(String, bool, PianoGrid, String)>| {
        ui.label(egui::RichText::new(title).font(font.clone()).color(theme::text_hint()));
        egui::Grid::new(("grid-menu", title)).spacing(egui::vec2(10.0, 2.0)).show(ui, |ui| {
            for (index, (name, on, result, label)) in cells.into_iter().enumerate() {
                let allowed = enabled(&result);
                let text = egui::RichText::new(ticked(on, &name)).font(font.clone());
                let response = ui.add_enabled(allowed, egui::Button::new(text).frame(false));
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, allowed, on, &label));
                if response.clicked() {
                    picked.set(Some(result));
                    ui.close();
                }
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    };
    row(ui, "Snap to grid", &keys[0], Some(grid.snap), grid.toggle_snap());
    ui.separator();
    let widths = Width::ALL
        .into_iter()
        .map(|w| {
            let result = PianoGrid { mode: GridMode::Adaptive(w), ..grid };
            let label = format!("Adaptive {}", w.name().to_lowercase());
            (w.name().to_string(), grid.mode == GridMode::Adaptive(w), result, label)
        })
        .collect();
    table(ui, "Adaptive grid:", 3, widths);
    // Picking a value turns snapping back on (the control bar's none is snap off).
    let values = FIXED_VALUES
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let result = PianoGrid { mode: GridMode::Fixed(i), snap: true, ..grid };
            let name = v.replace(" bars", " Bars").replace(" bar", " Bar");
            (name, grid.mode == GridMode::Fixed(i), result, v.to_string())
        })
        .collect();
    table(ui, "Fixed grid:", 5, values);
    ui.separator();
    row(ui, "Narrow grid", &keys[1], None, grid.narrower());
    row(ui, "Widen grid", &keys[2], None, grid.wider());
    ui.separator();
    row(ui, "Triplet grid", &keys[3], Some(grid.triplet), grid.toggle_triplet());
    picked.get().filter(|p| *p != grid)
}

/// The key labels of `actions` (snap, narrow, widen, triplet) in `keymap`, empty for none.
pub(crate) fn grid_key_labels(keymap: &crate::settings::keymap::Keymap, actions: &[&str; 4]) -> [String; 4] {
    actions.map(|a| keymap.label(a).unwrap_or_default())
}

/// The small triangle at the right end of a toolbar menu's button, drawn as a shape: the UI font
/// has no glyph for it.
pub(crate) fn menu_arrow(ui: &Ui, button: Rect, color: Color32) {
    let c = Pos2::new(button.right() - 7.0, button.center().y);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, -1.5), c + Vec2::new(0.0, 2.0)],
        color,
        Stroke::NONE,
    ));
}

/// The toolbar's workflow menu (› Workflow modes): its button shows the mode's name (its arrow
/// alone when the toolbar is short of room), its menu the workflow entries.
fn workflow_menu(
    ui: &mut Ui,
    rect: Rect,
    keymap: &crate::settings::keymap::Keymap,
    state: &mut PianoRollState,
    named: bool,
) {
    let mode = keymap.preset();
    let font = theme::ui_font(theme::TEXT_SMALL);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = if named { format!("{}   ", mode.name()) } else { "   ".to_string() };
        let menu = ui.menu_button(egui::RichText::new(text).font(font).color(theme::text_value()), |ui| {
            workflow_entries(ui, keymap, state);
        });
        menu_arrow(ui, menu.response.rect, theme::text_value());
        let label = format!("Workflow {}", mode.name());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        menu.response.on_hover_text("How clicks, right-clicks and double-clicks work: a preset, or Custom");
    });
}

/// The workflow entries (› Workflow modes), the toolbar's workflow menu: the four modes and, under
/// a divider, a submenu per workflow setting with its choices, each in a word or two with its value
/// dim at the right, then Allow stacked notes. A pick is left in `state` for the window, which
/// keeps it in the keymap.
pub(super) fn workflow_entries(ui: &mut Ui, keymap: &crate::settings::keymap::Keymap, state: &mut PianoRollState) {
    use crate::settings::keymap::Preset;
    use crate::settings::workflow::{Pick, Setting};
    let mode = keymap.preset();
    let current = keymap.workflow();
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut picked = None;
    for choice in Preset::MODES {
        let response = ui.selectable_label(choice == mode, egui::RichText::new(choice.name()).font(font.clone()));
        if response.clicked() {
            if choice != mode {
                picked = Some(Pick::Mode(choice));
            }
            ui.close();
        }
    }
    ui.separator();
    // Each setting in a word or two, its value dim at the right and its choices one level down:
    // short enough to read at a glance, and changed and tried at once.
    for setting in Setting::ALL {
        let chosen = current.get(setting);
        let shown = setting.choices().get(chosen).copied().unwrap_or("");
        let value = setting.short_choices().get(chosen).copied().unwrap_or(shown);
        let button = egui::Button::new(egui::RichText::new(setting.short_name()).font(font.clone())).right_text((
            egui::RichText::new(value).font(font.clone()).color(theme::text_hint()),
            egui::containers::menu::SubMenuButton::arrow_atom(None),
        ));
        let label = format!("{}: {shown}", setting.name());
        let (response, _) = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
            for (index, choice) in setting.choices().iter().enumerate() {
                let response = ui.selectable_label(index == chosen, egui::RichText::new(*choice).font(font.clone()));
                let name = format!("{}: {choice}", setting.name());
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, index == chosen, &name));
                if response.clicked() {
                    if index != chosen {
                        picked = Some(Pick::Setting(setting, index));
                    }
                    ui.close();
                }
            }
        });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        response.on_hover_text(setting.note());
    }
    // How notes that land on others of their key behave belongs with how the mouse edits them
    // (› Allow stacked notes).
    ui.separator();
    let stacked = !state.notes_replace;
    let response = ui.selectable_label(stacked, egui::RichText::new("Allow stacked notes").font(font.clone()));
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, stacked, "Workflow: Allow stacked notes"));
    if response.clicked() {
        state.view_picked = Some(super::ViewPick::AllowStacked(!stacked));
        ui.close();
    }
    if picked.is_some() {
        state.workflow_picked = picked;
    }
}

/// The toolbar's icon buttons as they are painted this frame: the surface they are drawn on, the
/// keys their tooltips show and the Toolbar size their shapes are drawn at.
pub(super) struct ButtonPaint<'a> {
    pub(super) ui: &'a Ui,
    pub(super) painter: &'a egui::Painter,
    pub(super) keymap: &'a crate::settings::keymap::Keymap,
    pub(super) scale: f32,
}

impl ButtonPaint<'_> {
    /// One icon button: the shape in the tool's colours, a tooltip with its name and the key it
    /// has, and the same name as its accessible label. Returns whether it was clicked.
    pub(super) fn place(&self, area: Rect, button: IconButton, chosen: bool, active: bool) -> bool {
        let ui = self.ui;
        let painter = self.painter;
        let response = widgets::button_area(ui, area, Id::unique(("piano-roll-icon", button.name())), button.name());
        let hint = icon_hint(button, self.keymap);
        let response = response.on_hover_text(hint.clone());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, active, chosen, hint.clone()));
        // `.piano-tool` styles them all, `#piano-draw` and the others one; `:checked` is the
        // chosen tool or an open column. `piano.draw` (`.on`, `.hover`) replaces its shape.
        let _style = crate::kit::style_class::enter_named_response("piano-tool", button.id(), &response);
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, chosen);
        let color = crate::kit::style_class::foreground(if !active {
            theme::text_hint()
        } else if chosen {
            theme::text_primary()
        } else {
            theme::text_secondary()
        });
        let state = if !active {
            crate::kit::skin::State::Disabled
        } else {
            crate::kit::skin::State::response(&response, chosen)
        };
        let role = format!("piano.{}", button.id().trim_start_matches("piano-"));
        if active {
            // Only the chosen tool or an open part has a fill; the others show one on hover.
            let fill = if chosen { theme::control_active() } else { Color32::TRANSPARENT };
            crate::kit::native_css::surface(
                painter,
                area,
                0.0,
                crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)),
            );
        }
        if !(crate::kit::skin::has_own(&role) && crate::kit::skin::paint(painter, area, &role, state, color, 0.0)) {
            button.draw(painter, area, color, self.scale);
        }
        active && response.clicked()
    }
}

/// The name and key an icon button shows, such as "Draw (2)": the key is the one the keymap has
/// bound to the button's action now, so a rebound key (or one a preset moved) shows as it is.
pub(super) fn icon_hint(button: IconButton, keymap: &crate::settings::keymap::Keymap) -> String {
    match button.action().and_then(|id| keymap.label(id)) {
        Some(key) => format!("{} ({key})", button.name()),
        None => button.name().to_string(),
    }
}
