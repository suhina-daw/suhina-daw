// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The left of the piano roll (`docs/spec/ui-clip-editor.md` › Key column and scale strip): the keys
//! drawn as a keyboard in a key style the theme can override, the scale strip that names the row
//! under the pointer and zooms the rows, the range a drag on the keys is selecting, and the corner's
//! menu.

use super::*;
use crate::kit::native_css::HostState;
use crate::settings::{PianoKeyNames, PianoKeyStyle};

/// How wide a black key is, as a share of the key column.
const BLACK_KEY_SHARE: f32 = 2.0 / 3.0;
/// How far a white key's name, and a black key's, is from the key's right end.
const NAME_INSET: f32 = 5.0;
const BLACK_NAME_INSET: f32 = 4.0;
/// Rows lower than this show no key names.
const LEAST_NAMED_ROW: f32 = 8.0;
/// The name of the row under the pointer, in the scale strip.
const POINTER_NAME: Vec2 = Vec2::new(23.0, 13.0);
/// How much one point of a sideways drag in the scale strip scales the rows, right making them taller.
const STRIP_ZOOM_PER_POINT: f32 = 0.012;
/// The size of the magnifier drawn for the pointer where a drag zooms.
const MAGNIFIER_RADIUS: f32 = 5.0;
/// How much of the accent a selected range lays over the note rows.
const RANGE_TINT: f32 = 0.11;

/// A choice made in the corner's menu this frame, for the window to keep in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPick {
    KeyStyle(PianoKeyStyle),
    KeyNames(PianoKeyNames),
    /// Whether notes of one key may overlap.
    AllowStacked(bool),
}

/// What the key column is painted with: a key style's values, and over them what the theme sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct KeyColours {
    white: Color32,
    white_c: Color32,
    white_hover: Color32,
    white_playing: Color32,
    white_selected: Color32,
    black: Color32,
    black_hover: Color32,
    black_playing: Color32,
    black_selected: Color32,
    line: Color32,
    text: Color32,
    text_black: Color32,
    text_selected: Color32,
    text_playing: Color32,
    scale: Color32,
    sheen_left: Color32,
    sheen_right: Color32,
    black_sheen: Color32,
}

/// `over` laid over `base` at `share` (0 to 1).
fn mix(base: Color32, over: Color32, share: f32) -> Color32 {
    let share = share.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    Color32::from_rgb(channel(base.r(), over.r()), channel(base.g(), over.g()), channel(base.b(), over.b()))
}

