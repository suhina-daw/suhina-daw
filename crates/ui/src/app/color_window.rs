// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The owned color tool window. Commands remain on the active project's main action path;
//! the native viewport has its own pointer, keyboard and close lifetime.

use super::*;
use crate::interaction::color_picker::{
    hex,
    panel::{self, Edit, Panel},
};
use suhina_engine::model::{ClipId, ClipPlace, Project, Rgb};

const WIDTH: f32 = 340.0;
/// How tall the system's title bar is taken to be over a window whose content starts under it.
const SYSTEM_TITLE_BAR: f32 = 32.0;

#[derive(Default)]
pub(super) struct ColorWindow {
    open: bool,
    project: Option<ProjectId>,
    tracks: Vec<TrackId>,
    /// Arrangement clips the window colors instead of `tracks`, opened from a clip's menu; it
    /// keeps them while the track selection changes, until it is opened again.
    clips: Vec<ClipId>,
    selection_seen: Vec<TrackId>,
    panel: Panel,
    // A stable opening position, not the last reported native position. eframe reads geometry
    // before applying queued moves, so feeding observations back here oscillates.
    initial_position: Option<Pos2>,
    height: f32,
    /// How much taller the size asked of the window is than the panel in it, as last measured:
    /// the dialog's padding, and its border and title row embedded, or the system's title bar in
    /// a window of its own.
    chrome: f32,
    focus_requested: bool,
    focused: bool,
    minimized: bool,
    embedded_layer: Option<egui::LayerId>,
    /// A drag in the picker has changed a color under a history gesture not yet ended.
    pub gesture_pending: bool,
}

impl App {
    pub(super) fn preview_color_targets(&self, ctx: &egui::Context) {
        let tracks = if self.color_window.open
            && self.projects.active() == self.color_window.project
            && self.color_window.clips.is_empty()
        {
            self.color_window.tracks.as_slice()
        } else {
            &[]
        };
        crate::kit::selection::preview_tracks(ctx, tracks);
    }

