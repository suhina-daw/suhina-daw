// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser beside the views, and the sample map in the window or in one of its own.

use super::*;

impl App {
    /// Shows or hides the browser panel, as Ctrl+B does.
    pub fn set_browser_open(&mut self, open: bool) {
        if self.settings.browser_open != open {
            self.toggle_browser();
        }
    }

    pub(super) fn toggle_browser(&mut self) {
        self.settings.browser_open = !self.settings.browser_open;
        self.settings_changed = true;
        if !self.settings.browser_open {
            browser_view::stop_preview(&mut self.browser, &mut self.projects);
            self.browser.drag = None;
        }
    }

    /// Lays out the browser at the side of `area`, draws it with its handle, and returns what is
    /// left for the view.
    pub(super) fn browser_panel(&mut self, ui: &mut Ui, area: Rect) -> Rect {
        let (low, high) = crate::settings::BROWSER_WIDTHS;
        let widest = (high as f32).min(area.width() * crate::settings::BROWSER_SHARE).max(low as f32);
        let width = (self.settings.browser_width as f32).clamp(low as f32, widest);
        let left = self.settings.browser_side == Side::Left;
        // While it slides (#708) only `part` of it shows: it moves in from its side whole, not
        // squeezed, and its handle takes hold only once it is all there.
        let part = self.browser_part.clamp(0.0, 1.0);
        let (shown, handle_width) = (width * part, BROWSER_HANDLE * part);
        let (panel, handle, rest) = if left {
            let panel = Rect::from_min_max(
                Pos2::new(area.left() + shown - width, area.top()),
                Pos2::new(area.left() + shown, area.bottom()),
            );
            let handle = Rect::from_min_max(
                Pos2::new(panel.right(), area.top()),
                Pos2::new(panel.right() + handle_width, area.bottom()),
            );
            (panel, handle, Rect::from_min_max(Pos2::new(handle.right(), area.top()), area.max))
        } else {
            let panel = Rect::from_min_max(
                Pos2::new(area.right() - shown, area.top()),
                Pos2::new(area.right() - shown + width, area.bottom()),
            );
            let handle = Rect::from_min_max(
                Pos2::new(panel.left() - handle_width, area.top()),
                Pos2::new(panel.left(), area.bottom()),
            );
            (panel, handle, Rect::from_min_max(area.min, Pos2::new(handle.left(), area.bottom())))
        };
        let default_width = crate::settings::Settings::default().browser_width;
        let (resized, done) = if part >= 1.0 {
            browser_view::resize_handle(ui, handle, width, left, default_width as f32)
        } else {
            (None, false)
        };
        if let Some(new) = resized {
            let new = new.round().clamp(low as f32, widest) as u32;
            self.settings.browser_width = new;
        }
        // Saved once the handle is let go: writing the settings file on every frame of a drag
        // made the drag lag.
        if done {
            self.settings_changed = true;
        }
        let search_key = self.keymap.label("browser.search").unwrap_or_default();
        let selected_track = self.selected_track();
        let revision = self.revision;
        let mut map_settings = self.settings.clone();
        let map_panel = &mut self.sample_map;
        let mut draw_map = |ui: &mut Ui, rect: Rect, state: &mut BrowserState, projects: &mut Projects| {
            suhina_plugins::trace::timed("sample map", || {
                map_panel.show(ui, rect, state, projects, &mut map_settings, revision)
            });
        };
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                ui.set_clip_rect(panel.intersect(area).intersect(ui.clip_rect()));
                let cx = browser_view::Context {
                    projects: &mut self.projects,
                    settings: &self.settings,
                    selected_track,
                    search_key,
                    other_results: Some(&mut draw_map),
                };
                browser_view::show(ui, panel, &mut self.browser, cx)
            })
            .inner;
        if map_settings != self.settings {
            self.settings = map_settings;
            self.settings_changed = true;
        }
        if let Some(why) = self.sample_map.status.take() {
            self.status = Status::Hint(why);
        }
        let added = std::mem::take(&mut self.sample_map.add);
        if !added.is_empty() {
            self.add_to_selected(added);
        }
        if let Some(status) = out.status {
            self.status = status;
        }
        if out.pick_folder {
            self.ask_pick(PickField::BrowserFolder);
        }
        if out.toggle_autoplay {
            self.settings.preview_autoplay = !self.settings.preview_autoplay;
            self.settings_changed = true;
        }
        if let Some(path) = out.open_project {
            self.request_open(path);
        }
        if !out.add.is_empty() {
            self.add_to_selected(out.add);
        }
        rest
    }

    /// Shows the sample map large in the main area, or closes it back into the browser.
    pub(super) fn toggle_large_map(&mut self) {
        if self.sample_map.view.window {
            self.close_map_window();
        }
        let large = !self.sample_map.view.large;
        self.sample_map.view.large = large;
        self.browser.mode = if large { ResultsMode::List } else { ResultsMode::Other };
    }

    /// Opens the sample map in a window of its own, or puts it back into the browser.
    pub(super) fn toggle_map_window(&mut self) {
        if self.sample_map.view.window {
            self.close_map_window();
        } else {
            self.settings_open = false;
            self.sample_map.set_window(true, &mut self.browser);
            self.settings.map_window = true;
            self.settings_changed = true;
        }
    }

    pub(super) fn close_map_window(&mut self) {
        self.sample_map.set_window(false, &mut self.browser);
        self.map_window.drag = None;
        if self.settings.map_window {
            self.settings.map_window = false;
            self.settings_changed = true;
        }
    }

    /// Where a drag from the browser or the map is, in the main window: the pointer, or for a
    /// drag that started in the map's own window, that window's pointer moved onto this one.
    pub(super) fn drag_pointer(&self, ctx: &egui::Context) -> Option<Pos2> {
        match self.map_window.drag {
            Some(remote) => {
                let main = ctx.input(|i| i.viewport().inner_rect).map_or(Pos2::ZERO, |r| r.min);
                remote.screen.map(|p| p - main.to_vec2())
            }
            None => ctx.pointer_latest_pos(),
        }
    }

    /// Draws the sample map in its own window while it has one (`docs/spec/sample-map.md` ›
    /// Where the map is shown), keeps where the window is, and follows a drag that starts in it.
    pub(super) fn sample_map_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.sample_map.view.window {
            self.map_window.opened_at = None;
            self.map_window.drag = None;
            return;
        }
        // The browser's own Map switch takes the map back.
        if self.browser.mode == ResultsMode::Other {
            self.close_map_window();
            return;
        }
        let opened_at = *self.map_window.opened_at.get_or_insert(self.settings.map_window_rect);
        let mut builder =
            egui::ViewportBuilder::default().with_title("Sample map - Suhina").with_min_inner_size([320.0, 240.0]);
        builder = match opened_at {
            Some([x, y, w, h]) => builder.with_position([x as f32, y as f32]).with_inner_size([w as f32, h as f32]),
            None => builder.with_inner_size(MAP_WINDOW_SIZE),
        };
        let revision = self.revision;
        let had_drag = self.browser.drag.is_some();
        let mut settings = self.settings.clone();
        let (panel, browser, projects, keymap) =
            (&mut self.sample_map, &mut self.browser, &mut self.projects, &self.keymap);
        let id = egui::ViewportId::from_hash_of("sample-map-window");
        let frame = ctx.show_viewport_immediate(id, builder, |ui, class| {
            let embedded = class == egui::ViewportClass::EmbeddedWindow;
            // Embedded (a toolkit without windows of its own, as in tests) the map gets a fixed size.
            let rect = if embedded {
                Rect::from_min_size(ui.available_rect_before_wrap().min, MAP_WINDOW_SIZE.into())
            } else {
                ui.max_rect()
            };
            ui.allocate_rect(rect, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, theme::window_background());
            let changed = panel.show(ui, rect, browser, projects, &mut settings, revision);
            // The window has its own keyboard: its keys run the same actions as the main window's,
            // unless a field in it is being typed in. Embedded, the main window reads them.
            let keys = if embedded || ui.ctx().egui_wants_keyboard_input() {
                Vec::new()
            } else {
                ui.input_mut(|i| keymap.pressed(i, |id| !crate::settings::keymap::piano_roll_only(id)))
            };
            ui.input(|i| MapWindowFrame {
                changed,
                keys,
                embedded,
                close: i.viewport().close_requested(),
                place: match (i.viewport().outer_rect, i.viewport().inner_rect) {
                    (Some(outer), Some(inner)) => Some([
                        outer.min.x.round() as i32,
                        outer.min.y.round() as i32,
                        inner.width().round() as i32,
                        inner.height().round() as i32,
                    ]),
                    _ => None,
                },
                screen: i.viewport().inner_rect.zip(i.pointer.latest_pos()).map(|(r, p)| p + r.min.to_vec2()),
                released: !i.pointer.primary_down(),
                escape: i.key_pressed(Key::Escape),
                ctrl: i.modifiers.command,
                time: i.time,
            })
        });
        if frame.changed || settings != self.settings {
            self.settings = settings;
            self.settings_changed = true;
        }
        if let Some(why) = self.sample_map.status.take() {
            self.status = Status::Hint(why);
        }
        let added = std::mem::take(&mut self.sample_map.add);
        if !added.is_empty() {
            self.add_to_selected(added);
        }
        if frame.close {
            self.close_map_window();
            return;
        }
        // The map window's keys wait behind the same dialogs as the main window's.
        if !self.dialog_holds_actions() {
            for action in &frame.keys {
                self.do_action(action, actions);
            }
        }
        if !self.sample_map.view.window {
            return;
        }
        // A drag that starts in the map's own window stays with that window until the button is
        // let go; the main window reads it from there. Embedded, the window shares the input.
        if !frame.embedded && self.browser.drag.is_some() && (self.map_window.drag.is_some() || !had_drag) {
            self.map_window.drag = Some(RemoteDrag {
                screen: frame.screen,
                released: frame.released,
                escape: frame.escape,
                ctrl: frame.ctrl,
            });
        }
        // Where the window is, saved once it has stayed put for a moment.
        if let Some(place) = frame.place.filter(|p| self.settings.map_window_rect != Some(*p)) {
            match self.map_window.moved {
                Some((p, since)) if p == place && frame.time - since >= MAP_WINDOW_SETTLE => {
                    self.settings.map_window_rect = Some(place);
                    self.settings_changed = true;
                    self.map_window.moved = None;
                }
                Some((p, _)) if p == place => {}
                _ => self.map_window.moved = Some((place, frame.time)),
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(MAP_WINDOW_SETTLE));
        }
    }

    pub(super) fn close_large_map(&mut self) {
        if self.sample_map.view.large {
            self.toggle_large_map();
        }
    }

    /// Adds browser items to the selected track, as Enter and a double-click do: devices to its
    /// chain, files in the Arrangement at the play start marker, in the Session from its first
    /// empty slot; without a selected track, on new tracks.
    pub(super) fn add_to_selected(&mut self, items: Vec<BrowserItem>) {
        let Some(session) = self.session() else {
            self.status = Status::Hint("Open a project to add to".into());
            return;
        };
        let project = session.project();
        let track = self.selected_track().filter(|t| project.tracks.iter().any(|x| x.id == *t));
        let files = items.iter().any(|i| i.kind.is_file_clip());
        let target = match (track, self.shown) {
            (Some(track), _) if !files => DropTarget::Track(track),
            (Some(track), View::Arrangement) => {
                let beat = session.player().map_or(0.0, |p| p.monitor().start_beat());
                DropTarget::Lane { track, beat }
            }
            (Some(track), View::Session) => {
                let column = project.launcher.columns.iter().find(|c| c.track == track);
                let scene = column
                    .and_then(|c| c.slots.iter().position(|s| s.clip.is_none()))
                    .unwrap_or(project.launcher.scenes.len());
                DropTarget::Slot { track, scene }
            }
            (None, View::Arrangement) => DropTarget::NewTracks { beat: Some(0.0), scene: None },
            (None, View::Session) => DropTarget::NewTracks { beat: None, scene: Some(0) },
        };
        suhina_plugins::trace::timed("adding to a track", || self.drop_items(&items, target, false));
    }
}

