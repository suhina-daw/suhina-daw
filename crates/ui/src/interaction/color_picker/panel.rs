// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The floating picker's controls and bounded color suggestions. Project edits belong to
//! the app; this panel only reports choices and the lifetime of a pointer gesture.

mod canvas;
pub(crate) mod steps;

use egui::{Id, Key, Sense, Ui, Vec2};
use suhina_engine::model::Rgb;
use suhina_style::value::{hsl_to_srgb, srgb_to_hsl};

use super::{color32, hex};
use crate::kit::{controls, element, widgets};

/// The room between rows.
const SPACING: Vec2 = Vec2::new(4.0, 5.0);
/// How tall the picker is in either [`View`] in the built-in themes, until it has been drawn: the
/// window sizes itself from this the first time, and from what it measures after.
const VIEW_GUESS: f32 = 240.75;

/// How the picker picks a color: from the Steps grid of a hue, or anywhere on a
/// saturation/value field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Steps,
    Field,
}

/// Conventional HSV of encoded sRGB, so its hue agrees with numeric HSL. Reuse egui's
/// component conversion, without its linear-light Hsva transfer functions.
#[derive(Clone, Copy)]
pub(super) struct Hsv {
    h: f32,
    s: f32,
    v: f32,
}

impl Hsv {
    fn from_srgb(rgb: [u8; 3]) -> Self {
        let (h, s, v) = egui::ecolor::hsv_from_rgb(rgb.map(|c| f32::from(c) / 255.0));
        Self { h, s, v }
    }

    fn to_srgb(self) -> [u8; 3] {
        egui::ecolor::rgb_from_hsv((self.h, self.s, self.v)).map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8)
    }
}

pub(crate) const GESTURE: &str = "track-color-window";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Edit {
    Color(Rgb, bool),
    Saved(Saved),
    Default,
    Finish,
    Cancel,
}

/// A change to the saved colors, by place in the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Saved {
    /// Add the color at the end.
    Add(Rgb),
    /// Put the color in place of the one at the place.
    Replace(usize, Rgb),
    Remove(usize),
    /// Put the color back at the place, as Undo after [`Saved::Remove`].
    Insert(usize, Rgb),
    /// Take the color at the first place and put it before the one at the second (the end
    /// when it is the length).
    Move(usize, usize),
}

/// How many saved colors a row holds: as many as the palette's.
const SAVED_COLUMNS: usize = suhina_session::colors::PALETTE_COLUMNS;
/// How many rows of saved colors show before they scroll.
const SAVED_ROWS: usize = 4;

pub(crate) struct Panel {
    /// The picker's view drawn now.
    view: View,
    /// The view the user last asked for, drawn once [`Self::settle`] runs (see [`Self::asks`]).
    ask_view: View,
    /// The heights last measured: the whole panel less the picker, and the picker in Steps and
    /// in Field.
    base: Option<f32>,
    views: [f32; 2],
    pub dragging: bool,
    cancelled_drag: bool,
    pub color: Rgb,
    hsv: Hsv,
    text: String,
    name: String,
    numbers: [String; 6],
    invalid: bool,
    soft: bool,
    /// Whether Steps' hue moves in [`steps::HUES`] steps, or to any hue.
    stepped: bool,
    random: u64,
    canvas: canvas::Cache,
    /// The Steps grid and the HSV hue (0..1) it was made for.
    grid: (f32, [Rgb; steps::CELLS]),
    /// The saved color being dragged to a new place, by its place.
    moving: Option<usize>,
    /// The saved color last removed and its place, for Undo until the next change to them.
    removed: Option<(usize, Rgb)>,
    /// The window colors clips, not tracks: Default gives them their tracks' color.
    pub clips: bool,
}

impl Default for Panel {
    fn default() -> Self {
        let color = suhina_session::colors::MIDI;
        let mut panel = Self {
            view: View::Steps,
            ask_view: View::Steps,
            base: None,
            views: [VIEW_GUESS; 2],
            dragging: false,
            cancelled_drag: false,
            color,
            hsv: Hsv::from_srgb([color.0, color.1, color.2]),
            text: String::new(),
            name: String::new(),
            numbers: Default::default(),
            invalid: false,
            soft: false,
            stepped: true,
            random: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
                .max(1),
            canvas: Default::default(),
            grid: (-1.0, steps::grid(0.0)),
            moving: None,
            removed: None,
            clips: false,
        };
        panel.set(color);
        panel
    }
}

impl Panel {
    /// Whether the user asked for a view that is not drawn yet. A view that needs more room
    /// (the taller one) waits until the window has grown to
    /// [`Self::height`], so no frame draws controls the window cuts off; one that needs less may
    /// settle at once.
    pub fn asks(&self) -> bool {
        self.ask_view != self.view
    }

    /// Draw the view the user asked for from the next [`Self::show`] on.
    pub fn settle(&mut self) {
        self.view = self.ask_view;
    }

