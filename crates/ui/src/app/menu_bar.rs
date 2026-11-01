// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's menu bar: the menus in `menus` and what their items run.

use super::*;

impl App {
    /// The menu bar: the same actions as the shortcuts and the views' controls.
    pub(super) fn menu_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let mut styled_ui = crate::kit::style_class::ui(ui);
        let ui = &mut *styled_ui;
        // Whether the File menu showed last frame, so its recent list is read once as it opens.
        let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
        let shown = menus::show(ui, &self.menus());
        if shown.open.contains(&"File") {
            self.file_menu_open = true;
            if !file_menu_was_open {
                self.lists_wanted = true;
            }
        }
        if let Some(id) = shown.picked {
            self.run_menu_item(&id, ui.ctx(), actions);
        }
    }

    /// The menu bar as it is now: what the window's bar draws and the macOS bar is built from.
    pub fn menus(&self) -> Vec<menus::Menu> {
        use menus::{Entry, Item, Menu};
        let has_project = self.session().is_some();
        let (can_undo, can_redo) = self.session().map_or((false, false), |s| (s.can_undo(), s.can_redo()));
        let several = self.projects.len() >= 2;
        let keys = |action: &str| self.keymap.label(action).unwrap_or_default();
        let item = |id: &str, label: &str| Item::new(id, label).keys(keys(id));
        let recent: Vec<Entry> = self
            .project_lists
            .recent
            .iter()
            .take(RECENT_IN_MENU)
            .enumerate()
            .map(|(index, path)| {
                let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into());
                Item::new(format!("menu.recent.{index}"), name).hint(path.display().to_string()).into()
            })
            .collect();
        let file = vec![
            item("file.new", "New or open…").into(),
            item("file.open", "Open…").into(),
            Entry::Submenu { label: "Open recent".into(), enabled: !recent.is_empty(), entries: recent },
            Entry::Separator,
            item("file.save", "Save").enabled(has_project).into(),
            item("file.saveAs", "Save as…").enabled(has_project).into(),
            Item::new("menu.collectAndSave", "Collect and save")
                .enabled(has_project)
                .hint("Copy the audio the project plays from other folders into its own folder, then save")
                .into(),
            item("file.saveTemplate", "Save as template…").enabled(has_project).into(),
            Item::new("menu.export", "Export…").enabled(has_project).into(),
            Entry::Separator,
            Item::new("menu.quit", "Quit").into(),
        ];
        let edit = vec![
            item("edit.undo", "Undo").enabled(can_undo).into(),
            item("edit.redo", "Redo").enabled(can_redo).into(),
        ];
        let missing = self.missing.rows.len();
        let devices = self.detail_open && self.detail_showing == detail_view::Showing::Devices;
        let (zoom, limits) = (self.settings.zoom, crate::settings::ZOOMS);
        let view = vec![
            item("view.session", "Launcher").enabled(has_project).into(),
            item("view.arrangement", "Arrangement").enabled(has_project).into(),
            item(
                "view.sampleMapWindow",
                if self.sample_map.view.window { "Dock sample map" } else { "Sample map in a window" },
            )
            .into(),
            item("view.sampleMap", if self.sample_map.view.large { "Hide sample map" } else { "Sample map" }).into(),
            Item::new(
                "menu.missingFiles",
                if missing == 0 { "Missing files…".to_string() } else { format!("Missing files ({missing})…") },
            )
            .enabled(missing > 0)
            .into(),
            item("view.browser", if self.settings.browser_open { "Hide browser" } else { "Show browser" }).into(),
            item("view.detail", if self.detail_open { "Hide detail area" } else { "Show detail area" })
                .enabled(has_project)
                .into(),
            // The selected clip's editor (› Least height and closing in ui-clip-editor.md).
            item("view.editor", if self.detail_open { "Hide editor" } else { "Show editor" })
                .enabled(has_project && self.shown_clip().is_some())
                .into(),
            // The detail area's Clip / Devices switch, for a clip with an editor.
            item("view.devices", if devices { "Show clip" } else { "Show devices" })
                .enabled(has_project && self.piano_clip(self.shown_clip()).is_some())
                .into(),
            Entry::Separator,
            item("view.zoomIn", "Zoom in").enabled(zoom < limits.1).into(),
            item("view.zoomOut", "Zoom out").enabled(zoom > limits.0).into(),
            Item::new("menu.actualSize", format!("Actual size ({zoom} %)"))
                .keys(keys("view.zoomReset"))
                .enabled(zoom != crate::settings::ZOOM)
                .into(),
        ];
        let mut projects = vec![
            item("project.next", "Next project").enabled(several).into(),
            item("project.previous", "Previous project").enabled(several).into(),
        ];
        if self.projects.active().is_some() {
            projects.push(Item::new("menu.closeProject", "Close project").into());
        }
        vec![
            Menu { title: "File", entries: file },
            Menu { title: "Edit", entries: edit },
            Menu { title: "View", entries: view },
            Menu { title: "Projects", entries: projects },
            Menu { title: "Options", entries: vec![item("settings.open", "Settings").into()] },
            Menu {
                title: "Help",
                entries: vec![
                    Item::new("menu.checkUpdates", "Check for updates…").into(),
                    item("help.about", "About Suhina").into(),
                ],
            },
        ]
    }

    /// Runs the menu item `id` (`menus`), picked from the window's bar or the system's.
    pub fn run_menu_item(&mut self, id: &str, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let has_project = self.session().is_some();
        match id {
            // The start panel already offers new and open; the dialog would only cover it.
            "file.new" if !self.start_panel_open() => self.open_project_dialog(false),
            "file.new" => {}
            "file.open" => self.open_project_dialog(true),
            "file.save" if has_project => self.save_active(false),
            "file.saveAs" if has_project => self.save_active(true),
            "menu.collectAndSave" if has_project => self.collect_and_save(),
            "file.saveTemplate" if has_project => self.save_template(),
            "menu.export" if has_project => self.open_export(),
            "menu.quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "edit.undo" => actions.push(Action::new("edit.undo", json!({}))),
            "edit.redo" => actions.push(Action::new("edit.redo", json!({}))),
            "project.next" => actions.push(Action::new("project.next", json!({}))),
            "project.previous" => actions.push(Action::new("project.previous", json!({}))),
            "view.session" if has_project => {
                self.shown = View::Session;
                self.settings_open = false;
            }
            "view.arrangement" if has_project => {
                self.shown = View::Arrangement;
                self.settings_open = false;
            }
            "view.sampleMapWindow" => self.toggle_map_window(),
            "view.sampleMap" => {
                self.settings_open = false;
                self.toggle_large_map();
            }
            "menu.missingFiles" if !self.missing.rows.is_empty() => {
                self.missing.open = true;
                self.settings_open = false;
            }
            "view.browser" => self.toggle_browser(),
            "view.detail" if has_project => {
                self.detail_open = !self.detail_open;
                self.editor_closed = false;
            }
            "view.editor" => self.toggle_editor(),
            "view.devices" => self.toggle_devices(),
            "view.zoomIn" => self.step_zoom(1),
            "view.zoomOut" => self.step_zoom(-1),
            "menu.actualSize" => {
                self.settings.zoom = crate::settings::ZOOM;
                self.settings_changed = true;
            }
            "menu.closeProject" => {
                if let Some(active) = self.projects.active() {
                    actions.push(Action::new("project.close", json!({ "project": active.0 })));
                }
            }
            "settings.open" => self.settings_open = true,
            "menu.checkUpdates" => self.updates.open_dialog(),
            "help.about" => self.about_open = true,
            recent if recent.starts_with("menu.recent.") => {
                let index = recent.trim_start_matches("menu.recent.").parse::<usize>().ok();
                if let Some(path) = index.and_then(|index| self.project_lists.recent.get(index)).cloned() {
                    if path.is_file() {
                        self.request_open(path);
                    } else {
                        self.status = Status::Error(format!("{} is no longer there", path.display()));
                    }
                }
            }
            _ => {}
        }
    }

    /// A pick from the system's menu bar, run with the next frame.
    pub fn queue_menu_item(&mut self, id: &str) {
        self.queued_menu_items.push(id.to_owned());
    }

    /// Asks for the recent projects and templates to be read again, for a menu that shows them
    /// without being opened first (the macOS bar).
    pub fn want_project_lists(&mut self) {
        self.lists_wanted = true;
    }
}
