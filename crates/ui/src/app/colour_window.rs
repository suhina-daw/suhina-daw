// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The owned colour tool window. Commands remain on the active project's main action path;
//! the native viewport has its own pointer, keyboard and close lifetime.

use super::*;
use crate::interaction::colour_picker::{
    hex,
    panel::{self, Edit, Panel},
};
use suhina_engine::model::Rgb;

const WIDTH: f32 = 340.0;

#[derive(Default)]
pub(super) struct ColourWindow {
    open: bool,
    project: Option<ProjectId>,
    tracks: Vec<TrackId>,
    selection_seen: Vec<TrackId>,
    panel: Panel,
    position: Option<Pos2>,
    height: f32,
    focus_requested: bool,
    focused: bool,
    minimised: bool,
    embedded_layer: Option<egui::LayerId>,
    pub pending_recent: Option<Rgb>,
}

impl App {
    pub(super) fn preview_colour_targets(&self, ctx: &egui::Context) {
        let tracks = if self.colour_window.open && self.projects.active() == self.colour_window.project {
            self.colour_window.tracks.as_slice()
        } else {
            &[]
        };
        crate::kit::selection::preview_tracks(ctx, tracks);
    }

    /// Only the embedded tool's own controls keep normal widget keyboard navigation.
    /// The rest of the DAW continues to use its existing shortcut and launch-key rules.
    pub(super) fn colour_window_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.colour_window.open && self.colour_window.embedded_layer.is_some_and(|layer| focused_in_layer(ctx, layer))
    }

    pub(super) fn colour_history_changed(&mut self) {
        if self.colour_window.pending_recent.take().is_some() {
            self.colour_window.panel.cancel_pointer();
            if self.open_gesture.as_deref() == Some(panel::GESTURE) {
                self.open_gesture = None;
            }
        }
    }

    pub(super) fn open_colour_window(&mut self, tracks: Option<Vec<TrackId>>) {
        let selection = self.focused_tracks();
        let tracks = tracks.unwrap_or_else(|| selection.clone());
        let Some(session) = self.session() else { return };
        if tracks.is_empty() || tracks.iter().any(|t| session.project().track(*t).is_none()) {
            self.status = Status::Hint("Select a track to choose its colour".into());
            return;
        }
        self.finish_colour_gesture(false);
        self.colour_window.panel.discard_drafts();
        if !self.colour_window.open {
            self.colour_window.position = None;
        }
        self.colour_window.open = true;
        self.colour_window.project = self.projects.active();
        self.colour_window.tracks = tracks;
        self.colour_window.selection_seen = selection;
        self.colour_window.focus_requested = true;
    }

    pub(super) fn run_colour_window_action(&mut self, action: &Action) -> bool {
        match action.id {
            "ui.colourPicker" => {
                let tracks = action.params["tracks"].as_array().map(|list| {
                    list.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).map(TrackId)).collect()
                });
                self.open_colour_window(tracks);
            }
            "ui.finishColour" => self.finish_colour_gesture(false),
            "ui.cancelColour" => self.finish_colour_gesture(true),
            _ => return false,
        }
        true
    }

    fn finish_colour_gesture(&mut self, cancel: bool) {
        let project = self.colour_window.project;
        let ended = self
            .projects
            .session_mut(project)
            .ok()
            .and_then(|session| session.execute("history.endGesture", &json!({ "gesture": panel::GESTURE })).ok())
            .is_some_and(|value| value["ended"].as_bool() == Some(true));
        self.colour_window.panel.dragging = false;
        if self.open_gesture.as_deref() == Some(panel::GESTURE) {
            self.open_gesture = None;
        }
        let pending = self.colour_window.pending_recent.take();
        if cancel && ended && self.projects.active() == project {
            // endGesture is conditional on its id: an intervening edit is never undone.
            self.run(&Action::new("edit.undo", json!({})));
        } else if let Some(colour) = pending {
            panel::remember(&mut self.settings.recent_colours, colour);
            self.settings_changed = true;
        }
    }

    pub(super) fn show_colour_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // Menus show the user's binding, including an unbound or reassigned F2.
        let shortcut = self.keymap.label("selection.colour").unwrap_or_default();
        ctx.data_mut(|d| d.insert_temp(Id::unique("colour-picker-shortcut"), shortcut));
        if !self.colour_window.open {
            return;
        }
        if self.projects.active() != self.colour_window.project {
            self.finish_colour_gesture(false);
            self.colour_window.open = false;
            self.settings_changed = true;
            return;
        }
        let selection = self.focused_tracks();
        let targets_removed = self.session().is_some_and(|s| {
            !self.colour_window.tracks.is_empty()
                && self.colour_window.tracks.iter().all(|id| s.project().track(*id).is_none())
        });
        if targets_removed {
            // Deletion may automatically focus the next track. Do not silently colour it;
            // a subsequent header selection or reopening the picker chooses a new target.
            self.finish_colour_gesture(false);
            self.colour_window.tracks.clear();
            self.colour_window.selection_seen = selection.clone();
        } else if !selection.is_empty() && selection != self.colour_window.selection_seen {
            self.finish_colour_gesture(false);
            self.colour_window.panel.discard_drafts();
            self.colour_window.selection_seen = selection.clone();
            self.colour_window.tracks = selection;
        }
        let Some(session) = self.projects.active().and_then(|id| self.projects.get(id)) else { return };
        let project = session.project();
        self.colour_window.tracks.retain(|t| project.track(*t).is_some());
        let first = self
            .colour_window
            .tracks
            .first()
            .and_then(|id| project.tracks.iter().enumerate().find(|(_, t)| t.id == *id));
        let current = first.map(|(place, t)| suhina_session::colours::track_colour(t.color, place));
        let title = match (self.colour_window.tracks.len(), first) {
            (1, Some((_, track))) => format!("Track colour · {}", track.name),
            (0, _) => "Track colour · Select a track".to_string(),
            (count, _) => format!("Track colour · {count} tracks"),
        };
        if let Some(colour) = current {
            self.colour_window.panel.sync(colour);
        }
        let (main_focused, main_minimised, main_rect) = ctx.input_for(egui::ViewportId::ROOT, |i| {
            (
                i.viewport().focused.unwrap_or(false),
                i.viewport().minimized.unwrap_or(false),
                i.viewport().outer_rect.or(i.viewport().inner_rect),
            )
        });
        let bounds = main_rect.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0)));
        let height = (if self.colour_window.panel.expanded { 720.0_f32 } else { 400.0 })
            .min((bounds.height() - 48.0).max(180.0));
        if self.colour_window.height != height {
            // Expanding near the bottom must keep the new controls on screen. Ordinary
            // moves are left alone; only a size change rechecks the available bounds.
            self.colour_window.position =
                self.colour_window.position.map(|at| place(bounds, Vec2::new(WIDTH, height + 32.0), Some(at)));
            self.colour_window.height = height;
        }
        let position = *self.colour_window.position.get_or_insert_with(|| {
            let proposed = self.settings.colour_window_rect.map(|[x, y, _, _]| Pos2::new(x as f32, y as f32));
            place(bounds, Vec2::new(WIDTH, height + 32.0), proposed)
        });
        let id = egui::ViewportId::from_hash_of("track-colour-window");
        let builder = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_position(position)
            .with_inner_size([WIDTH, height])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_window_level(browser_panel::map_window_level(main_focused, self.colour_window.focused));
        let enabled = current.is_some() && !self.dialog_holds_actions();
        let recent = &self.settings.recent_colours;
        let panel = &mut self.colour_window.panel;
        let keymap = &self.keymap;
        let mut edits = Vec::new();
        let mut keys = Vec::new();
        let mut close = false;
        let mut focused = false;
        let mut placement = None;
        let mut embedded_layer = None;
        let mut render = |ui: &mut Ui, embedded: bool| {
            if embedded {
                embedded_layer = Some(ui.layer_id());
            }
            {
                let style = crate::kit::element::COLOUR_PICKER.enter();
                if !embedded && suhina_plugins::trace::on() {
                    suhina_plugins::trace::state(
                        "colour window surface",
                        format!(
                            "rect {:?}, clip {:?}, scale {}, fill {:?}, authored {:?}, geometry {:?}",
                            ui.max_rect(),
                            ui.clip_rect(),
                            ui.ctx().pixels_per_point(),
                            style.background(),
                            style.style().map(|s| (s.background, s.authored.background, s.opacity)),
                            ui.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect)),
                        ),
                    );
                }
                let inside = style.paint(ui.painter(), ui.max_rect()).content;
                if !embedded && suhina_plugins::trace::on() {
                    let mut fills = Vec::new();
                    ui.painter().for_each_shape(|shape| {
                        if let egui::Shape::Rect(fill) = &shape.shape {
                            fills.push((fill.rect, fill.fill, shape.clip_rect));
                        }
                    });
                    suhina_plugins::trace::state("colour window painted surface", format!("{fills:?}"));
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(inside), |ui| {
                    let mut styled = style.ui(ui);
                    egui::ScrollArea::vertical().show(&mut styled, |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            edits.extend(panel.show(ui, recent));
                        });
                    });
                });
            }
            if !embedded {
                close = ui.input(|i| i.viewport().close_requested());
            }
            // Widget navigation and activation belong to the tool. Only transport, Undo,
            // Redo and reopening the picker are global here; text drafts own every key.
            let owns_keyboard = focused_in_layer(ui.ctx(), ui.layer_id());
            if enabled && (!embedded || owns_keyboard) && !ui.ctx().text_edit_focused() {
                keys = ui.input_mut(|i| {
                    if owns_keyboard {
                        i.events.retain(|event| !matches!(event,
                            egui::Event::Key { key, modifiers, .. }
                                if (modifiers.is_none() || (*key == egui::Key::Tab && *modifiers == egui::Modifiers::SHIFT))
                                    && matches!(key, egui::Key::Tab | egui::Key::Enter | egui::Key::Space
                                        | egui::Key::Escape | egui::Key::ArrowLeft | egui::Key::ArrowRight
                                        | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home
                                        | egui::Key::End | egui::Key::PageUp | egui::Key::PageDown)
                        ));
                    }
                    keymap.pressed(i, |id| {
                        id.starts_with("transport.") || matches!(id, "edit.undo" | "edit.redo" | "selection.colour")
                    })
                });
            }
            ui.input(|i| {
                focused = i.viewport().focused.unwrap_or(false);
                if !embedded {
                    placement = i.viewport().outer_rect.zip(i.viewport().inner_rect).map(|(outer, inner)| {
                        [
                            outer.left().round() as i32,
                            outer.top().round() as i32,
                            inner.width().round() as i32,
                            inner.height().round() as i32,
                        ]
                    });
                }
            });
        };
        if ctx.embed_viewports() {
            // egui 0.37 already wraps an embedded viewport in a Window. Own that
            // fallback here instead, to provide closing and changing compact/fine size
            // without nesting one floating Window inside another.
            let mut open = true;
            egui::Window::new(&title)
                .id(Id::unique("track-colour-window"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size([WIDTH, height])
                .default_pos(ctx.content_rect().center() - Vec2::new(WIDTH, height) * 0.5)
                .show(ctx, |ui| render(ui, true));
            close = !open;
        } else {
            ctx.show_viewport_immediate(id, builder, |ui, class| {
                render(ui, class == egui::ViewportClass::EmbeddedWindow);
            });
        }
        self.colour_window.focused = focused;
        self.colour_window.embedded_layer = embedded_layer;
        if self.colour_window_owns_keyboard(ctx) || (!ctx.embed_viewports() && focused) {
            // Key-up may go to this window instead of the DAW. End held notes and
            // Gate/Repeat launches at the ownership change, even before a key-up arrives.
            self.release_typing_keys();
            if let Some(project) = self.projects.active().and_then(|id| self.projects.get(id)).map(Session::project) {
                self.view.launch_keys.release_all(project, actions);
            }
        }
        if let Some(rect) = placement
            && self.settings.colour_window_rect != Some(rect)
        {
            self.settings.colour_window_rect = Some(rect);
            // Keep live placement in memory; persist once on close, not on every move frame.
            self.colour_window.position = Some(Pos2::new(rect[0] as f32, rect[1] as f32));
        }
        if self.colour_window.focus_requested {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
            self.colour_window.focus_requested = false;
        }
        if let Some(minimise) = browser_panel::follow_main_minimised(main_minimised, self.colour_window.minimised) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Minimized(minimise));
            self.colour_window.minimised = minimise;
        }
        let tracks: Vec<u32> = self.colour_window.tracks.iter().map(|t| t.0).collect();
        for edit in edits {
            match edit {
                Edit::Colour(colour, dragging) => {
                    let unchanged = self.session().is_some_and(|s| {
                        s.project()
                            .tracks
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| tracks.contains(&t.id.0))
                            .all(|(place, t)| suhina_session::colours::track_colour(t.color, place) == colour)
                    });
                    if unchanged {
                        continue;
                    }
                    let mut params = json!({ "tracks": tracks, "color": hex(colour) });
                    if dragging {
                        params["gesture"] = json!(panel::GESTURE);
                    }
                    actions.push(Action::new("track.setColor", params));
                }
                Edit::Default => {
                    actions.push(Action::new("track.setColor", json!({ "tracks": tracks, "color": "none" })))
                }
                Edit::Finish => actions.push(Action::new("ui.finishColour", json!({}))),
                Edit::Cancel => actions.push(Action::new("ui.cancelColour", json!({}))),
            }
        }
        for key in keys {
            self.do_action(&key, actions);
        }
        if close {
            actions.push(Action::new("ui.finishColour", json!({})));
            self.colour_window.open = false;
            self.settings_changed = true;
        }
    }
}

fn focused_in_layer(ctx: &egui::Context, layer: egui::LayerId) -> bool {
    ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| r.layer_id == layer)
}

/// A new or remembered window stays on the current screen even after a monitor is removed.
fn place(bounds: Rect, size: Vec2, remembered: Option<Pos2>) -> Pos2 {
    let proposed = remembered.filter(|p| p.x.is_finite() && p.y.is_finite()).unwrap_or(bounds.center() - size * 0.5);
    Pos2::new(
        proposed.x.clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left())),
        proposed.y.clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_placement_centres_and_expansion_or_stale_positions_stay_inside_bounds() {
        let bounds = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(1280.0, 900.0));
        let compact = Vec2::new(WIDTH, 432.0);
        let centre = place(bounds, compact, None);
        assert_eq!(centre + compact * 0.5, bounds.center());
        let expanded = Vec2::new(WIDTH, 752.0);
        for remembered in [centre, Pos2::new(-3000.0, 2000.0), Pos2::new(f32::NAN, 0.0)] {
            let rect = Rect::from_min_size(place(bounds, expanded, Some(remembered)), expanded);
            assert!(bounds.contains_rect(rect));
        }
    }
}