    /// How tall the panel is in the view drawn now (`asked` false) or the one asked for, once it
    /// has been shown once. A view's height is a guess until it has been drawn.
    pub fn height(&self, asked: bool) -> Option<f32> {
        let view = if asked { self.ask_view } else { self.view };
        self.base.map(|base| base + self.views[view as usize])
    }

    pub fn discard_drafts(&mut self) {
        self.fields();
        self.removed = None;
    }

    pub fn cancel_pointer(&mut self) {
        self.dragging = false;
        self.cancelled_drag = true;
    }

    /// Follow commands, Undo and selection changes without converting unchanged exact RGB.
    pub fn sync(&mut self, color: Rgb) {
        if self.color != color {
            self.set(color);
        }
    }

    fn fields(&mut self) {
        self.text = hex(self.color);
        self.name = super::names::describe(self.color);
        let Rgb(r, g, b) = self.color;
        let [h, s, l] = srgb_to_hsl([r, g, b]);
        self.numbers = [
            r.to_string(),
            g.to_string(),
            b.to_string(),
            format!("{h:.1}"),
            format!("{:.1}", s * 100.0),
            format!("{:.1}", l * 100.0),
        ];
        self.invalid = false;
    }

    fn set(&mut self, color: Rgb) {
        self.color = color;
        let mut next = Hsv::from_srgb([color.0, color.1, color.2]);
        if next.s <= f32::EPSILON || next.v <= f32::EPSILON {
            next.h = self.hsv.h;
        }
        if next.v <= f32::EPSILON {
            next.s = self.hsv.s;
        }
        // A Steps swatch (every palette color is one) keeps the hue step whose grid holds it, so
        // Steps shows it ringed: its own RGB hue can round onto a neighboring step. Grays are in
        // every grid and keep the hue they had.
        if next.s > f32::EPSILON
            && next.v > f32::EPSILON
            && let Some(hue) = self.step_holding(color, next.h)
        {
            next.h = hue;
        }
        self.hsv = next;
        self.fields();
    }

    /// The hue of the step whose grid holds `color`, trying the grid shown first and then the
    /// steps out from the one nearest `hue` (0..1); the grid found becomes the one shown.
    fn step_holding(&mut self, color: Rgb, hue: f32) -> Option<f32> {
        let hues = steps::hues();
        if hues.contains(&self.grid.0) && self.grid.1.contains(&color) {
            return Some(self.grid.0);
        }
        let nearest = steps::nearest(hue) as isize;
        // Off the hue's own step by more than a few, a cell would read as another hue.
        const REACH: isize = 3;
        let len = hues.len() as isize;
        (0..=REACH).flat_map(|d| [d, -d]).find_map(|d| {
            let step = *hues.get((nearest + d).rem_euclid(len) as usize)?;
            let grid = steps::grid(step);
            grid.contains(&color).then(|| {
                self.grid = (step, grid);
                step
            })
        })
    }

    fn choose(&mut self, color: Rgb, exact: bool, edits: &mut Vec<Edit>) {
        if exact {
            self.soft = false;
        }
        self.set(color);
        edits.push(Edit::Color(color, false));
    }

    fn pointer_color(&mut self, hsv: Hsv, dragging: bool, edits: &mut Vec<Edit>) {
        if self.cancelled_drag {
            return;
        }
        let hsv = constrain(hsv, self.soft);
        let [r, g, b] = hsv.to_srgb();
        let color = Rgb(r, g, b);
        self.hsv = hsv;
        // Hue can change while black/gray without changing RGB: retain it for the next move.
        if color != self.color {
            self.color = color;
            self.fields();
            edits.push(Edit::Color(color, dragging));
        }
        self.dragging |= dragging;
    }

