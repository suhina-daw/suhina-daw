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
/// How many frames a taller layout waits for the window to grow to it.
const MAX_WAIT: u8 = 12;

#[derive(Default)]
pub(super) struct ColourWindow {
    open: bool,
    project: Option<ProjectId>,
    tracks: Vec<TrackId>,
    selection_seen: Vec<TrackId>,
    panel: Panel,
    // A stable opening position, not the last reported native position. eframe reads geometry
    // before applying queued moves, so feeding observations back here oscillates.
    initial_position: Option<Pos2>,
    height: f32,
    /// How tall the window was when last drawn: in a window of its own the system applies a new
    /// size a frame or more after it is asked for.
    shown: f32,
    /// Frames a taller layout has waited for the window to reach its size.
    waited: u8,
    /// How much taller the size asked of the window is than the panel in it, as last measured:
    /// the dialog's padding, and its border and title row embedded, or the system's title bar in
    /// a window of its own.
    chrome: f32,
    focus_requested: bool,
    focused: bool,
    minimised: bool,
    embedded_layer: Option<egui::LayerId>,
    pub pending_recent: Option<Rgb>,
}

/// Whether the layout the user asked for, `asked` tall where the one drawn is `now` tall, is
/// drawn this frame: a shorter one at once; a taller one, which needs a window `target` tall,
/// once the window is asked to be that tall and was drawn that tall (`(asked for, drawn)`), or
/// after waiting [`MAX_WAIT`] frames for a system that will not make it so.
fn settles(now: Option<f32>, asked: Option<f32>, target: f32, (requested, shown): (f32, f32), waited: u8) -> bool {
    asked <= now || (requested >= target - 0.5 && shown >= target - 0.5) || waited >= MAX_WAIT
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
            self.colour_window.initial_position = None;
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
        let id = egui::ViewportId::from_hash_of("track-colour-window");
        // As tall as the panel, measured as it was drawn. A layout the user asked for that needs
        // more room (Fine adjustment opening, the wheel's Value row) is drawn once the window has
        // grown to it; one that needs less is drawn at once and the window shrinks after.
        let room = (bounds.height() - 48.0).max(180.0);
        let chrome = self.colour_window.chrome;
        let fit = |content: Option<f32>| content.map_or(400.0, |content| (content + chrome).ceil()).min(room);
        let window = &mut self.colour_window;
        if window.panel.asks() {
            let (now, asked) = (window.panel.height(false), window.panel.height(true));
            // Grown when the window drawn last was that tall, not when the size was asked for:
            // the system resizes a window of its own a frame or more later, sometimes, and the
            // taller layout drawn in the shorter window would flicker. A system that will not
            // make it that tall is waited for a few frames only.
            if settles(now, asked, fit(asked), (window.height, window.shown), window.waited) {
                window.panel.settle();
                window.waited = 0;
            } else {
                window.waited += 1;
                ctx.request_repaint_of(egui::ViewportId::from_hash_of("track-colour-window"));
            }
        }
        let panel = &mut window.panel;
        let height = fit(match (panel.height(false), panel.height(true)) {
            (Some(now), Some(asked)) => Some(now.max(asked)),
            (now, asked) => now.or(asked),
        });
        if self.colour_window.height != height {
            // Expanding near the bottom must keep the new controls on screen. Ordinary
            // moves are left alone; only a size change rechecks the available bounds.
            if let Some(requested) = self.colour_window.initial_position {
                let observed =
                    self.settings.colour_window_rect.map_or(requested, |[x, y, _, _]| Pos2::new(x as f32, y as f32));
                let adjusted = place(bounds, Vec2::new(WIDTH, height + 32.0), Some(observed));
                if adjusted != observed {
                    // An explicit one-shot move also works if the clamped position
                    // happens to equal the builder's unchanged opening position.
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(adjusted));
                }
            }
            self.colour_window.height = height;
        }
        let position = *self.colour_window.initial_position.get_or_insert_with(|| {
            let proposed = self.settings.colour_window_rect.map(|[x, y, _, _]| Pos2::new(x as f32, y as f32));
            place(bounds, Vec2::new(WIDTH, height + 32.0), proposed)
        });
        let own_frame = self.own_window_frame();
        let builder = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_position(position)
            .with_inner_size([WIDTH, height])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_window_level(browser_panel::map_window_level(main_focused, self.colour_window.focused));
        let builder = crate::kit::own_window::builder(builder, own_frame);
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
        let mut chrome = None;
        let mut shown = None;
        let mut render = |ui: &mut Ui, embedded: bool| {
            if embedded {
                embedded_layer = Some(ui.layer_id());
            }
            {
                let style = crate::kit::dialog::enter("color-picker");
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
                // Embedded, it is a dialog over the window, its title row and Close button its
                // own. In a window of its own it is that window's root, under the system's title
                // bar; the window's base is `window`'s background, as under the main window.
                let (outer, inside) = if embedded {
                    let outer = Rect::from_min_size(ui.max_rect().min, Vec2::new(WIDTH, height));
                    ui.allocate_rect(outer, egui::Sense::hover());
                    let painter = ui.painter().clone();
                    let fill = crate::kit::style_class::own_background_or(crate::kit::element::app_fill("panel"));
                    let (painted, title_row) =
                        crate::kit::dialog::paint(&painter, &style, outer, fill, Some(title.as_str()));
                    let bar = painted.title;
                    let button =
                        Rect::from_min_size(Pos2::new(bar.right() - 52.0, bar.top()), Vec2::new(52.0, bar.height()));
                    let id = Id::unique("track-colour-window-close");
                    let response = crate::kit::widgets::button_area(ui, button, id, format!("Close {title}"));
                    crate::kit::widgets::flat_button(
                        ui,
                        &painter,
                        button,
                        &response,
                        "Close",
                        (crate::kit::theme::TEXT_SMALL, None),
                    );
                    drop(title_row);
                    close |= response.clicked();
                    (outer.height(), painted.content)
                } else {
                    let base = crate::kit::own_window::base(ui, own_frame);
                    let content = style.paint_root(ui.painter(), base.whole, base.corners);
                    let inside =
                        Rect::from_min_max(Pos2::new(content.left(), content.top() + base.title_bar), content.max);
                    (base.whole.height(), inside)
                };
                chrome = Some(outer - inside.height());
                shown = Some(outer);
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
            // Where windows cannot open (the tests), the tool is a dialog over the window: one
            // box, the dialog's, not one in an egui Window's frame.
            crate::kit::floating::area(ctx, Id::unique("track-colour-window"), crate::kit::floating::Kind::Own)
                .order(egui::Order::Middle)
                .movable(true)
                .default_pos(ctx.content_rect().center() - Vec2::new(WIDTH, height) * 0.5)
                .show(ctx, |ui| render(ui, true));
        } else {
            ctx.show_viewport_immediate(id, builder, |ui, class| {
                render(ui, class == egui::ViewportClass::EmbeddedWindow);
            });
        }
        if let Some(shown) = shown {
            self.colour_window.shown = shown;
        }
        if let Some(chrome) = chrome
            && chrome.is_finite()
            && (chrome - self.colour_window.chrome).abs() > 0.5
        {
            self.colour_window.chrome = chrome.max(0.0);
            ctx.request_repaint_of(id);
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
            // Observe native moves for reopening/expansion, but never replay them through
            // the viewport builder. Persist once on close, not on every move frame.
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

    /// A taller layout waits for the window to be drawn at its size, not only asked for it: the
    /// system resizes a window of its own a frame or more later.
    #[test]
    fn a_taller_layout_waits_until_the_window_is_drawn_that_tall() {
        let (now, asked) = (Some(300.0), Some(500.0));
        assert!(!settles(now, asked, 540.0, (400.0, 400.0), 0), "not asked for yet");
        assert!(!settles(now, asked, 540.0, (540.0, 400.0), 1), "asked for, not drawn yet");
        assert!(settles(now, asked, 540.0, (540.0, 540.0), 2), "drawn that tall");
        assert!(settles(now, asked, 540.0, (540.0, 400.0), MAX_WAIT), "a system that will not");
        assert!(settles(asked, now, 340.0, (540.0, 540.0), 0), "a shorter layout at once");
    }
}