fn shine(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

/// The values of a built-in key style.
fn style_colours(style: PianoKeyStyle) -> KeyColours {
    let accent = theme::accent();
    let rgb = Color32::from_rgb;
    let dark = theme::text_inverse();
    let colours = |white,
                   white_c,
                   white_hover,
                   black,
                   black_hover,
                   line,
                   (text, text_black, text_selected),
                   scale,
                   (selected_white, selected_black),
                   (sheen_left, sheen_right, black_sheen)| KeyColours {
        white,
        white_c,
        white_hover,
        white_playing: accent,
        white_selected: mix(white, accent, selected_white),
        black,
        black_hover,
        black_playing: accent,
        black_selected: mix(black, accent, selected_black),
        line,
        text,
        text_black,
        text_selected,
        text_playing: dark,
        scale,
        sheen_left,
        sheen_right,
        black_sheen,
    };
    match style {
        PianoKeyStyle::Bone => colours(
            rgb(0xC9, 0xC4, 0xBB),
            rgb(0xAB, 0xA6, 0x9C),
            rgb(0xDA, 0xD5, 0xCB),
            rgb(0x2A, 0x29, 0x26),
            rgb(0x45, 0x43, 0x3F),
            rgb(0x8F, 0x8A, 0x80),
            (rgb(0x1D, 0x1C, 0x1A), rgb(0xC9, 0xC4, 0xBB), rgb(0x1D, 0x1C, 0x1A)),
            rgb(0x8F, 0x8A, 0x80),
            (0.55, 0.40),
            (shine(29, 28, 26, 0.20), shine(255, 255, 255, 0.22), shine(255, 255, 255, 0.16)),
        ),
        PianoKeyStyle::Stone => colours(
            rgb(0x66, 0x63, 0x5C),
            rgb(0x57, 0x54, 0x4E),
            rgb(0x75, 0x72, 0x6A),
            rgb(0x1D, 0x1C, 0x1A),
            rgb(0x34, 0x33, 0x2F),
            rgb(0x45, 0x43, 0x3F),
            (rgb(0xEC, 0xE8, 0xE1), rgb(0xC9, 0xC4, 0xBB), rgb(0x1D, 0x1C, 0x1A)),
            rgb(0xC9, 0xC4, 0xBB),
            (0.60, 0.40),
            (shine(29, 28, 26, 0.22), shine(255, 255, 255, 0.12), shine(255, 255, 255, 0.12)),
        ),
        PianoKeyStyle::Night => colours(
            rgb(0x34, 0x33, 0x2F),
            rgb(0x45, 0x43, 0x3F),
            rgb(0x4A, 0x48, 0x43),
            rgb(0x1A, 0x19, 0x17),
            rgb(0x2E, 0x2D, 0x2A),
            rgb(0x1D, 0x1C, 0x1A),
            (rgb(0xC9, 0xC4, 0xBB), rgb(0x9A, 0x95, 0x8C), rgb(0xEC, 0xE8, 0xE1)),
            rgb(0x9A, 0x95, 0x8C),
            (0.30, 0.30),
            (shine(0, 0, 0, 0.22), shine(255, 255, 255, 0.07), shine(255, 255, 255, 0.08)),
        ),
    }
}

/// Whether the theme's sheet sets the keys' colours, so a key style has no say over them.
pub(super) fn theme_sets_keys() -> bool {
    part_style("piano-key", &[]).background.is_some() || part_style("piano-key-black", &[]).background.is_some()
}

/// Whether the sheet gives `class` a gradient: it is then painted in place of the style's shading.
fn has_gradient(class: &str) -> bool {
    let _style = crate::kit::style_class::enter(class);
    crate::kit::style_class::computed().is_some_and(|c| c.has_gradient())
}

/// A state's colour from the sheet, when it differs from the base rule's: a rule for `.piano-key`
/// alone also answers for `.piano-key:selected`, and must not hide the state.
fn state_colour(class: &str, state: HostState, base: Option<Color32>) -> Option<Color32> {
    part_style(class, &[state]).background.filter(|c| Some(*c) != base)
}

/// The colours this frame: the key style's, with whatever the sheet sets over them.
pub(super) fn key_colours(style: PianoKeyStyle) -> KeyColours {
    let mut c = style_colours(style);
    let key = part_style("piano-key", &[]);
    let black = part_style("piano-key-black", &[]);
    let set = |target: &mut Color32, value: Option<Color32>| {
        if let Some(value) = value {
            *target = value;
        }
    };
    if let Some(white) = key.background {
        c.white = white;
        c.white_c = white;
        c.white_selected = mix(white, theme::accent(), 0.5);
    }
    if let Some(dark) = black.background {
        c.black = dark;
        c.black_selected = mix(dark, theme::accent(), 0.4);
    }
    set(&mut c.text, key.color);
    set(&mut c.text_black, black.color);
    set(&mut c.white_c, part_style("piano-key-c", &[]).background);
    set(&mut c.white_hover, part_style("piano-key-hover", &[]).background);
    set(&mut c.black_hover, part_style("piano-key-black-hover", &[]).background);
    set(&mut c.white_selected, state_colour("piano-key", HostState::Selected, key.background));
    set(&mut c.black_selected, state_colour("piano-key-black", HostState::Selected, black.background));
    set(&mut c.white_playing, state_colour("piano-key", HostState::Playing, key.background));
    set(&mut c.black_playing, state_colour("piano-key-black", HostState::Playing, black.background));
    set(&mut c.line, part_style("piano-key-line", &[]).background);
    set(&mut c.scale, part_style("piano-scale-mark", &[]).background);
    c
}

/// A horizontal shading over `rect`, from `left` to `right`.
fn shade(painter: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Paints what the sheet's gradient for `class` gives `rect`.
fn sheet_gradient(painter: &egui::Painter, class: &str, rect: Rect) {
    let _style = crate::kit::style_class::enter(class);
    crate::kit::native_css::background(painter, rect);
}

/// What state each key is in this frame.
struct KeyStates<'a> {
    colours: &'a KeyColours,
    playing: Option<u8>,
    hover: Option<u8>,
    ranges: Vec<(u8, u8)>,
}

impl KeyStates<'_> {
    fn selected(&self, key: u8) -> bool {
        self.ranges.iter().any(|(low, high)| (*low..=*high).contains(&key))
    }

    /// The fill of a white key; a key that starts an octave stands out over its whole shape.
    fn white(&self, key: u8) -> Color32 {
        let c = self.colours;
        if self.playing == Some(key) {
            c.white_playing
        } else if self.hover == Some(key) {
            c.white_hover
        } else if self.selected(key) {
            c.white_selected
        } else if key.is_multiple_of(12) {
            c.white_c
        } else {
            c.white
        }
    }

    fn black(&self, key: u8) -> Color32 {
        let c = self.colours;
        if self.playing == Some(key) {
            c.black_playing
        } else if self.hover == Some(key) {
            c.black_hover
        } else if self.selected(key) {
            c.black_selected
        } else {
            c.black
        }
    }

    /// The colour of a key's name on its fill.
    fn text(&self, key: u8) -> Color32 {
        let c = self.colours;
        if self.playing == Some(key) {
            c.text_playing
        } else if is_black(key) {
            c.text_black
        } else if self.selected(key) {
            c.text_selected
        } else {
            c.text
        }
    }
}

