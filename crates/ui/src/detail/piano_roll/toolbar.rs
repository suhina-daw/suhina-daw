// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's toolbar: its tool buttons, and the grid and workflow menus.

use super::*;

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
    painter.rect_filled(rect, 0.0, crate::kit::style_class::surface_or(theme::panel_background()));
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let row = rect.shrink2(Vec2::new(4.0, 3.0));
    // The icon box, the gap and the strokes scale with Toolbar size; the box never outgrows the
    // band, so a small detail area shows the icons and not a clipped toolbar.
    let icon = (TOOL_ICON * scale).min(row.height());
    let gap = TOOL_GAP * scale;
    let side = row.height().min(icon);
    // `piano-tool { width; height }` sizes the buttons, so stretched artwork has room.
    let size = {
        let el = crate::kit::element::PIANO_TOOL.enter();
        let own = el.style();
        Vec2::new(
            own.as_ref().and_then(|c| c.width).map_or(side, |w| w.clamp(8.0, 96.0)),
            own.as_ref().and_then(|c| c.height).map_or(side, |h| h.clamp(8.0, row.height().max(8.0))),
        )
    };
    let x = std::cell::Cell::new(row.left() + TOOLBAR_INSET);
    let place = || {
        let area = Rect::from_min_size(Pos2::new(x.get(), row.top() + (row.height() - size.y) / 2.0), size);
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
    let selected = !state.acting().is_empty();
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
                            egui::WidgetInfo::selected(egui::Role::Button, true, index == chosen, &name)
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
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
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
        let response = widgets::button_area(ui, area, Id::unique(("piano-roll-icon", button.name())), button.name());
        let hint = icon_hint(button, self.keymap);
        let response = response.on_hover_text(hint.clone());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, active, chosen, hint.clone()));
        // `piano-tool` styles them all, `#piano-draw` and the others one; `[checked]` is the
        // chosen tool or an open column. `piano.draw` (`.on`, `.hover`) replaces its shape.
        let el = crate::kit::element::PIANO_TOOL
            .id(button.id())
            .response(&response)
            .attr(crate::kit::element::Checked, chosen)
            .enter();
        let color = el.color_or(if !active {
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
            let fill = if chosen { theme::control_active() } else { theme::control_background() };
            let fill = crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response));
            el.paint_with(painter, area, crate::kit::element::Native { radius: 0.0, fill });
        }
        if !(crate::kit::skin::has_own(&role) && crate::kit::skin::paint_as(&el, painter, area, &role, state, color)) {
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
