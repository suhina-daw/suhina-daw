// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's menu bar and its Open recent menu.

use super::*;

impl App {
    /// File › Open recent: the recent projects the project dialog lists, newest first, each opened
    /// as File › Open… opens a file. The list is read again each time the File menu opens.
    pub(super) fn open_recent_menu(&mut self, ui: &mut Ui, was_open: bool) {
        self.file_menu_open = true;
        if !was_open {
            self.lists_wanted = true;
        }
        let recent: Vec<PathBuf> = self.project_lists.recent.iter().take(RECENT_IN_MENU).cloned().collect();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for path in recent {
                    let name =
                        path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        if path.is_file() {
                            self.request_open(path);
                        } else {
                            self.status = Status::Error(format!("{} is no longer there", path.display()));
                        }
                    }
                }
            });
        });
    }

    /// The menu bar: the same actions as the shortcuts and the views' controls.
    pub(super) fn menu_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let has_project = self.session().is_some();
        let (can_undo, can_redo) = self.session().map_or((false, false), |s| (s.can_undo(), s.can_redo()));
        let several = self.projects.len() >= 2;
        let keymap = self.keymap.clone();
        let keys = move |action: &str| keymap.label(action).unwrap_or_default();
        ui.style_mut().visuals.widgets.inactive.fg_stroke = theme::hairline(theme::text_secondary());
        // Whether the File menu showed last frame, so its recent list is read once as it opens.
        let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
        egui::MenuBar::new().ui(ui, |ui| {
            bar_menu(ui, "File", |ui| {
                // The start panel already offers new and open; the dialog would only cover it.
                if ui.add(egui::Button::new("New or open…").shortcut_text(keys("file.new"))).clicked()
                    && !self.start_panel_open()
                {
                    self.open_project_dialog(false);
                }
                if ui.add(egui::Button::new("Open…").shortcut_text(keys("file.open"))).clicked() {
                    self.open_project_dialog(true);
                }
                self.open_recent_menu(ui, file_menu_was_open);
                ui.separator();
                if ui.add_enabled(has_project, egui::Button::new("Save").shortcut_text(keys("file.save"))).clicked() {
                    self.save_active(false);
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Save as…").shortcut_text(keys("file.saveAs")))
                    .clicked()
                {
                    self.save_active(true);
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Collect and save"))
                    .on_hover_text("Copy the audio the project plays from other folders into its own folder, then save")
                    .clicked()
                {
                    self.collect_and_save();
                }
                if ui
                    .add_enabled(
                        has_project,
                        egui::Button::new("Save as template…").shortcut_text(keys("file.saveTemplate")),
                    )
                    .clicked()
                {
                    self.save_template();
                }
                if ui.add_enabled(has_project, egui::Button::new("Export…")).clicked() {
                    self.open_export();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            bar_menu(ui, "Edit", |ui| {
                if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text(keys("edit.undo"))).clicked() {
                    actions.push(Action::new("edit.undo", json!({})));
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text(keys("edit.redo"))).clicked() {
                    actions.push(Action::new("edit.redo", json!({})));
                }
            });
            bar_menu(ui, "View", |ui| {
                if ui
                    .add_enabled(has_project, egui::Button::new("Session").shortcut_text(keys("view.session")))
                    .clicked()
                {
                    self.shown = View::Session;
                    self.settings_open = false;
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Arrangement").shortcut_text(keys("view.arrangement")))
                    .clicked()
                {
                    self.shown = View::Arrangement;
                    self.settings_open = false;
                }
                let label = if self.sample_map.view.window { "Dock sample map" } else { "Sample map in a window" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.sampleMapWindow"))).clicked() {
                    self.toggle_map_window();
                }
                let label = if self.sample_map.view.large { "Hide sample map" } else { "Sample map" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.sampleMap"))).clicked() {
                    self.settings_open = false;
                    self.toggle_large_map();
                }
                let missing = self.missing.rows.len();
                let label =
                    if missing == 0 { "Missing files…".to_string() } else { format!("Missing files ({missing})…") };
                if ui.add_enabled(missing > 0, egui::Button::new(label)).clicked() {
                    self.missing.open = true;
                    self.settings_open = false;
                }
                let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.browser"))).clicked() {
                    self.toggle_browser();
                }
                let label = if self.detail_open { "Hide detail area" } else { "Show detail area" };
                if ui.add_enabled(has_project, egui::Button::new(label).shortcut_text(keys("view.detail"))).clicked() {
                    self.detail_open = !self.detail_open;
                    self.editor_closed = false;
                }
                // The selected clip's editor (› Least height and closing in ui-clip-editor.md).
                let label = if self.detail_open { "Hide editor" } else { "Show editor" };
                let clip = has_project && self.shown_clip().is_some();
                if ui.add_enabled(clip, egui::Button::new(label).shortcut_text(keys("view.editor"))).clicked() {
                    self.toggle_editor();
                }
                // The detail area's Clip / Devices switch, for a clip with an editor.
                let devices = self.detail_open && self.detail_showing == detail_view::Showing::Devices;
                let label = if devices { "Show clip" } else { "Show devices" };
                let editor = has_project && self.piano_clip(self.shown_clip()).is_some();
                if ui.add_enabled(editor, egui::Button::new(label).shortcut_text(keys("view.devices"))).clicked() {
                    self.toggle_devices();
                }
                ui.separator();
                let (zoom, limits) = (self.settings.zoom, crate::settings::ZOOMS);
                if ui
                    .add_enabled(zoom < limits.1, egui::Button::new("Zoom in").shortcut_text(keys("view.zoomIn")))
                    .clicked()
                {
                    self.step_zoom(1);
                }
                if ui
                    .add_enabled(zoom > limits.0, egui::Button::new("Zoom out").shortcut_text(keys("view.zoomOut")))
                    .clicked()
                {
                    self.step_zoom(-1);
                }
                let reset = egui::Button::new(format!("Actual size ({zoom} %)")).shortcut_text(keys("view.zoomReset"));
                if ui.add_enabled(zoom != crate::settings::ZOOM, reset).clicked() {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            });
            bar_menu(ui, "Projects", |ui| {
                if ui
                    .add_enabled(several, egui::Button::new("Next project").shortcut_text(keys("project.next")))
                    .clicked()
                {
                    actions.push(Action::new("project.next", json!({})));
                }
                if ui
                    .add_enabled(several, egui::Button::new("Previous project").shortcut_text(keys("project.previous")))
                    .clicked()
                {
                    actions.push(Action::new("project.previous", json!({})));
                }
                if let Some(active) = self.projects.active()
                    && ui.button("Close project").clicked()
                {
                    actions.push(Action::new("project.close", json!({ "project": active.0 })));
                }
            });
            bar_menu(ui, "Options", |ui| {
                if ui.add(egui::Button::new("Settings").shortcut_text(keys("settings.open"))).clicked() {
                    self.settings_open = true;
                }
            });
            bar_menu(ui, "Help", |ui| {
                if ui.button("Check for updates…").clicked() {
                    self.updates.open_dialog();
                }
                if ui.add(egui::Button::new("About Suhina").shortcut_text(keys("help.about"))).clicked() {
                    self.about_open = true;
                }
            });
        });
    }
}

/// One menu of the menu bar. While another of the bar's menus is open, the pointer over this
/// one's title opens it, as system menu bars do.
pub(super) fn bar_menu<R>(
    ui: &mut Ui,
    title: &str,
    content: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let shown = ui.menu_button(title, content);
    let id = egui::Popup::default_response_id(&shown.response);
    if shown.response.hovered() && !egui::Popup::is_id_open(ui.ctx(), id) && egui::Popup::is_any_open(ui.ctx()) {
        egui::Popup::open_id(ui.ctx(), id);
    }
    shown
}