    pub fn show(&mut self, ui: &mut Ui, saved: &[Rgb]) -> Vec<Edit> {
        let mut edits = Vec::new();
        ui.spacing_mut().item_spacing = SPACING;
        ui.spacing_mut().button_padding = Vec2::new(5.0, 2.0);
        controls::heading(ui, "Palette");
        if let Some(color) = palette(ui, &super::swatches(), self.color) {
            self.choose(color, true, &mut edits);
        }
        controls::separator(ui);
        controls::heading(ui, "Saved colors");
        self.saved(ui, saved, &mut edits);
        controls::separator(ui);
        controls::text(ui, &format!("Current color · {}", self.name))
            .on_hover_text("The nearest named color. The hex value remains exact.");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(62.0, controls::HEIGHT), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, color32(self.color));
            // `color-swatch[kind="square"]`'s `border-color`, as beside a color's name in a menu.
            let edge = element::COLOR_SWATCH.attribute("kind", "square").look().border();
            ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
            let id = Id::unique("track-color-hex");
            let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
            let response = widgets::text_input(
                ui,
                egui::TextEdit::singleline(&mut self.text).id(id).desired_width(100.0).char_limit(16),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), "Hex color"));
            if (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
                self.fields();
                response.surrender_focus();
            } else if response.lost_focus() || (response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                if let Some(color) = suhina_session::colors::parse_hex(self.text.trim()) {
                    if color != self.color {
                        self.choose(color, true, &mut edits);
                    } else {
                        self.fields();
                    }
                } else {
                    self.invalid = true;
                    // Enter normally surrenders text focus. An invalid draft stays editable
                    // so Escape can discard it and the error does not strand the controls.
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        response.request_focus();
                    }
                }
            }
            if controls::button(ui, "Copy").on_hover_text("Copy exact #RRGGBB color").clicked() {
                ui.ctx().copy_text(hex(self.color));
            }
        });
        let top = ui.cursor().top();
        self.picker(ui, &mut edits);
        let picker = ui.cursor().top() - top;
        controls::separator(ui);
        ui.horizontal(|ui| {
            if controls::button(ui, "Random").clicked() {
                self.random ^= self.random << 13;
                self.random ^= self.random >> 7;
                self.random ^= self.random << 17;
                let color = random_color(self.color, self.soft, self.random);
                self.choose(color, false, &mut edits);
            }
            let default = if self.clips {
                "Give each clip its track's color again"
            } else {
                "Restore each selected track's default color"
            };
            if controls::button(ui, "Default").on_hover_text(default).clicked() {
                self.discard_drafts();
                edits.push(Edit::Default);
            }
            if self.invalid {
                let ink = element::LABEL.attribute("kind", "error").look().color();
                controls::words(ui, "Enter a valid color value", ink);
            }
        });
        if self.dragging && ui.input(|i| i.key_pressed(Key::Escape)) {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            self.cancel_pointer();
            edits.clear();
            edits.push(Edit::Cancel);
        } else if self.dragging && !ui.input(|i| i.pointer.any_down()) {
            self.dragging = false;
            edits.push(Edit::Finish);
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.cancelled_drag = false;
        }
        let whole = ui.min_rect().height();
        self.base = Some(whole - picker);
        self.views[self.view as usize] = picker;
        edits
    }

    /// The saved colors in rows as wide as the palette's, then a + that saves the current
    /// color (dim while it is saved already), one stop for Tab (see [`super::Roving`]). A click
    /// applies a color and a drag moves it. On the one with the keys, Alt+Left and Alt+Right
    /// move it, Delete removes it (the keys go to the one after it) and the Menu key or
    /// Shift+F10 opens its menu, as a right-click does: Move earlier, Move later, Replace with
    /// current color, Remove. After a removal, Undo puts it back until the next change. Past
    /// [`SAVED_ROWS`] rows they scroll.
    fn saved(&mut self, ui: &mut Ui, saved: &[Rgb], edits: &mut Vec<Edit>) {
        let gap = ui.spacing().item_spacing;
        let columns = SAVED_COLUMNS as f32;
        let width = ((ui.available_width() - (columns - 1.0) * gap.x) / columns).clamp(14.0, 30.0);
        let size = Vec2::new(width, 18.0);
        let pitch = size + gap;
        let count = saved.len();
        let rows = (count + 1).div_ceil(SAVED_COLUMNS);
        let height = rows as f32 * pitch.y - gap.y;
        let shown = SAVED_ROWS as f32 * pitch.y - gap.y;
        let is_saved = saved.contains(&self.color);
        let current = self.color;
        let before = edits.len();
        // By place, so the keys stay at a place when the color there changes; the + is last.
        let ids: Vec<Id> = (0..=count).map(|index| ui.scope_id().with(("saved-color", index))).collect();
        let plus_id = ui.scope_id().with(("saved-color", count));
        let mut focus_to = None;
        let mut grid = |ui: &mut Ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(columns * pitch.x - gap.x, height), Sense::hover());
            let slot = |index: usize| {
                let (row, column) = (index / SAVED_COLUMNS, index % SAVED_COLUMNS);
                egui::Rect::from_min_size(area.min + Vec2::new(column as f32 * pitch.x, row as f32 * pitch.y), size)
            };
            let fallback = saved.iter().position(|c| *c == current).unwrap_or(count);
            let roving = super::Roving::new(ui, &ids, SAVED_COLUMNS, Some(fallback));
            let mut focused = None;
            for (index, (&color, &id)) in saved.iter().zip(&ids).enumerate() {
                let rect = slot(index);
                let response = ui.interact(rect, id, roving.sense(index, Sense::click_and_drag()));
                let name = super::swatch_name("Saved color", color);
                let chosen = color == current;
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &name));
                let lifted = self.moving == Some(index);
                let fill = color32(color);
                ui.painter().rect_filled(rect, 0.0, if lifted { fill.gamma_multiply(0.4) } else { fill });
                if chosen || response.hovered() {
                    super::swatch_ring(ui.painter(), &response, color, chosen);
                }
                if response.clicked() {
                    // The keys follow the click, so the arrows go on from here.
                    super::focus_by_pointer(&response);
                    self.choose(color, true, edits);
                }
                if response.drag_started() {
                    self.moving = Some(index);
                }
                let mut menu_key = false;
                if response.has_focus() {
                    focused = Some(rect);
                    let (earlier, later, delete, menu) = ui.input_mut(|i| {
                        let alt = egui::Modifiers::ALT;
                        (
                            i.consume_key(alt, Key::ArrowLeft),
                            i.consume_key(alt, Key::ArrowRight),
                            i.consume_key(egui::Modifiers::NONE, Key::Delete)
                                || i.consume_key(egui::Modifiers::NONE, Key::Backspace),
                            i.consume_key(egui::Modifiers::NONE, Key::Menu)
                                || i.consume_key(egui::Modifiers::SHIFT, Key::F10),
                        )
                    });
                    if earlier && index > 0 {
                        edits.push(Edit::Saved(Saved::Move(index, index - 1)));
                        focus_to = Some(index - 1);
                    }
                    if later && index + 1 < count {
                        edits.push(Edit::Saved(Saved::Move(index, index + 2)));
                        focus_to = Some(index + 1);
                    }
                    if delete {
                        edits.push(Edit::Saved(Saved::Remove(index)));
                        self.removed = Some((index, color));
                        // The color after it, or the + when none is, moves into this place and
                        // keeps the keys; the last of several hands them to the one before it.
                        if index + 1 == count && index > 0 {
                            focus_to = Some(index - 1);
                        }
                    }
                    menu_key = menu;
                }
                let open = if response.secondary_clicked() || menu_key {
                    Some(egui::SetOpenCommand::Bool(true))
                } else if response.clicked() {
                    Some(egui::SetOpenCommand::Bool(false))
                } else {
                    None
                };
                egui::Popup::menu(&response).open_memory(open).show(|ui| {
                    use crate::kit::menu::item;
                    if index > 0 && item(ui, "Move earlier", false).clicked() {
                        edits.push(Edit::Saved(Saved::Move(index, index - 1)));
                        focus_to = Some(index - 1);
                        ui.close();
                    }
                    if index + 1 < count && item(ui, "Move later", false).clicked() {
                        edits.push(Edit::Saved(Saved::Move(index, index + 2)));
                        focus_to = Some(index + 1);
                        ui.close();
                    }
                    if !is_saved && item(ui, "Replace with current color", false).clicked() {
                        edits.push(Edit::Saved(Saved::Replace(index, current)));
                        ui.close();
                    }
                    if item(ui, "Remove", false).clicked() {
                        edits.push(Edit::Saved(Saved::Remove(index)));
                        self.removed = Some((index, color));
                        ui.close();
                    }
                });
                if self.moving.is_none() {
                    super::swatch_tooltip(response, color);
                }
            }
            let plus = slot(count);
            let response = ui.interact(plus, plus_id, roving.sense(count, Sense::click()));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !is_saved, "Save current color"));
            // `color-swatch[kind="add"]`'s `color` for its outline and +: `:hover` under the
            // pointer, `:disabled` while the current color is saved and there is nothing to save.
            let add = element::COLOR_SWATCH.attribute("kind", "add");
            let edge = add.pointer(response.hovered(), response.is_pointer_button_down_on(), is_saved).look().color();
            ui.painter().rect_stroke(plus, 0.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
            let center = plus.center();
            for arm in [Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)] {
                ui.painter().line_segment([center - arm, center + arm], egui::Stroke::new(1.0, edge));
            }
            if response.has_focus() {
                focused = Some(plus);
            }
            let hint = if is_saved { "The current color is saved" } else { "Save current color" };
            if response.on_hover_text(hint).clicked() && !is_saved {
                edits.push(Edit::Saved(Saved::Add(current)));
            }
            if count == 0 {
                let words = egui::Rect::from_min_max(egui::pos2(plus.right() + 6.0, plus.top()), area.right_bottom());
                widgets::text_in(
                    ui.painter(),
                    words,
                    egui::Align2::LEFT_CENTER,
                    "Save colors to reuse them",
                    crate::kit::theme::ui_font(crate::kit::theme::TEXT_SMALL),
                    element::LABEL.attribute("kind", "hint").look().color(),
                );
            }
            if let Some(rect) = focused {
                super::focus_ring(ui, rect);
            }
            // The arrows pick the color they reach; reaching the + saves nothing.
            if let Some(&color) = roving.keys(ui).and_then(|index| saved.get(index)) {
                self.choose(color, true, edits);
            }
            // Moving: a line before the place the color would go, and the move on release.
            if let Some(from) = self.moving.filter(|&from| from < count) {
                let to = ui.input(|i| i.pointer.interact_pos()).map(|at| {
                    let column = ((at.x - area.left() + gap.x / 2.0) / pitch.x).round().clamp(0.0, columns) as usize;
                    let row = ((at.y - area.top()) / pitch.y).floor().clamp(0.0, (rows - 1) as f32) as usize;
                    (row * SAVED_COLUMNS + column.min(SAVED_COLUMNS)).min(count)
                });
                if let Some(to) = to {
                    // The end of a full row: after its last color, not before the next row's first.
                    let at_end = to == count && to > 0 && to % SAVED_COLUMNS == 0;
                    let line = if at_end { slot(to - 1) } else { slot(to) };
                    let x = if at_end { line.right() + gap.x / 2.0 } else { line.left() - gap.x / 2.0 };
                    let ink = element::LABEL.look().color();
                    ui.painter().line_segment(
                        [egui::pos2(x, line.top()), egui::pos2(x, line.bottom())],
                        egui::Stroke::new(2.0, ink),
                    );
                }
                if !ui.input(|i| i.pointer.any_down()) {
                    if let Some(to) = to
                        && to != from
                        && to != from + 1
                    {
                        edits.push(Edit::Saved(Saved::Move(from, to)));
                    }
                    self.moving = None;
                }
            } else {
                self.moving = None;
            }
        };
        if height > shown {
            egui::ScrollArea::vertical().id_salt("saved-colors").max_height(shown).show(ui, |ui| grid(ui));
        } else {
            grid(ui);
        }
        // The keys follow a moved color, or leave a removed last one for its neighbor.
        if let Some(&id) = focus_to.and_then(|to| ids.get(to)) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        // Any other change to the saved colors ends Undo for the last removal.
        let removed_now = edits[before..].iter().any(|e| matches!(e, Edit::Saved(Saved::Remove(_))));
        if !removed_now && edits.len() > before && edits[before..].iter().any(|e| matches!(e, Edit::Saved(_))) {
            self.removed = None;
        }
        if let Some((index, color)) = self.removed {
            ui.horizontal(|ui| {
                controls::words(ui, &format!("Removed {}", super::names::describe(color)), element::caption_ink());
                if controls::button(ui, "Undo").on_hover_text("Put the removed color back").clicked() {
                    edits.push(Edit::Saved(Saved::Insert(index, color)));
                    self.removed = None;
                }
            });
        }
    }

    fn picker(&mut self, ui: &mut Ui, edits: &mut Vec<Edit>) {
        let views = [("Steps", View::Steps), ("Field", View::Field)];
        if let Some(view) = controls::options(ui, "View", &views, self.ask_view) {
            self.ask_view = view;
            ui.ctx().request_repaint();
        }
        match self.view {
            View::Steps => self.steps(ui, edits),
            View::Field => self.field(ui, edits),
        }
        self.numeric_row(ui, false, edits);
        self.numeric_row(ui, true, edits);
        match self.view {
            View::Steps => {
                let stepped = controls::check_box(ui, "Hue steps", self.stepped)
                    .on_hover_text("On: the hue moves in 36 even steps. Off: any hue.");
                if stepped.clicked() {
                    self.stepped = !self.stepped;
                }
            }
            View::Field => {
                let soft = controls::check_box(ui, "Soft colors", self.soft).on_hover_text(
                    "Pointer selection: 25–70% saturation, 45–90% value. Exact entries remain unrestricted.",
                );
                if soft.clicked() {
                    self.soft = !self.soft;
                }
            }
        }
    }

    /// The grid of the current hue (its nearest step while [`Self::stepped`]), then its hue: a
    /// band like Field's that, while stepped, moves from step to step.
    fn steps(&mut self, ui: &mut Ui, edits: &mut Vec<Edit>) {
        let hues = steps::hues();
        let hue = if self.stepped { hues.get(steps::nearest(self.hsv.h)).copied().unwrap_or(0.0) } else { self.hsv.h };
        if self.grid.0 != hue {
            self.grid = (hue, steps::grid(hue));
        }
        if let Some(cell) = ui.vertical_centered(|ui| steps::show(ui, &self.grid.1, self.color)).inner
            && let Some(&color) = self.grid.1.get(cell)
        {
            self.pick(color, hue, false, edits);
        }
        let (next, response) = if self.stepped {
            let colors: Vec<egui::Color32> = hues.iter().map(|&h| full_hue(h)).collect();
            let at = |i: usize| hues.get(i).copied().unwrap_or(0.0);
            let mut index = steps::nearest(hue);
            let response = controls::stepped_band(
                ui,
                "Hue",
                &mut index,
                |i| format!("{:.0}°", at(i) * 360.0),
                &colors,
                full_hue(hue),
            );
            (at(index), response)
        } else {
            hue_band(ui, hue)
        };
        if response.changed() && next != hue {
            // The same place in the new hue's grid: where the current color is, or the nearest.
            let cell = self
                .grid
                .1
                .iter()
                .position(|c| *c == self.color)
                .unwrap_or_else(|| nearest_cell(&self.grid.1, self.color));
            let grid = steps::grid(next);
            self.grid = (next, grid);
            if let Some(&color) = grid.get(cell) {
                self.pick(color, next, response.dragged() || self.dragging, edits);
            }
            ui.ctx().request_repaint();
        }
    }

    /// Choose a Steps cell of the grid for HSV hue `hue` (0..1), keeping that hue.
    fn pick(&mut self, color: Rgb, hue: f32, dragging: bool, edits: &mut Vec<Edit>) {
        if self.cancelled_drag {
            return;
        }
        if color != self.color {
            self.set(color);
            edits.push(Edit::Color(color, dragging));
        }
        self.hsv.h = hue;
        self.dragging |= dragging;
    }

    fn field(&mut self, ui: &mut Ui, edits: &mut Vec<Edit>) {
        let mut hsv = self.hsv;
        let response = ui.vertical_centered(|ui| self.canvas.show(ui, &mut hsv, self.soft)).inner;
        if response.changed() {
            self.pointer_color(hsv, response.dragged() || self.dragging, edits);
        }
        let (hue, response) = hue_band(ui, self.hsv.h);
        if response.changed() {
            let mut hsv = self.hsv;
            hsv.h = hue;
            self.pointer_color(hsv, response.dragged() || self.dragging, edits);
        }
    }

    fn numeric_row(&mut self, ui: &mut Ui, hsl: bool, edits: &mut Vec<Edit>) {
        let start = if hsl { 3 } else { 0 };
        let labels = if hsl { ["H", "S", "L"] } else { ["R", "G", "B"] };
        let names = if hsl { ["HSL hue", "HSL saturation", "HSL lightness"] } else { ["Red", "Green", "Blue"] };
        let mut commit = false;
        let mut entered = None;
        let mut cancel = false;
        ui.horizontal(|ui| {
            for (offset, (label, name)) in labels.into_iter().zip(names).enumerate() {
                controls::text(ui, label);
                let Some(text) = self.numbers.get_mut(start + offset) else { continue };
                let id = Id::unique(("track-color-number", name));
                let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                let response =
                    widgets::text_input(ui, egui::TextEdit::singleline(text).id(id).desired_width(45.0).char_limit(12));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), name));
                cancel |= (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Escape));
                commit |= response.lost_focus() || (response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                if (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Enter)) {
                    entered = Some(id);
                }
                if cancel {
                    response.surrender_focus();
                }
            }
        });
        if cancel {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            self.fields();
            return;
        }
        if !commit {
            return;
        }
        let color = if hsl {
            let values: Option<Vec<f32>> = self.numbers.iter().skip(3).map(|s| s.trim().parse::<f32>().ok()).collect();
            values.and_then(|v| {
                let [h, s, l] = v.as_slice() else { return None };
                hsl_to_srgb([*h, *s / 100.0, *l / 100.0]).map(|[r, g, b]| Rgb(r, g, b))
            })
        } else {
            let values: Option<Vec<u8>> = self.numbers.iter().take(3).map(|s| s.trim().parse::<u8>().ok()).collect();
            values.and_then(|v| {
                let [r, g, b] = v.as_slice() else { return None };
                Some(Rgb(*r, *g, *b))
            })
        };
        if let Some(color) = color {
            // Unedited rounded HSL text must never round-trip an exact RGB color.
            let current = if hsl {
                let [h, s, l] = srgb_to_hsl([self.color.0, self.color.1, self.color.2]);
                [format!("{h:.1}"), format!("{:.1}", s * 100.0), format!("{:.1}", l * 100.0)]
            } else {
                [self.color.0.to_string(), self.color.1.to_string(), self.color.2.to_string()]
            };
            if self.numbers.get(start..start + 3) != Some(current.as_slice()) {
                self.choose(color, true, edits);
            } else {
                self.fields();
            }
        } else {
            self.invalid = true;
            if let Some(id) = entered {
                ui.memory_mut(|m| m.request_focus(id));
            }
        }
    }
}