    /// Only the embedded tool's own controls keep normal widget keyboard navigation.
    /// The rest of the DAW continues to use its existing shortcut and launch-key rules.
    pub(super) fn color_window_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.color_window.open && self.color_window.embedded_layer.is_some_and(|layer| focused_in_layer(ctx, layer))
    }

    pub(super) fn color_history_changed(&mut self) {
        if std::mem::take(&mut self.color_window.gesture_pending) {
            self.color_window.panel.cancel_pointer();
            if self.open_gesture.as_deref() == Some(panel::GESTURE) {
                self.open_gesture = None;
            }
        }
    }

    pub(super) fn open_color_window(&mut self, tracks: Option<Vec<TrackId>>) {
        let selection = self.focused_tracks();
        let tracks = tracks.unwrap_or_else(|| selection.clone());
        let Some(session) = self.session() else { return };
        if tracks.is_empty() || tracks.iter().any(|t| session.project().track(*t).is_none()) {
            self.status = Status::Hint("Select a track to choose its color".into());
            return;
        }
        self.open_color_window_on(tracks, Vec::new(), selection);
    }

    /// Open the window on clips, arrangement clips or slots', from a clip's menu.
    pub(super) fn open_color_window_for_clips(&mut self, clips: Vec<ClipId>) {
        let Some(session) = self.session() else { return };
        if clips.is_empty() || clips.iter().any(|c| clip_color(session.project(), *c).is_none()) {
            self.status = Status::Hint("Select a clip to choose its color".into());
            return;
        }
        let selection = self.focused_tracks();
        self.open_color_window_on(Vec::new(), clips, selection);
    }

    fn open_color_window_on(&mut self, tracks: Vec<TrackId>, clips: Vec<ClipId>, selection: Vec<TrackId>) {
        self.finish_color_gesture(false);
        let window = &mut self.color_window;
        window.panel.discard_drafts();
        if !window.open {
            window.initial_position = None;
        }
        window.open = true;
        window.project = self.projects.active();
        window.tracks = tracks;
        window.clips = clips;
        window.selection_seen = selection;
        window.focus_requested = true;
    }

    pub(super) fn run_color_window_action(&mut self, action: &Action) -> bool {
        match action.id {
            "ui.colorPicker" => {
                let ids = |key: &str| {
                    action.params[key].as_array().map(|list| {
                        list.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect::<Vec<_>>()
                    })
                };
                if let Some(clips) = ids("clips") {
                    self.open_color_window_for_clips(clips.into_iter().map(ClipId).collect());
                } else {
                    self.open_color_window(ids("tracks").map(|list| list.into_iter().map(TrackId).collect()));
                }
            }
            "ui.finishColor" => self.finish_color_gesture(false),
            "ui.cancelColor" => self.finish_color_gesture(true),
            _ => return false,
        }
        true
    }

    /// Whether every target already has `color`: a clip only as its own, so choosing its track's
    /// color still gives it one.
    fn color_targets_have(&self, color: Rgb) -> bool {
        let Some(project) = self.session().map(Session::project) else { return false };
        let window = &self.color_window;
        if window.clips.is_empty() {
            project
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| window.tracks.contains(&t.id))
                .all(|(place, t)| suhina_session::colors::track_color(t.color, place) == color)
        } else {
            window.clips.iter().all(|c| {
                project.clip_place(*c).and_then(|p| project.clip_at(p)).and_then(|clip| clip.color) == Some(color)
            })
        }
    }

    fn finish_color_gesture(&mut self, cancel: bool) {
        let project = self.color_window.project;
        let ended = self
            .projects
            .session_mut(project)
            .ok()
            .and_then(|session| session.execute("history.endGesture", &json!({ "gesture": panel::GESTURE })).ok())
            .is_some_and(|value| value["ended"].as_bool() == Some(true));
        self.color_window.panel.dragging = false;
        if self.open_gesture.as_deref() == Some(panel::GESTURE) {
            self.open_gesture = None;
        }
        self.color_window.gesture_pending = false;
        if cancel && ended && self.projects.active() == project {
            // endGesture is conditional on its id: an intervening edit is never undone.
            self.run(&Action::new("edit.undo", json!({})));
        }
    }

    pub(super) fn show_color_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // Menus show the user's binding, including an unbound or reassigned F2.
        let shortcut = self.keymap.label("selection.colour").unwrap_or_default();
        ctx.data_mut(|d| d.insert_temp(Id::unique("color-picker-shortcut"), shortcut));
        crate::interaction::color_picker::set_menu_saved(ctx, &self.settings.saved_colors);
        if !self.color_window.open {
            return;
        }
        if self.projects.active() != self.color_window.project {
            self.finish_color_gesture(false);
            self.color_window.open = false;
            self.settings_changed = true;
            return;
        }
        let selection = self.focused_tracks();
        if !self.color_window.clips.is_empty() {
            // Clips stay the target as the track selection changes; it is seen, not followed.
            self.color_window.selection_seen = selection.clone();
        }
        let targets_removed = self.session().is_some_and(|s| {
            !self.color_window.tracks.is_empty()
                && self.color_window.tracks.iter().all(|id| s.project().track(*id).is_none())
        });
        if targets_removed {
            // Deletion may automatically focus the next track. Do not silently color it;
            // a subsequent header selection or reopening the picker chooses a new target.
            self.finish_color_gesture(false);
            self.color_window.tracks.clear();
            self.color_window.selection_seen = selection.clone();
        } else if !selection.is_empty() && selection != self.color_window.selection_seen {
            self.finish_color_gesture(false);
            self.color_window.panel.discard_drafts();
            self.color_window.selection_seen = selection.clone();
            self.color_window.tracks = selection;
        }
        let Some(session) = self.projects.active().and_then(|id| self.projects.get(id)) else { return };
        let project = session.project();
        self.color_window.tracks.retain(|t| project.track(*t).is_some());
        self.color_window.clips.retain(|c| clip_color(project, *c).is_some());
        self.color_window.panel.clips = !self.color_window.clips.is_empty();
        let first = self
            .color_window
            .tracks
            .first()
            .and_then(|id| project.tracks.iter().enumerate().find(|(_, t)| t.id == *id));
        let clips = &self.color_window.clips;
        let current = if clips.is_empty() {
            first.map(|(place, t)| suhina_session::colors::track_color(t.color, place))
        } else {
            clips.first().and_then(|c| clip_color(project, *c))
        };
        let title = match (clips.len(), self.color_window.tracks.len(), first) {
            (1, ..) => "Clip color".to_string(),
            (0, 1, Some((_, track))) => format!("Track color · {}", track.name),
            (0, 0, _) => "Track color · Select a track".to_string(),
            (0, count, _) => format!("Track color · {count} tracks"),
            (count, ..) => format!("Clip color · {count} clips"),
        };
        if let Some(color) = current {
            self.color_window.panel.sync(color);
        }
        let (main_focused, main_minimized, main_rect) = ctx.input_for(egui::ViewportId::ROOT, |i| {
            (
                i.viewport().focused.unwrap_or(false),
                i.viewport().minimized.unwrap_or(false),
                i.viewport().outer_rect.or(i.viewport().inner_rect),
            )
        });
        let bounds = main_rect.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0)));
        let id = egui::ViewportId::from_hash_of("track-color-window");
        // As tall as the panel, measured as it was drawn. A view the user asked for that needs more
        // room is drawn once the window has grown to it; one that needs less is drawn at once and
        // the window shrinks after.
        let room = (bounds.height() - 48.0).max(180.0);
        let fit = |content: Option<f32>| {
            content.map_or(400.0, |content| (content + self.color_window.chrome).ceil()).min(room)
        };
        let panel = &mut self.color_window.panel;
        if panel.asks() {
            let (now, asked) = (panel.height(false), panel.height(true));
            let grown = self.color_window.height >= fit(asked) - 0.5;
            if grown || asked <= now {
                panel.settle();
            } else {
                ctx.request_repaint_of(id);
            }
        }
        let height = fit(match (panel.height(false), panel.height(true)) {
            (Some(now), Some(asked)) => Some(now.max(asked)),
            (now, asked) => now.or(asked),
        });
        let own_frame = self.own_window_frame();
        let outer = outer_size(height, own_frame);
        if self.color_window.height != height {
            // Expanding near the bottom must keep the new controls on screen. Ordinary
            // moves are left alone; only a size change rechecks the available bounds.
            if let Some(requested) = self.color_window.initial_position {
                let observed =
                    self.settings.color_window_rect.map_or(requested, |[x, y, _, _]| Pos2::new(x as f32, y as f32));
                let adjusted = place(bounds, outer, Some(observed));
                if adjusted != observed {
                    // An explicit one-shot move also works if the clamped position
                    // happens to equal the builder's unchanged opening position.
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(adjusted));
                }
            }
            self.color_window.height = height;
        }
        let position = *self.color_window.initial_position.get_or_insert_with(|| {
            let proposed = self.settings.color_window_rect.map(|[x, y, _, _]| Pos2::new(x as f32, y as f32));
            place(bounds, outer, proposed)
        });
        let builder = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_position(position)
            .with_inner_size([WIDTH, height])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_window_level(browser_panel::map_window_level(main_focused, self.color_window.focused));
        let builder = crate::kit::own_window::builder(builder, own_frame);
        let enabled = current.is_some() && !self.dialog_holds_actions();
        let saved = &self.settings.saved_colors;
        let panel = &mut self.color_window.panel;
        let keymap = &self.keymap;
        let mut edits = Vec::new();
        let mut keys = Vec::new();
        let mut close = false;
        let mut focused = false;
        let mut placement = None;
        let mut embedded_layer = None;
        let mut chrome = None;
        let mut render = |ui: &mut Ui, embedded: bool| {
            if embedded {
                embedded_layer = Some(ui.layer_id());
            }
            {
                let style = crate::kit::dialog::enter("color-picker");
                if !embedded && suhina_plugins::trace::on() {
                    suhina_plugins::trace::state(
                        "color window surface",
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
                    let (painted, title_row) = crate::kit::dialog::paint(&painter, &style, outer, Some(title.as_str()));
                    let id = Id::unique("track-color-window-close");
                    let response = crate::kit::dialog::close_button(ui, &painter, painted.title, id, &title);
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
                ui.scope_builder(egui::UiBuilder::new().max_rect(inside), |ui| {
                    let mut styled = style.ui(ui);
                    egui::ScrollArea::vertical().show(&mut styled, |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            edits.extend(panel.show(ui, saved));
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
            crate::kit::floating::area(ctx, Id::unique("track-color-window"), crate::kit::floating::Kind::Own)
                .order(egui::Order::Middle)
                .movable(true)
                .default_pos(ctx.content_rect().center() - Vec2::new(WIDTH, height) * 0.5)
                .show(ctx, |ui| render(ui, true));
        } else {
            ctx.show_viewport_immediate(id, builder, |ui, class| {
                render(ui, class == egui::ViewportClass::EmbeddedWindow);
            });
        }
        if let Some(chrome) = chrome
            && chrome.is_finite()
            && (chrome - self.color_window.chrome).abs() > 0.5
        {
            self.color_window.chrome = chrome.max(0.0);
            ctx.request_repaint_of(id);
        }
        self.color_window.focused = focused;
        self.color_window.embedded_layer = embedded_layer;
        if self.color_window_owns_keyboard(ctx) || (!ctx.embed_viewports() && focused) {
            // Key-up may go to this window instead of the DAW. End held notes and
            // Gate/Repeat launches at the ownership change, even before a key-up arrives.
            self.release_typing_keys();
            if let Some(project) = self.projects.active().and_then(|id| self.projects.get(id)).map(Session::project) {
                self.view.launch_keys.release_all(project, actions);
            }
        }
        if let Some(rect) = placement
            && self.settings.color_window_rect != Some(rect)
        {
            self.settings.color_window_rect = Some(rect);
            // Observe native moves for reopening/expansion, but never replay them through
            // the viewport builder. Persist once on close, not on every move frame.
        }
        if self.color_window.focus_requested {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
            self.color_window.focus_requested = false;
        }
        if let Some(minimize) = browser_panel::follow_main_minimised(main_minimized, self.color_window.minimized) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Minimized(minimize));
            self.color_window.minimized = minimize;
        }
        // Clips, when the window was opened on them, else tracks: one command for them all.
        let (command, key, targets) = if self.color_window.clips.is_empty() {
            let tracks: Vec<u32> = self.color_window.tracks.iter().map(|t| t.0).collect();
            ("track.setColor", "tracks", json!(tracks))
        } else {
            let clips: Vec<serde_json::Value> =
                self.color_window.clips.iter().map(|c| json!({ "clip": c.0 })).collect();
            ("clip.setColorMany", "clips", json!(clips))
        };
        for edit in edits {
            match edit {
                Edit::Color(color, dragging) => {
                    if self.color_targets_have(color) {
                        continue;
                    }
                    let mut params = json!({ key: targets, "color": hex(color) });
                    if dragging {
                        params["gesture"] = json!(panel::GESTURE);
                    }
                    actions.push(Action::new(command, params));
                }
                Edit::Default => actions.push(Action::new(command, json!({ key: targets, "color": "none" }))),
                Edit::Saved(change) => {
                    if panel::change_saved(&mut self.settings.saved_colors, change) {
                        self.settings_changed = true;
                    }
                }
                Edit::Finish => actions.push(Action::new("ui.finishColor", json!({}))),
                Edit::Cancel => actions.push(Action::new("ui.cancelColor", json!({}))),
            }
        }
        for key in keys {
            self.do_action(&key, actions);
        }
        if close {
            actions.push(Action::new("ui.finishColor", json!({})));
            self.color_window.open = false;
            self.settings_changed = true;
        }
    }
}

fn focused_in_layer(ctx: &egui::Context, layer: egui::LayerId) -> bool {
    ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| r.layer_id == layer)
}

/// How big the window is on screen with content `height` tall, as [`place`] keeps it on screen:
/// the system's title bar over it, unless the content runs under that bar (`frame.title_bar`),
/// when `height` holds the bar already.
fn outer_size(height: f32, frame: crate::kit::own_window::Frame) -> Vec2 {
    let bar = if frame.title_bar > 0.0 { 0.0 } else { SYSTEM_TITLE_BAR };
    Vec2::new(WIDTH, height + bar)
}

/// A new or remembered window stays on the current screen even after a monitor is removed.
fn place(bounds: Rect, size: Vec2, remembered: Option<Pos2>) -> Pos2 {
    let proposed = remembered.filter(|p| p.x.is_finite() && p.y.is_finite()).unwrap_or(bounds.center() - size * 0.5);
    Pos2::new(
        proposed.x.clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left())),
        proposed.y.clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top())),
    )
}