/// Whether `key`'s row is named, with `names` chosen and rows `height` points high: a folded view
/// names every row, as the keys beside each other are no longer neighbours.
pub(super) fn named(key: u8, names: PianoKeyNames, folded: bool, height: f32) -> bool {
    if height < LEAST_NAMED_ROW {
        return false;
    }
    folded
        || match names {
            PianoKeyNames::OnlyC => key.is_multiple_of(12),
            PianoKeyNames::White => !is_black(key),
            PianoKeyNames::All => true,
        }
}

/// The key column: a keyboard, with the row height's rows. Folded, the rows beside each other are
/// not neighbouring keys, so they are drawn as plain rows.
pub(super) fn draw_keys(painter: &egui::Painter, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let key_painter = painter.with_clip_rect(layout.keys);
    let colours = key_colours(state.key_style);
    let selecting = matches!(state.gesture, Some(Gesture::KeySelect { .. }));
    let states = KeyStates {
        colours: &colours,
        playing: state.playing_key(),
        hover: if selecting { None } else { state.hover_key() },
        ranges: state.key_live.into_iter().collect(),
    };
    let folded = layout.rows.folded();
    let (white_gradient, black_gradient) = (has_gradient("piano-key"), has_gradient("piano-key-black"));
    let black_right = layout.keys.left() + (layout.keys.width() * BLACK_KEY_SHARE).round();
    let edge = (layout.keys.width() * BLACK_KEY_SHARE).round() / layout.keys.width().max(1.0);
    let first = layout.key_at(layout.grid.bottom() - 0.5).unwrap_or(0);
    let last = layout.key_at(layout.grid.top() + 0.5).unwrap_or(127);
    key_painter.rect_filled(layout.keys, 0.0, colours.black);
    for key in (first..=last).filter(|k| layout.rows.shows(*k)) {
        let y = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.keys.left(), y.round()),
            Pos2::new(layout.keys.right(), (y + layout.key_height).round()),
        );
        let black = is_black(key);
        if folded {
            key_painter.rect_filled(row, 0.0, if black { states.black(key) } else { states.white(key) });
            key_painter.hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, colours.line));
        } else if black {
            // The white keys either side run on past the black key's tip, each to the row's middle.
            let middle = row.center().y.round();
            let above = Rect::from_min_max(Pos2::new(black_right, row.top()), Pos2::new(row.right(), middle));
            let below = Rect::from_min_max(Pos2::new(black_right, middle), row.max);
            key_painter.rect_filled(above, 0.0, states.white(key.saturating_add(1).min(127)));
            key_painter.rect_filled(below, 0.0, states.white(key.saturating_sub(1)));
            let tip = Rect::from_min_max(Pos2::new(black_right, row.top()), row.max);
            if white_gradient {
                sheet_gradient(&key_painter, "piano-key", tip);
            } else {
                shade(&key_painter, tip, Color32::TRANSPARENT, colours.sheen_right);
            }
            let cap = Rect::from_min_max(row.min, Pos2::new(black_right, row.bottom()));
            key_painter.rect_filled(cap, 0.0, states.black(key));
            if black_gradient {
                sheet_gradient(&key_painter, "piano-key-black", cap);
            } else {
                shade(&key_painter, cap, Color32::TRANSPARENT, colours.black_sheen);
            }
        } else {
            key_painter.rect_filled(row, 0.0, states.white(key));
            if white_gradient {
                sheet_gradient(&key_painter, "piano-key", row);
            } else {
                let split = row.left() + row.width() * edge;
                shade(
                    &key_painter,
                    Rect::from_min_max(row.min, Pos2::new(split, row.bottom())),
                    colours.sheen_left,
                    Color32::TRANSPARENT,
                );
                shade(
                    &key_painter,
                    Rect::from_min_max(Pos2::new(split, row.top()), row.max),
                    Color32::TRANSPARENT,
                    colours.sheen_right,
                );
            }
            // White keys meet only between E and F and between B and C.
            if matches!(key % 12, 0 | 5) {
                key_painter.hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, colours.line));
            }
        }
        scale_view::key_mark(&key_painter, clip.scale, key, row, colours.scale);
        if named(key, state.key_names, folded, layout.key_height) {
            let right = if black && !folded { black_right - BLACK_NAME_INSET } else { row.right() - NAME_INSET };
            key_painter.text(
                Pos2::new(right, row.center().y),
                Align2::RIGHT_CENTER,
                key_name(key),
                theme::mono_font(theme::TEXT_TINY),
                states.text(key),
            );
        }
    }
}