/// HSV hue `h` (0..1) at full saturation and value.
fn full_hue(h: f32) -> egui::Color32 {
    let [r, g, b] = Hsv { h, s: 1.0, v: 1.0 }.to_srgb();
    egui::Color32::from_rgb(r, g, b)
}

/// A Hue slider over a band of every hue, moving by the degree: the hue (0..1) it gives.
fn hue_band(ui: &mut Ui, hue: f32) -> (f32, egui::Response) {
    let stops = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0].map(|sector| full_hue(sector / 6.0));
    let mut degrees = hue * 360.0;
    let response =
        controls::band(ui, "Hue", &mut degrees, 0.0..=360.0, 1.0, |h| format!("{h:.0}°"), &stops, full_hue(hue));
    (degrees / 360.0, response)
}

/// The palette in rows of [`suhina_session::colors::PALETTE_COLUMNS`] swatches as wide as the
/// panel allows, one stop for Tab (see [`super::Roving`]). The color clicked, if any.
fn palette(ui: &mut Ui, colors: &[Rgb], current: Rgb) -> Option<Rgb> {
    let mut picked = None;
    let columns = suhina_session::colors::PALETTE_COLUMNS;
    let gap = ui.spacing().item_spacing;
    let width = ((ui.available_width() - (columns - 1) as f32 * gap.x) / columns as f32).clamp(14.0, 30.0);
    let size = Vec2::new(width, 18.0);
    let pitch = size + gap;
    let rows = colors.len().div_ceil(columns);
    let (area, _) = ui.allocate_exact_size(
        Vec2::new(columns as f32 * pitch.x - gap.x, rows as f32 * pitch.y - gap.y),
        Sense::hover(),
    );
    let ids: Vec<Id> = (0..colors.len()).map(|index| ui.scope_id().with(("palette", index))).collect();
    let roving = super::Roving::new(ui, &ids, columns, colors.iter().position(|c| *c == current));
    let mut focused = None;
    for (index, (&color, &id)) in colors.iter().zip(&ids).enumerate() {
        let (row, column) = (index / columns, index % columns);
        let rect = egui::Rect::from_min_size(area.min + Vec2::new(column as f32 * pitch.x, row as f32 * pitch.y), size);
        let response = ui.interact(rect, id, roving.sense(index, Sense::click()));
        let name = super::swatch_name("Palette", color);
        let chosen = color == current;
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &name));
        ui.painter().rect_filled(rect, 0.0, color32(color));
        if chosen || response.hovered() {
            super::swatch_ring(ui.painter(), &response, color, chosen);
        }
        if response.has_focus() {
            focused = Some(rect);
        }
        if response.clicked() {
            // The keys follow the click, so the arrows go on from here.
            super::focus_by_pointer(&response);
            picked = Some(color);
        }
        super::swatch_tooltip(response, color);
    }
    if let Some(rect) = focused {
        super::focus_ring(ui, rect);
    }
    if let Some(index) = roving.keys(ui) {
        picked = colors.get(index).copied();
    }
    picked
}

