// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's right-click menu (`docs/spec/ui-clip-editor.md` › Right-click menu): what the
//! toolbar, the clip panel and the keys can do, in one menu at the pointer. Every item runs a
//! command or a UI action that already has a key or a control; Invert selection is the one that
//! is new. A right click opens it wherever the right button does nothing else.

use egui::containers::menu::menu_style;
use egui::{Layout as EguiLayout, PopupKind, SetOpenCommand};
use suhina_engine::model::{ROOT_NAMES, ScaleMode};

use super::*;
use crate::detail::piano_roll_ruler::Part as RulerPart;

/// Where the menu was opened: the point on the screen, the beat under it, and the note it was
/// opened on, if any. `opened` once the popup has been told to open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MenuAt {
    pub(super) pos: Pos2,
    pub(super) beat: f64,
    pub(super) note: Option<usize>,
    pub(super) opened: bool,
}

/// What a right click in the grid opens, read once a frame after the right button's own gestures.
pub(super) struct RightClick<'a> {
    pub(super) layout: &'a Layout,
    pub(super) clip: &'a PianoClip,
    pub(super) inputs: Inputs,
    pub(super) workflow: Workflow,
    /// The right button deletes a note here (the Draw and Paint tools with Draw-first, or the
    /// Mouse-first switch): on a note it goes on deleting, and only the empty grid opens the menu.
    pub(super) right_deletes: bool,
}

impl RightClick<'_> {
    /// A right click let go without a drag, and without the wheel turned while it was held,
    /// opens the menu at once; on a ghost note it waits for the double-click time, as two quick
    /// right clicks there go into that clip and show no menu. Alt (the right button plays) and
    /// Shift (it splits) keep their gestures. A click on a note that is not selected selects it
    /// alone first, so the menu acts on what was clicked.
    pub(super) fn handle(&self, ui: &Ui, response: &egui::Response, pointer: Option<Pos2>, state: &mut PianoRollState) {
        let now = ui.input(|i| i.time);
        if response.double_clicked_by(PointerButton::Secondary) {
            state.menu_waiting = None;
            return;
        }
        if let Some((at, menu)) = state.menu_waiting {
            if now >= at {
                state.menu_waiting = None;
                state.menu = Some(menu);
            } else {
                ui.ctx().request_repaint_after_secs((at - now) as f32);
            }
        }
        if !response.clicked_by(PointerButton::Secondary)
            || state.right_stepped
            || state.right_playing
            || (self.workflow.alt_right_plays && self.inputs.alt)
            || (self.workflow.shift_right_splits && self.inputs.shift)
        {
            return;
        }
        let Some(pos) = pointer else { return };
        let over = note_at(self.layout, &self.clip.notes, pos).map(|(index, _)| index);
        if over.is_some() && self.right_deletes {
            return;
        }
        if let Some(index) = over
            && !state.selected.contains(&index)
        {
            state.selected = BTreeSet::from([index]);
            state.clear_span();
        }
        let menu = MenuAt { pos, beat: self.layout.beat_at(pos.x).max(0.0), note: over, opened: false };
        let ghost = over.is_none()
            && ghost_view::ghost_at(&self.clip.ghosts, pos, |g| self.layout.note_rect(&g.note())).is_some();
        if ghost {
            state.menu_waiting = Some((now + DOUBLE_CLICK_TIME, menu));
            ui.ctx().request_repaint_after_secs(DOUBLE_CLICK_TIME as f32);
        } else {
            state.menu_waiting = None;
            state.menu = Some(menu);
        }
    }
}

/// An item of the menu picked this frame, made an action once the menu is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Pick {
    /// One of the keys' actions (`selection.cut`, `pianoRoll.zoomClip` and the others), run as the
    /// key runs it.
    Key(&'static str),
    Delete,
    InvertSelection,
    QuantizeSettings,
    Mute(bool),
    Split,
    LoopSelection,
    Marker(RulerPart),
    Grid(PianoGrid),
    Fold,
    ScaleRows,
    Highlight,
    SetScale(Option<(usize, ScaleMode)>),
}