/// The sample map's own window when nothing says where it was, in points.
pub(super) const MAP_WINDOW_SIZE: [f32; 2] = [900.0, 640.0];

/// How long the map's window must stay put before its place is saved, in seconds.
pub(super) const MAP_WINDOW_SETTLE: f64 = 0.5;

/// The sample map's own window, as the main window keeps track of it.
#[derive(Debug, Default)]
pub(super) struct MapWindow {
    /// Where it opened (`left, top, width, height`), set when it opens; `Some(None)` for the
    /// default place.
    pub(super) opened_at: Option<Option<[i32; 4]>>,
    /// A place it moved to and when, saved when it stays there.
    pub(super) moved: Option<([i32; 4], f64)>,
    /// A drag from the browser that started in this window.
    pub(super) drag: Option<RemoteDrag>,
}

/// A drag that started in the map's own window, read from that window's input.
#[derive(Debug, Clone, Copy)]
pub(super) struct RemoteDrag {
    /// The pointer on the desktop, in points.
    pub(super) screen: Option<Pos2>,
    pub(super) released: bool,
    pub(super) escape: bool,
    pub(super) ctrl: bool,
}

/// What a frame of the map's own window reports back.
pub(super) struct MapWindowFrame {
    pub(super) changed: bool,
    /// Keymap actions whose keys were pressed in that window.
    pub(super) keys: Vec<String>,
    pub(super) embedded: bool,
    pub(super) close: bool,
    pub(super) place: Option<[i32; 4]>,
    pub(super) screen: Option<Pos2>,
    pub(super) released: bool,
    pub(super) escape: bool,
    pub(super) ctrl: bool,
    pub(super) time: f64,
}