/// The index of the cell nearest `color` in Oklab.
fn nearest_cell(cells: &[Rgb], color: Rgb) -> usize {
    let lab = |c: Rgb| steps::oklab([c.0, c.1, c.2].map(|v| f32::from(v) / 255.0));
    let target = lab(color);
    let distance = |c: Rgb| lab(c).into_iter().zip(target).map(|(a, b)| (a - b).powi(2)).sum::<f32>();
    cells.iter().enumerate().min_by(|a, b| distance(*a.1).total_cmp(&distance(*b.1))).map_or(0, |(i, _)| i)
}

/// Make `change` to the saved colors, keeping none twice: whether they changed.
pub(crate) fn change_saved(saved: &mut Vec<Rgb>, change: Saved) -> bool {
    match change {
        Saved::Add(color) if !saved.contains(&color) => saved.push(color),
        Saved::Replace(index, color) if !saved.contains(&color) && index < saved.len() => {
            if let Some(slot) = saved.get_mut(index) {
                *slot = color;
            }
        }
        Saved::Remove(index) if index < saved.len() => {
            saved.remove(index);
        }
        Saved::Insert(index, color) if !saved.contains(&color) => saved.insert(index.min(saved.len()), color),
        Saved::Move(from, to) if from < saved.len() && to <= saved.len() && to != from && to != from + 1 => {
            let color = saved.remove(from);
            saved.insert(if to > from { to - 1 } else { to }, color);
        }
        _ => return false,
    }
    true
}

