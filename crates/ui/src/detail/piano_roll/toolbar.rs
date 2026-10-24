// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's toolbar: its tool buttons, and the grid and workflow menus.

use super::*;

/// The toolbar's buttons: one shape each, drawn in code with our own strokes, as
/// [`crate::browser::browser_view`]'s icons are. `active` marks the note clipboard's buttons while there
/// is something to act on, and the quantise block while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IconButton {
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
            Self::Quantise | Self::MidiTools | Self::Audition => return None,
        })
    }

    /// Our own strokes for the shape, in a 14 px box around `rect`'s centre, scaled with the
    /// toolbar (Toolbar size): `scale` multiplies the shape and its strokes together.
    pub(super) fn draw(self, painter: &egui::Painter, rect: Rect, color: Color32, scale: f32) {
        let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) } else { 1.0 };
        let c = rect.center();
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
                painter.rect_filled(front, 0.0, theme::panel_background());
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

/// The toolbar along the top of the piano roll: the tools, the note clipboard and the quantise
/// button as icons drawn in code, and the grid value at the right end. Returns nothing; a button
/// that runs something adds to `actions`.
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
    let paint = ButtonPaint { ui, painter: &painter, keymap, scale };
    painter.rect_filled(rect, 0.0, theme::panel_background());
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let row = rect.shrink2(Vec2::new(4.0, 3.0));
    // The icon box, the gap and the strokes scale with Toolbar size; the box never outgrows the
    // band, so a small detail area shows the icons and not a clipped toolbar.
    let icon = (TOOL_ICON * scale).min(row.height());
    let gap = TOOL_GAP * scale;
    let side = row.height().min(icon);
    let x = std::cell::Cell::new(row.left() + KEYS - 4.0);
    let place = || {
        let area = Rect::from_min_size(Pos2::new(x.get(), row.top() + (row.height() - side) / 2.0), Vec2::splat(side));
        x.set(area.right() + gap);
        area
    };
    for (tool, button) in [
        (Some(Tool::Select), IconButton::Select),
        (Some(Tool::Draw), IconButton::Draw),
        (Some(Tool::Paint), IconButton::Paint),
        (Some(Tool::Erase), IconButton::Erase),
        (Some(Tool::Slice), IconButton::Slice),
        (Some(Tool::Mute), IconButton::Mute),
    ] {
        let area = place();
        let chosen = tool == Some(state.tool);
        if paint.place(area, button, chosen, true) {
            state.tool = tool.unwrap_or(state.tool);
        }
    }
    // A divider, then the note clipboard's buttons.
    x.set(x.get() + 2.0);
    if x.get() < row.right() {
        painter.vline(x.get(), row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
    }
    x.set(x.get() + 5.0);
    let selected = !state.selected.is_empty();
    for button in [IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate] {
        let area = place();
        let active = matches!(button, IconButton::Paste) || selected;
        if paint.place(area, button, false, active)
            && let Some(command) = state.clipboard_action(button, clip, copied)
        {
            actions.push(command);
        }
    }
    // A divider, then the quantise button (which shows the MIDI tools column and its quantise
    // block) and the column's own toggle.
    x.set(x.get() + 2.0);
    if x.get() < row.right() {
        painter.vline(x.get(), row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
    }
    x.set(x.get() + 5.0);
    let quantise = place();
    if paint.place(quantise, IconButton::Quantise, state.quantize_open, true) {
        state.quantize_open = !state.quantize_open;
        if state.quantize_open {
            state.tools_open = true;
        }
    }
    let tools = place();
    if paint.place(tools, IconButton::MidiTools, state.tools_open, true) {
        state.tools_open = !state.tools_open;
    }
    let audition = place();
    if paint.place(audition, IconButton::Audition, state.audition, true) {
        state.audition = !state.audition;
        if !state.audition {
            state.audition_pending.clear();
        }
    }
    // The workflow menu: the mode's name, the four modes and every workflow setting
    // (› Workflow modes).
    let hide_left = row.right() - side;
    let workflow = Rect::from_min_size(Pos2::new(x.get() + 4.0, row.top()), Vec2::new(WORKFLOW_MENU, row.height()));
    if workflow.right() + 4.0 + 104.0 < hide_left {
        // Each menu takes the width its button needs, so the next one starts right after it.
        let shown = workflow_menu(ui, workflow, keymap, state);
        x.set(shown.right().min(workflow.right()) + gap);
        // The ghost menu after it, when there is room for both (› Ghost notes).
        let ghosts =
            Rect::from_min_size(Pos2::new(x.get() + 4.0, row.top()), Vec2::new(ghost_view::MENU_WIDTH, row.height()));
        if ghosts.right() + 4.0 + 104.0 < hide_left {
            let shown = ghost_view::menu(ui, ghosts, &clip.ghosts, &mut state.ghost_source);
            x.set(shown.right().min(ghosts.right()) + gap);
            // The Scale menu after it, when the grid value still fits after it (› Scale and key);
            // the grid value's caption gives way to it.
            let menu = Rect::from_min_size(
                Pos2::new(x.get() + 4.0, row.top()),
                Vec2::new(scale_view::MENU_WIDTH, row.height()),
            );
            if menu.right() + gap + 2.0 + GRID_MENU + 6.0 <= hide_left - gap {
                let before = state.fold;
                let shown = scale_view::menu(ui, menu, clip, &mut state.fold, actions);
                if state.fold != before {
                    // Another set of rows: the notes are centred again in them.
                    state.scrolled_by_hand = false;
                    state.centred_for = 0.0;
                }
                x.set(shown.right().min(menu.right()) + gap);
            }
        }
    }
    let paint = ButtonPaint { ui, painter: &painter, keymap, scale };
    let x = x.get();
    // At the very end the × that hides the editor, then the grid value before it.
    let hide =
        Rect::from_min_size(Pos2::new(row.right() - side, row.top() + (row.height() - side) / 2.0), Vec2::splat(side));
    if hide.left() >= x && paint.place(hide, IconButton::Hide, false, true) {
        state.hide_requested = true;
    }
    let row = Rect::from_min_max(row.min, Pos2::new(hide.left() - gap - 2.0, row.bottom()));
    let menu = Rect::from_min_max(Pos2::new(row.right() - GRID_MENU, row.top()), row.max);
    if menu.left() < x + 6.0 {
        return;
    }
    // The caption, when there is room for it too.
    if menu.left() >= x + 40.0 {
        let label =
            Rect::from_min_max(Pos2::new(menu.left() - 34.0, row.top()), Pos2::new(menu.left() - 4.0, row.bottom()));
        widgets::text_in(
            &painter,
            label,
            Align2::RIGHT_CENTER,
            "Grid",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    if let Some(picked) = grid_menu(ui, menu, clip, keymap) {
        state.grid_picked = Some(picked);
    }
}

/// How wide the toolbar's grid menu button is, in points.
pub(super) const GRID_MENU: f32 = 76.0;

/// The toolbar's grid menu (› Grid and snapping): its button shows the grid value; its menu holds
/// Snap to grid, the adaptive widths, the fixed values, Narrow grid, Widen grid and Triplet grid,
/// each with its key, the chosen ones ticked. While the grid follows the control bar, an entry the
/// control bar cannot hold is greyed out. Returns the grid picked this frame.
pub(super) fn grid_menu(
    ui: &mut Ui,
    rect: Rect,
    clip: &PianoClip,
    keymap: &crate::settings::keymap::Keymap,
) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let shown = grid.label();
    let mut picked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = egui::RichText::new(format!("{shown}   ")).font(mono.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            // One entry: its text, the key its action has, ticked when `ticked` says so.
            let mut entry = |ui: &mut Ui, name: &str, action: Option<&str>, ticked: Option<bool>, result: PianoGrid| {
                let enabled = !clip.grid_follows || result.control_bar_value().is_some();
                let key = action.and_then(|a| keymap.label(a)).unwrap_or_default();
                let text = egui::RichText::new(name).font(font.clone());
                let button = egui::Button::selectable(ticked.unwrap_or(false), text)
                    .shortcut_text(egui::RichText::new(key.as_str()).font(mono.clone()).color(theme::text_hint()));
                let response = ui.add_enabled(enabled, button);
                // Its accessible label is its name and its key, as the icon buttons' are.
                let label = if key.is_empty() { name.to_string() } else { format!("{name} ({key})") };
                response.widget_info(|| match ticked {
                    Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, &label),
                    None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label),
                });
                if response.clicked() {
                    picked = Some(result);
                    ui.close();
                }
            };
            let heading = |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).font(font.clone()).color(theme::text_hint()));
            };
            entry(ui, "Snap to grid", Some("pianoRoll.gridSnap"), Some(grid.snap), grid.toggle_snap());
            ui.separator();
            heading(ui, "Adaptive grid");
            for width in Width::ALL {
                let name = format!("Adaptive {}", width.name().to_lowercase());
                let result = PianoGrid { mode: GridMode::Adaptive(width), ..grid };
                entry(ui, &name, None, Some(grid.mode == GridMode::Adaptive(width)), result);
            }
            ui.separator();
            heading(ui, "Fixed grid");
            for (index, value) in FIXED_VALUES.iter().enumerate() {
                // Picking a value while following the control bar turns snapping back on: its
                // none is snap off.
                let snap = grid.snap || clip.grid_follows;
                let result = PianoGrid { mode: GridMode::Fixed(index), snap, ..grid };
                entry(ui, value, None, Some(grid.mode == GridMode::Fixed(index)), result);
            }
            ui.separator();
            entry(ui, "Narrow grid", Some("pianoRoll.gridNarrow"), None, grid.narrower());
            entry(ui, "Widen grid", Some("pianoRoll.gridWiden"), None, grid.wider());
            ui.separator();
            entry(ui, "Triplet grid", Some("pianoRoll.gridTriplet"), Some(grid.triplet), grid.toggle_triplet());
        });
        let label = format!("Grid value {shown}");
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        menu.response.on_hover_text("The grid: snapping, adaptive or fixed lines, triplets");
    });
    picked.filter(|p| *p != grid)
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