/// The color a clip shows, an arrangement clip or a slot's: its own, else its track's; `None`
/// when no clip is `id`.
fn clip_color(project: &Project, id: ClipId) -> Option<Rgb> {
    let place = project.clip_place(id)?;
    let clip = project.clip_at(place)?;
    clip.color.or_else(|| {
        let track = match place {
            ClipPlace::Lane { lane, .. } => project.arrangement.lanes.get(lane)?.track?,
            ClipPlace::Slot { column, .. } => project.launcher.columns.get(column)?.track,
        };
        let (place, t) = project.tracks.iter().enumerate().find(|(_, t)| t.id == track)?;
        Some(suhina_session::colors::track_color(t.color, place))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_placement_centers_and_expansion_or_stale_positions_stay_inside_bounds() {
        let bounds = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(1280.0, 900.0));
        let compact = Vec2::new(WIDTH, 432.0);
        let center = place(bounds, compact, None);
        assert_eq!(center + compact * 0.5, bounds.center());
        let expanded = Vec2::new(WIDTH, 752.0);
        for remembered in [center, Pos2::new(-3000.0, 2000.0), Pos2::new(f32::NAN, 0.0)] {
            let rect = Rect::from_min_size(place(bounds, expanded, Some(remembered)), expanded);
            assert!(bounds.contains_rect(rect));
        }
    }

    /// A window whose content runs under the system's title bar is as tall as its content, so near
    /// the screen's bottom it sits on the bottom edge, not a title bar's height above it.
    #[test]
    fn a_window_with_its_content_under_the_title_bar_is_as_tall_as_its_content() {
        let framed = crate::kit::own_window::Frame { title_bar: 0.0, ..Default::default() };
        let under = crate::kit::own_window::Frame { title_bar: 28.0, ..Default::default() };
        assert_eq!(outer_size(400.0, framed), Vec2::new(WIDTH, 400.0 + SYSTEM_TITLE_BAR));
        assert_eq!(outer_size(400.0, under), Vec2::new(WIDTH, 400.0));
        let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let low = Some(Pos2::new(100.0, 790.0));
        assert_eq!(place(bounds, outer_size(400.0, under), low).y, 400.0);
        assert_eq!(place(bounds, outer_size(400.0, framed), low).y, 400.0 - SYSTEM_TITLE_BAR);
    }
}