/// The scale strip: the octaves' names, and the name of the row under the pointer at its row.
pub(super) fn draw_strip(ui: &Ui, layout: &Layout, state: &PianoRollState) {
    let strip = layout.strip;
    if strip.width() < 1.0 {
        return;
    }
    let style = part_style("piano-scale-strip", &[]);
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, style.background.unwrap_or_else(theme::panel_background));
    painter.vline(strip.right() - 0.5, strip.y_range(), theme::hairline(style.border.unwrap_or_else(theme::border)));
    let text = style.color.unwrap_or_else(theme::text_secondary);
    let font = theme::mono_font(theme::TEXT_TINY);
    let first = layout.key_at(layout.grid.bottom() - 0.5).unwrap_or(0);
    let last = layout.key_at(layout.grid.top() + 0.5).unwrap_or(127);
    let pointer = state.hover_key();
    for key in (first..=last).filter(|k| k.is_multiple_of(12) && layout.rows.shows(*k) && pointer != Some(*k)) {
        let centre = Pos2::new(strip.center().x, layout.y_of(key) + layout.key_height / 2.0);
        painter.text(centre, Align2::CENTER_CENTER, key_name(key), font.clone(), text);
    }
    if let Some(key) = pointer.filter(|k| layout.rows.shows(*k)) {
        let centre = Pos2::new(strip.center().x, layout.y_of(key) + layout.key_height / 2.0);
        let chip = Rect::from_center_size(centre, POINTER_NAME);
        painter.rect_filled(chip, 1.0, theme::text_primary());
        let name = key_name(key);
        painter.text(centre, Align2::CENTER_CENTER, &name, font, theme::text_inverse());
        let response = ui.interact(chip.intersect(strip), Id::new("piano-roll-pointer-key"), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Row under the pointer: {name}"))
        });
    }
}