/// How wide the toolbar's workflow menu is, in points.
pub(super) const WORKFLOW_MENU: f32 = 96.0;

/// The toolbar's workflow menu: its button shows the mode, its menu offers the four modes and,
/// under a divider, a submenu per workflow setting with its choices. A pick is left in `state`
/// for the window, which keeps it in the keymap.
pub(super) fn workflow_menu(
    ui: &mut Ui,
    rect: Rect,
    keymap: &crate::settings::keymap::Keymap,
    state: &mut PianoRollState,
) -> Rect {
    use crate::settings::keymap::Preset;
    use crate::settings::workflow::{Pick, Setting};
    let mode = keymap.preset();
    let current = keymap.workflow();
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut picked = None;
    let mut shown = rect;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = egui::RichText::new(format!("{}   ", mode.name())).font(font.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            for choice in Preset::MODES {
                let response =
                    ui.selectable_label(choice == mode, egui::RichText::new(choice.name()).font(font.clone()));
                if response.clicked() {
                    if choice != mode {
                        picked = Some(Pick::Mode(choice));
                    }
                    ui.close();
                }
            }
            ui.separator();
            for setting in Setting::ALL {
                let chosen = current.get(setting);
                let shown = setting.choices().get(chosen).copied().unwrap_or("");
                let title = egui::RichText::new(format!("{}: {shown}", setting.name())).font(font.clone());
                ui.menu_button(title, |ui| {
                    for (index, choice) in setting.choices().iter().enumerate() {
                        let response =
                            ui.selectable_label(index == chosen, egui::RichText::new(*choice).font(font.clone()));
                        let name = format!("{}: {choice}", setting.name());
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, index == chosen, &name)
                        });
                        if response.clicked() {
                            if index != chosen {
                                picked = Some(Pick::Setting(setting, index));
                            }
                            ui.close();
                        }
                    }
                });
            }
        });
        let label = format!("Workflow {}", mode.name());
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        shown = menu.response.rect;
        menu.response.on_hover_text("How clicks, right-clicks and double-clicks work: a preset, or Custom");
    });
    if picked.is_some() {
        state.workflow_picked = picked;
    }
    shown
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
        let response = widgets::button_area(ui, area, Id::new(("piano-roll-icon", button.name())), button.name());
        let hint = icon_hint(button, self.keymap);
        let response = response.on_hover_text(hint.clone());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, active, chosen, hint.clone()));
        if active {
            let fill = if chosen { theme::control_active() } else { theme::control_background() };
            painter.rect_filled(
                area,
                0.0,
                crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)),
            );
        }
        let color = if !active {
            theme::text_hint()
        } else if chosen {
            theme::text_primary()
        } else {
            theme::text_secondary()
        };
        button.draw(painter, area, color, self.scale);
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