fn constrain(mut hsv: Hsv, soft: bool) -> Hsv {
    hsv.h = hsv.h.rem_euclid(1.0);
    hsv.s = hsv.s.clamp(if soft { 0.25 } else { 0.0 }, if soft { 0.7 } else { 1.0 });
    hsv.v = hsv.v.clamp(if soft { 0.45 } else { 0.0 }, if soft { 0.9 } else { 1.0 });
    hsv
}

pub(crate) fn random_color(current: Rgb, soft: bool, random: u64) -> Rgb {
    let mut choices = Vec::with_capacity(24);
    for color in super::swatches() {
        let [r, g, b] = constrain(Hsv::from_srgb([color.0, color.1, color.2]), soft).to_srgb();
        let color = Rgb(r, g, b);
        if color != current && !choices.contains(&color) {
            choices.push(color);
        }
    }
    if choices.is_empty() {
        return current;
    }
    choices.get((random % choices.len() as u64) as usize).copied().unwrap_or(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_and_numeric_hsl_share_hue_without_changing_exact_rgb() {
        for color in super::super::swatches().into_iter().chain([Rgb(37, 119, 231), Rgb(0, 0, 0), Rgb(255, 255, 255)]) {
            let rgb = [color.0, color.1, color.2];
            let hsv = Hsv::from_srgb(rgb);
            assert_eq!(hsv.to_srgb(), rgb);
            assert!((hsv.h * 360.0 - srgb_to_hsl(rgb)[0]).abs() < 0.001);
        }
    }

    #[test]
    fn saved_colors_change_by_place_keep_none_twice_and_round_trip() {
        let colors = |n: u8| (0..n).map(|i| Rgb(i, 25, 70)).collect::<Vec<_>>();
        let mut saved = Vec::new();
        for color in colors(30) {
            assert!(change_saved(&mut saved, Saved::Add(color)));
        }
        assert_eq!(saved, colors(30), "no limit, in the order saved");
        assert!(!change_saved(&mut saved, Saved::Add(Rgb(3, 25, 70))), "a saved color is not saved twice");
        assert!(!change_saved(&mut saved, Saved::Replace(0, Rgb(3, 25, 70))));
        assert!(change_saved(&mut saved, Saved::Replace(0, Rgb(200, 0, 0))));
        assert_eq!(saved[0], Rgb(200, 0, 0));
        assert!(change_saved(&mut saved, Saved::Remove(0)));
        assert_eq!(saved, colors(30)[1..]);
        assert!(!change_saved(&mut saved, Saved::Remove(29)), "a place past the end changes nothing");
        assert!(change_saved(&mut saved, Saved::Insert(0, Rgb(0, 25, 70))), "Undo puts it back");
        assert_eq!(saved, colors(30));
        assert!(!change_saved(&mut saved, Saved::Insert(5, Rgb(0, 25, 70))));
        assert!(change_saved(&mut saved, Saved::Remove(0)));
        assert!(change_saved(&mut saved, Saved::Move(0, 3)));
        assert_eq!(&saved[..3], &[Rgb(2, 25, 70), Rgb(3, 25, 70), Rgb(1, 25, 70)]);
        assert!(change_saved(&mut saved, Saved::Move(2, 0)));
        assert_eq!(&saved[..3], &colors(4)[1..]);
        let end = saved.len();
        assert!(change_saved(&mut saved, Saved::Move(0, end)));
        assert_eq!(saved.last(), Some(&Rgb(1, 25, 70)));
        assert!(!change_saved(&mut saved, Saved::Move(4, 5)), "before the next one is where it is");
        assert!(!change_saved(&mut saved, Saved::Move(0, 99)));
        let settings = crate::settings::Settings { saved_colors: saved.clone(), ..Default::default() };
        assert_eq!(crate::settings::Settings::from_text(&settings.to_text()).0.saved_colors, saved);
        for value in ["#gg0000", "#123", "#abé12", "#000000,#123"] {
            assert!(
                crate::settings::Settings::from_text(&format!("saved_colors = {value}\n")).0.saved_colors.is_empty()
            );
        }
    }

    #[test]
    fn random_candidates_are_different_and_respect_aids_for_fixed_seeds() {
        for current in super::super::swatches() {
            for seed in 0..48 {
                let free = random_color(current, false, seed);
                assert_ne!(free, current);
                assert!(super::super::swatches().contains(&free));
                let soft = random_color(current, true, seed);
                assert_ne!(soft, current);
                let hsv = Hsv::from_srgb([soft.0, soft.1, soft.2]);
                assert!((0.24..=0.71).contains(&hsv.s));
                assert!((0.44..=0.91).contains(&hsv.v));
            }
        }
    }

    #[test]
    fn exact_values_and_black_keep_their_state_across_views_and_aids() {
        let mut panel = Panel::default();
        let exact = Rgb(37, 119, 231);
        panel.set(exact);
        panel.soft = true;
        panel.view = View::Field;
        panel.sync(exact);
        assert_eq!(panel.color, exact);
        assert_eq!(panel.text, "#2577e7");
        let hue = panel.hsv.h;
        panel.set(Rgb(0, 0, 0));
        assert_eq!(panel.hsv.h, hue);
        let mut edits = Vec::new();
        panel.choose(exact, true, &mut edits);
        assert!(!panel.soft);
        assert_eq!(edits, [Edit::Color(exact, false)]);
    }

    #[test]
    fn a_steps_cell_keeps_its_hue_step_and_the_hue_moves_to_the_same_cell() {
        let mut panel = Panel::default();
        let hue = steps::hues()[7];
        let cells = steps::grid(hue);
        let mut edits = Vec::new();
        panel.pick(cells[7], hue, false, &mut edits);
        assert_eq!(panel.color, cells[7]);
        assert_eq!(steps::nearest(panel.hsv.h), 7, "the cell's own hue would drift off the step");
        assert_eq!(edits, [Edit::Color(cells[7], false)]);
        assert_eq!(nearest_cell(&cells, cells[8]), 8);
        // White and black are in every grid, and keep the last step for the next move.
        panel.pick(cells[steps::CELLS - steps::COLUMNS], hue, false, &mut edits);
        assert_eq!(steps::nearest(panel.hsv.h), 7);
    }

    /// A palette color chosen elsewhere (a menu, a command) shows ringed in Steps: the grid shown
    /// is the step whose grid holds it, though its own hue may round onto the next step.
    #[test]
    fn a_palette_color_set_from_outside_is_in_the_steps_grid_shown() {
        for color in super::super::swatches() {
            let mut panel = Panel::default();
            panel.sync(color);
            let hue = steps::hues()[steps::nearest(panel.hsv.h)];
            assert!(steps::grid(hue).contains(&color), "{} is not in the grid shown", hex(color));
        }
    }
}