/// A drag in the scale strip: sideways it makes the rows taller (right) or lower (left), and up or
/// down it scrolls them, the row where the drag began staying under the pointer. The ruler does the
/// same the other way round.
pub(super) fn strip_zoom(ui: &Ui, strip: Rect, grid: Rect, state: &mut PianoRollState) {
    if strip.width() < 1.0 || strip.height() < 1.0 {
        return;
    }
    let response = ui.interact(strip, Id::new("piano-roll-scale-strip"), Sense::drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Row height strip"));
    if response.hovered() || response.dragged() {
        state.magnifier = true;
    }
    if response.drag_started() {
        let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(strip.center());
        let row = (from.y - grid.top() + state.scroll.y) / state.key_height;
        state.strip_drag = Some((from.x, state.key_height, row));
    }
    match (state.strip_drag, response.interact_pointer_pos()) {
        (Some((from, height, row)), Some(now)) if response.dragged() => {
            let wanted = height * ((now.x - from) * STRIP_ZOOM_PER_POINT).exp();
            state.key_height = wanted.clamp(KEY_HEIGHT_RANGE.0, KEY_HEIGHT_RANGE.1);
            state.scroll.y = row * state.key_height - (now.y - grid.top());
            state.scrolled_by_hand = true;
        }
        _ if !response.dragged() => state.strip_drag = None,
        _ => {}
    }
}

/// The pointer where a drag zooms: the windowing layer has no magnifier cursor on every system, so
/// the pointer is hidden and a magnifier is drawn where it is, as the eraser is.
pub(super) fn draw_magnifier(ui: &Ui, pos: Pos2) {
    ui.ctx().set_cursor_icon(CursorIcon::None);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("piano-roll-magnifier")));
    let (halo, line) = (theme::window_background(), theme::text_primary());
    let handle = [pos + Vec2::splat(MAGNIFIER_RADIUS * 0.7), pos + Vec2::splat(MAGNIFIER_RADIUS * 1.7)];
    painter.line_segment(handle, Stroke::new(4.0, halo));
    painter.circle_stroke(pos, MAGNIFIER_RADIUS, Stroke::new(3.5, halo));
    painter.circle_stroke(pos, MAGNIFIER_RADIUS, Stroke::new(1.5, line));
    painter.line_segment(handle, Stroke::new(2.0, line));
    let cross = MAGNIFIER_RADIUS * 0.5;
    painter.hline(pos.x - cross..=pos.x + cross, pos.y, Stroke::new(1.0, line));
    painter.vline(pos.x, pos.y - cross..=pos.y + cross, Stroke::new(1.0, line));
}

/// The range a drag on the keys is selecting, across the note rows: a tint with dashed edges at its
/// top and bottom. Once let go it is the notes that show what is selected.
pub(super) fn draw_ranges(painter: &egui::Painter, layout: &Layout, state: &PianoRollState) {
    let Some((low, high)) = state.key_live else { return };
    let style = part_style("piano-range", &[]);
    let edge = style.border.unwrap_or_else(theme::accent);
    let tint = style.background.unwrap_or_else(|| theme::accent().gamma_multiply(RANGE_TINT));
    let tops = (low..=high).filter(|k| layout.rows.shows(*k)).map(|k| layout.y_of(k));
    let (top, bottom) = tops.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
    if !top.is_finite() {
        return;
    }
    let bottom = bottom + layout.key_height;
    let rect = Rect::from_min_max(Pos2::new(layout.grid.left(), top), Pos2::new(layout.grid.right(), bottom));
    painter.rect_filled(rect, 0.0, tint);
    for y in [top + 0.5, bottom - 0.5] {
        let line = [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)];
        painter.extend(egui::Shape::dashed_line(&line, Stroke::new(1.0, edge), 4.0, 3.0));
    }
}