/// What the menu needs for one frame besides the state.
pub(super) struct MenuCx<'a> {
    pub(super) clip: &'a PianoClip,
    pub(super) keymap: &'a crate::settings::keymap::Keymap,
    pub(super) copied: Option<usize>,
    pub(super) grid: TimelineGrid,
}

/// The menu's accessible name, for a reader and a test.
pub(super) const MENU_NAME: &str = "Piano roll menu";

impl MenuCx<'_> {
    /// The menu, while it is open; the item picked is run here: one command (one undo step) or
    /// one change to the view.
    pub(super) fn show(&self, ui: &Ui, state: &mut PianoRollState, actions: &mut Vec<Action>) {
        let Some(at) = state.menu else { return };
        let id = Id::unique("piano-roll-context-menu");
        let open = (!at.opened).then_some(SetOpenCommand::Bool(true));
        let mut picked = None;
        let shown = egui::Popup::new(id, ui.ctx().clone(), at.pos, ui.layer_id())
            .kind(PopupKind::Menu)
            .layout(EguiLayout::top_down_justified(egui::Align::Min))
            .style(menu_style)
            .accessible_name(MENU_NAME)
            .open_memory(open)
            .show(|ui| picked = self.entries(ui, state, at));
        if shown.is_none() && at.opened {
            state.menu = None;
            return;
        }
        if let Some(menu) = &mut state.menu {
            menu.opened = true;
        }
        if let Some(pick) = picked {
            egui::Popup::close_id(ui.ctx(), id);
            state.menu = None;
            actions.extend(self.run(pick, at, state));
        }
    }

    /// The items, each with its key, greyed out where there is nothing to act on.
    fn entries(&self, ui: &mut Ui, state: &PianoRollState, at: MenuAt) -> Option<Pick> {
        let clip = self.clip;
        let font = theme::ui_font(theme::TEXT_SMALL);
        let acting = !state.acting().is_empty();
        let notes = !clip.notes.is_empty();
        let mut picked = None;
        let mut item = |ui: &mut Ui, name: &str, key: Option<&str>, enabled: bool, pick: Pick| {
            let mut button = egui::Button::new(egui::RichText::new(name).font(font.clone()));
            if let Some(text) = key.and_then(|k| self.keymap.label(k)) {
                button = button.shortcut_text(egui::RichText::new(text).font(font.clone()));
            }
            let response = ui.add_enabled(enabled, button);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
            if response.clicked() {
                picked = Some(pick);
                ui.close();
            }
        };
        item(ui, "Cut", Some("selection.cut"), acting, Pick::Key("selection.cut"));
        item(ui, "Copy", Some("selection.copy"), acting, Pick::Key("selection.copy"));
        let pastes = self.copied.is_some_and(|n| n > 0);
        item(ui, "Paste", Some("selection.paste"), pastes, Pick::Key("selection.paste"));
        item(ui, "Duplicate", Some("selection.duplicate"), acting, Pick::Key("selection.duplicate"));
        item(ui, "Delete", Some("selection.delete"), acting, Pick::Delete);
        ui.separator();
        item(ui, "Select all", Some("pianoRoll.selectAll"), notes, Pick::Key("pianoRoll.selectAll"));
        item(ui, "Invert selection", None, notes, Pick::InvertSelection);
        ui.separator();
        let quantizes = notes && !(state.acting().is_empty() && state.span.is_some());
        item(ui, "Quantize", Some("pianoRoll.quantize"), quantizes, Pick::Key("pianoRoll.quantize"));
        item(ui, "Quantize settings…", None, true, Pick::QuantizeSettings);
        let muted = self.acting_notes(state).all(|n| n.muted);
        let (word, mute) = if acting && muted { ("Unmute", false) } else { ("Mute", true) };
        item(ui, word, None, acting, Pick::Mute(mute));
        item(ui, "Split here", None, !self.split_notes(state, at).is_empty(), Pick::Split);
        ui.separator();
        item(ui, "Loop selection", None, self.selection_span(state).is_some(), Pick::LoopSelection);
        let shape = clip.shape;
        item(ui, "Set start here", None, true, Pick::Marker(RulerPart::StartMarker));
        item(ui, "Set end here", None, shape.loop_range.is_none(), Pick::Marker(RulerPart::EndMarker));
        item(ui, "Set loop start here", None, true, Pick::Marker(RulerPart::LoopStart));
        item(ui, "Set loop end here", None, true, Pick::Marker(RulerPart::LoopEnd));
        ui.separator();
        item(ui, "Zoom to selection", Some("pianoRoll.zoomSelection"), true, Pick::Key("pianoRoll.zoomSelection"));
        item(ui, "Zoom to clip", Some("pianoRoll.zoomClip"), true, Pick::Key("pianoRoll.zoomClip"));
        ui.separator();
        // The grid's entries right in the menu, as the toolbar's grid menu holds them: no submenu
        // to open first.
        let keys = grid_key_labels(self.keymap, &PIANO_GRID_KEYS);
        if let Some(grid) = grid_entries(ui, clip.grid_menu, &keys, &|_| true) {
            picked = Some(Pick::Grid(grid));
            ui.close();
        }
        ui.separator();
        let toggle = |ui: &mut Ui, name: &str, on: bool| {
            let response = ui.add(egui::Button::selectable(on, egui::RichText::new(name).font(font.clone())));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, name));
            if response.clicked() {
                ui.close();
            }
            response.clicked()
        };
        if toggle(ui, "Fold", state.fold == Fold::Notes) {
            picked = Some(Pick::Fold);
        }
        if toggle(ui, "Fold to scale", state.fold == Fold::Scale && clip.scale.is_some()) {
            picked = Some(Pick::ScaleRows);
        }
        if toggle(ui, "Highlight scale", state.highlight_scale) {
            picked = Some(Pick::Highlight);
        }
        let scale_name = clip.scale.map_or("Off".to_string(), |s| format!("{} {}", s.root_name(), s.mode.label()));
        let scale_label = format!("Scale ({scale_name})");
        let response = ui.menu_button(egui::RichText::new("Scale").font(font.clone()), |ui| {
            if ui.add_enabled(clip.scale.is_some(), egui::Button::new("Off")).clicked() {
                picked = Some(Pick::SetScale(None));
                ui.close();
            }
            for mode in ScaleMode::ALL {
                ui.menu_button(mode.label(), |ui| {
                    for index in 0..ROOT_NAMES.len() {
                        let root = pitch_class_name(u8::try_from(index).unwrap_or(0));
                        let on = clip.scale.is_some_and(|s| usize::from(s.root()) == index && s.mode == mode);
                        if ui.add(egui::Button::selectable(on, root)).clicked() {
                            picked = Some(Pick::SetScale(Some((index, mode))));
                            ui.close();
                        }
                    }
                });
            }
        });
        response.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &scale_label));
        picked
    }

    fn acting_notes<'s>(&'s self, state: &'s PianoRollState) -> impl Iterator<Item = &'s Note> + 's {
        state.acting().iter().filter_map(|i| self.clip.notes.get(*i))
    }

    /// The beat a menu item acts at: the clicked point, on the grid (Alt free is for drags).
    fn beat(&self, at: MenuAt) -> f64 {
        self.grid.place(at.beat, false, false).clamp(0.0, f64::from(BEAT_LIMIT))
    }

    /// The notes Split here cuts: those the edit keys act on (the clicked one, which the click
    /// selected) that the clicked point falls inside.
    fn split_notes(&self, state: &PianoRollState, at: MenuAt) -> Vec<usize> {
        let beat = self.beat(at);
        state
            .acting()
            .iter()
            .copied()
            .filter(|i| {
                self.clip.notes.get(*i).is_some_and(|n| n.time + EPSILON < beat && beat < n.time + n.duration - EPSILON)
            })
            .collect()
    }

    /// What Loop selection loops over: the time selection, else the selected notes' span.
    fn selection_span(&self, state: &PianoRollState) -> Option<(f64, f64)> {
        if let Some(span) = &state.span {
            return Some((round_beat(span.start), round_beat(span.end))).filter(|(s, e)| e > s);
        }
        let chosen = || state.selected.iter().filter_map(|i| self.clip.notes.get(*i));
        let start = chosen().map(|n| n.time).fold(f64::INFINITY, f64::min);
        let end = chosen().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
        (start.is_finite() && end > start).then(|| (round_beat(start), round_beat(end)))
    }

    /// The action an item runs, or the change it makes to the view (none then).
    pub(super) fn run(&self, pick: Pick, at: MenuAt, state: &mut PianoRollState) -> Option<Action> {
        // The click on the item landed on the menu, over the piano roll, which took the keyboard
        // from it: the menu is the piano roll's, so it keeps the keyboard and its keys' actions run.
        state.focused = true;
        let clip = self.clip;
        let mut params = clip.address.clone();
        match pick {
            Pick::Key("pianoRoll.quantize") => state.quantize_selected(),
            Pick::Key(action) => state.key_action(action, clip, 0.0, self.copied),
            Pick::Delete => {
                params["notes"] = json!(state.acting().iter().collect::<Vec<_>>());
                state.selected.clear();
                state.span_notes.clear();
                state.settling = None;
                Some(Action::new("note.delete", params))
            }
            Pick::InvertSelection => {
                state.invert_selection(clip);
                None
            }
            Pick::QuantizeSettings => {
                state.set_quantize_open(true);
                None
            }
            Pick::Mute(muted) => {
                params["notes"] = json!(state.acting().iter().collect::<Vec<_>>());
                params["muted"] = json!(muted);
                Some(Action::new("note.setMuted", params))
            }
            Pick::Split => {
                let notes = self.split_notes(state, at);
                params["notes"] = json!(notes);
                params["at"] = json!(round_beat(self.beat(at)));
                Some(Action::new("note.split", params))
            }
            Pick::LoopSelection => {
                let (start, end) = self.selection_span(state)?;
                params["start"] = json!(start);
                params["end"] = json!(end);
                Some(Action::new("clip.setLoop", params))
            }
            Pick::Marker(part) => {
                let shape = clip.shape;
                let from = match part {
                    RulerPart::StartMarker => shape.offset,
                    RulerPart::EndMarker => shape.offset + shape.length,
                    RulerPart::LoopStart => shape.bar().0,
                    RulerPart::LoopEnd | RulerPart::Loop => shape.bar().1,
                };
                let after = ruler_view::dragged(&shape, part, from, at.beat, self.grid, false);
                ruler_view::command(&clip.address, &shape, &after, part)
            }
            Pick::Grid(grid) => {
                state.grid_picked = Some(grid);
                None
            }
            Pick::Fold => {
                state.fold = if state.fold == Fold::Notes { Fold::Off } else { Fold::Notes };
                None
            }
            Pick::ScaleRows => {
                let scaled = state.fold == Fold::Scale && clip.scale.is_some();
                state.fold = if scaled { Fold::Off } else { Fold::Scale };
                (!scaled && clip.scale.is_none()).then(|| {
                    let mut params = clip.address.clone();
                    let (root, mode) = default_scale();
                    let root = ROOT_NAMES.get(usize::from(root)).copied().unwrap_or("C");
                    params["root"] = json!(root);
                    params["mode"] = json!(mode.name());
                    Action::new("clip.setScale", params)
                })
            }
            Pick::Highlight => {
                state.highlight_scale = !state.highlight_scale;
                state.view_picked = Some(ViewPick::HighlightScale(state.highlight_scale));
                None
            }
            Pick::SetScale(None) => {
                params["off"] = json!(true);
                Some(Action::new("clip.setScale", params))
            }
            Pick::SetScale(Some((root, mode))) => {
                params["root"] = json!(ROOT_NAMES.get(root).copied().unwrap_or("C"));
                params["mode"] = json!(mode.name());
                Some(Action::new("clip.setScale", params))
            }
        }
    }
}

impl PianoRollState {
    /// Invert selection: the notes that are not selected become the selection, and no others.
    pub(super) fn invert_selection(&mut self, clip: &PianoClip) {
        let was = std::mem::take(&mut self.selected);
        self.selected = (0..clip.notes.len()).filter(|i| !was.contains(i)).collect();
        self.clear_span();
    }

    /// Whether the right-click menu is open (or waiting for the double-click time on a ghost note).
    pub fn menu_open(&self) -> bool {
        self.menu.is_some() || self.menu_waiting.is_some()
    }
}