/// The menu in the corner over the keys: how the keys look and name themselves, and whether notes
/// of a key may overlap. What is picked is kept in the settings by the window.
pub(super) fn view_menu(ui: &mut Ui, corner: Rect, state: &mut PianoRollState) {
    if corner.width() < 1.0 || corner.height() < 1.0 {
        return;
    }
    let font = theme::ui_font(theme::TEXT_SMALL);
    let theme_keys = theme_sets_keys();
    let mut picked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(corner), |ui| {
        let text = egui::RichText::new("View   ").font(font.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            let heading = |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).font(font.clone()).color(theme::text_secondary()));
            };
            heading(ui, "Key style");
            if theme_keys {
                let theme_label = egui::RichText::new("Theme").font(font.clone());
                let response = ui.selectable_label(true, theme_label);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, true, "Key style: Theme")
                });
            }
            for style in PianoKeyStyle::ALL {
                let on = !theme_keys && state.key_style == style;
                let response = ui.add_enabled(
                    !theme_keys,
                    egui::Button::selectable(on, egui::RichText::new(style.name()).font(font.clone())),
                );
                let name = format!("Key style: {}", style.name());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name));
                if response.clicked() {
                    picked = Some(ViewPick::KeyStyle(style));
                    ui.close();
                }
            }
            ui.separator();
            heading(ui, "Key names");
            for names in PianoKeyNames::ALL {
                let on = state.key_names == names;
                let response = ui.selectable_label(on, egui::RichText::new(names.name()).font(font.clone()));
                let name = format!("Key names: {}", names.name());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name));
                if response.clicked() {
                    picked = Some(ViewPick::KeyNames(names));
                    ui.close();
                }
            }
            ui.separator();
            let stacked = !state.notes_replace;
            let response = ui.selectable_label(stacked, egui::RichText::new("Allow stacked notes").font(font.clone()));
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, stacked, "Allow stacked notes")
            });
            if response.clicked() {
                picked = Some(ViewPick::AllowStacked(!stacked));
                ui.close();
            }
        });
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Piano roll view"));
        menu.response.on_hover_text("How the keys look, which keys are named, and whether notes may overlap");
    });
    if picked.is_some() {
        state.view_picked = picked;
    }
}

impl PianoRollState {
    /// Starts a selection from the key column at `key`: `add` keeps what is selected.
    pub(super) fn begin_key_selection(&mut self, key: u8, add: bool) {
        self.key_base = if add { self.selected.clone() } else { BTreeSet::new() };
        self.key_live = Some((key, key));
    }

    /// The selection while a drag in the key column runs from `anchor` to `at`: what it began
    /// with, and every note of the keys between the two that the rows show.
    pub(super) fn select_key_range(&mut self, clip: &PianoClip, rows: &KeyRows, anchor: u8, at: u8) {
        let (low, high) = (anchor.min(at), anchor.max(at));
        self.key_live = Some((low, high));
        self.selected = self.key_base.clone();
        for (index, note) in clip.notes.iter().enumerate() {
            if (low..=high).contains(&note.key) && rows.shows(note.key) {
                self.selected.insert(index);
            }
        }
    }

    /// Ends a drag in the key column: what it selected stays selected, to add to. The keys already
    /// sounded as the drag reached them.
    pub(super) fn end_key_selection(&mut self) {
        self.key_live = None;
    }

    /// The rectangle a drag is selecting with is at `rect`: the notes it has just reached sound at
    /// once, each key once.
    pub(super) fn audition_swept(&mut self, layout: &Layout, clip: &PianoClip, rect: Rect) {
        let inside: BTreeSet<usize> = clip
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| layout.note_rect(n).intersects(rect))
            .map(|(i, _)| i)
            .collect();
        let reached: Vec<u8> =
            inside.difference(&self.swept).filter_map(|i| clip.notes.get(*i)).map(|n| n.key).collect();
        self.audition_keys(reached);
        self.swept = inside;
    }

    /// Whether notes this command places replace what is under them (Allow stacked notes off).
    pub(crate) fn replacing(&self, params: &mut Value) {
        if self.notes_replace {
            params["replace"] = json!(true);
        }
    }

    /// `note.add` for `notes`, under the rule for stacked notes.
    pub(super) fn add_action(&self, clip: &PianoClip, notes: &[Note]) -> Action {
        let mut params = add_params(clip, notes);
        self.replacing(&mut params);
        Action::new("note.add", params)
    }

    /// The notes at `replaced` are gone (a command placed notes over them): the selection stays on
    /// the same notes as the indices close up.
    pub fn notes_replaced(&mut self, replaced: &[usize]) {
        let moved = |index: usize| index - replaced.partition_point(|r| *r < index);
        self.selected =
            self.selected.iter().filter(|i| replaced.binary_search(i).is_err()).map(|i| moved(*i)).collect();
        self.click_added = self.click_added.map(moved);
    }
}
