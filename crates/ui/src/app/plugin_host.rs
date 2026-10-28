// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's side of plug-in hosting: scans, the plug-in manager's requests, and where editors open.

use super::*;

impl App {
    /// The shown track's plug-in: the panel's pick when it is one of the track's plug-ins, else
    /// the first running plug-in in the chain. Its editor opens or closes, whichever state it
    /// has (the command and the placement machinery do the rest).
    pub(super) fn toggle_plugin_editor(&mut self) {
        let shown = match self.shown {
            View::Session => self.view.selected,
            View::Arrangement => self.arrangement.selected_tracks.first().copied(),
        };
        let Some(track) = shown else {
            self.status = Status::Hint("Click a track first".into());
            return;
        };
        // The first running plug-in in the chain is the one the key toggles. (Now that the
        // device panel selects a device, the panel's pick can stand in its place; that comes
        // with the device selection (#554).)
        let target = {
            let Ok(session) = self.projects.session_mut(None) else { return };
            let chain = session
                .project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|c| session.project().channel(c))
                .map_or(0, |c| c.devices.len());
            (0..chain)
                .filter_map(|index| Some((index, session.plugin_report(track, index)?)))
                .find(|(_, report)| report.status == PluginStatus::Running)
                .map(|(index, report)| (index, report.editor_open))
        };
        let Some((index, open)) = target else {
            self.status = Status::Hint("The selected track has no running plug-in".into());
            return;
        };
        let command = if open { "device.closeEditor" } else { "device.openEditor" };
        self.run(&Action::new(command, json!({ "track": track.0, "device": index })));
    }

    /// How many devices `track`'s chain holds, if the track is in the project.
    pub(super) fn device_count(&self, track: TrackId) -> Option<usize> {
        self.session().and_then(|s| {
            s.project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|c| s.project().channel(c))
                .map(|ch| ch.devices.len())
        })
    }

    /// Keeps the active project's plug-ins in line with it (ADR 0006) and shows what the user
    /// should hear about in the status bar. While the project has plug-ins, the window looks
    /// again a few times a second, so one that starts or crashes is noticed without input.
    pub(super) fn poll_plugins(&mut self, ctx: &egui::Context) {
        // A host's unasked news (Space in a plug-in's window, an ARA plug-in's transport request)
        // wakes the window at once (#714).
        if !self.waker_set {
            let ctx = ctx.clone();
            suhina_plugins::set_waker(move || ctx.request_repaint());
            self.waker_set = true;
        }
        self.projects.set_editor_embedding(self.settings.plugin_editors == EditorPlacement::DeviceView);
        // Tracks keep the colour they show when others come and go (#690, #709).
        for id in self.projects.ids() {
            let Ok(session) = self.projects.session_mut(Some(id)) else { continue };
            let palette = crate::interaction::grid::track_palette();
            let fixed = self
                .settings
                .view
                .fixed_track_colour
                .then(|| crate::interaction::grid::to_rgb(theme::midi_clip_default()));
            session.set_track_colours(palette.to_vec(), fixed);
            session.set_track_type_colours(
                crate::interaction::grid::to_rgb(theme::midi_clip_default()),
                crate::interaction::grid::to_rgb(theme::audio_clip_default()),
            );
            session.set_new_clips_take_track_colour(self.settings.view.new_clip_track_colour);
        }
        let notices = self.projects.poll_plugins();
        if let Some(last) = notices.last() {
            self.status = Status::Hint(last.clone());
        }
        if self
            .session()
            .is_some_and(|s| s.player().is_some() && !suhina_engine::plugin_devices(s.project()).is_empty())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// While the active project's plug-ins start, a dialog over the window says how far they
    /// have got, until it is sent to the background or nothing is loading any more.
    pub(super) fn plugin_dialog(&mut self, ui: &Ui, window: Rect) {
        if self.plugin_dialog_hidden || self.settings_open {
            return;
        }
        let Some(id) = self.projects.active() else { return };
        let Some(progress) = self.projects.get(id).map(Session::plugin_progress) else { return };
        if progress.loading.is_empty() {
            return;
        }
        let name = self.projects.name(id).unwrap_or("the project").to_string();
        if loading_dialog::show(ui, window, &name, &progress) {
            self.plugin_dialog_hidden = true;
        }
    }

    /// Whether something drawn over the whole window is open (Settings or a dialog): an embedded
    /// plug-in editor is a native window egui cannot draw over, so it is hidden meanwhile (#723).
    pub(super) fn over_the_window(&self) -> bool {
        (self.settings_open && !self.page.window)
            || self.about_open
            || self.theme_import.is_some()
            || self.updates.open
            || self.project_dialog.is_some()
            || self.save_prompt.is_some()
            || self.export_dialog.is_some()
            || self.unsaved_prompt.is_some()
            || self.ara_prompt.is_some()
            || self.missing.open
            || self.recovery_open.is_some()
            || !self.recovery.is_empty()
            || !self.agent_asks.is_empty()
    }

    /// Tells the active project where the panel shows its embedded plug-in editors this frame
    /// (none: they are hidden).
    pub(super) fn place_editors(&mut self, places: &[detail_view::EditorPlace]) {
        // One placement a frame: with something over the window the editors are told to hide,
        // never shown first and hidden after (which left them over Settings).
        let places = if self.over_the_window() { &[][..] } else { places };
        let Ok(session) = self.projects.session_mut(None) else { return };
        let shown: Vec<_> = places.iter().map(|p| (p.track, p.device, p.placement)).collect();
        session.place_editors(&shown);
        let fits: Vec<_> = places.iter().filter_map(|p| Some((p.track, p.device, p.fit?))).collect();
        if !fits.is_empty() {
            session.fit_editors(&fits);
        }
    }

    /// Reads the plug-in library through its commands, for the Settings page and the plug-in
    /// manager.
    pub(super) fn read_plugins(&mut self) {
        self.plugins_read = true;
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let empty = Vec::new();
        match self.projects.execute("plugin.folders", &json!({})) {
            Ok(folders) => {
                self.plugins.folders = folders["folders"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|f| settings_view::FolderRow {
                        path: text(&f["folder"]),
                        format: text(&f["format"]),
                        enabled: f["enabled"].as_bool().unwrap_or(true),
                        standard: f["standard"].as_bool().unwrap_or(false),
                    })
                    .collect();
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
        }
        match self.projects.execute("plugin.list", &json!({})) {
            Ok(found) => {
                self.plugins.found = found["plugins"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|p| {
                        let file = text(&p["file"]);
                        let format = text(&p["format"]);
                        let spec = format!("{format}:{file}#{}", text(&p["id"]));
                        settings_view::PluginRow {
                            name: text(&p["name"]),
                            vendor: text(&p["vendor"]),
                            kind: text(&p["type"]),
                            format,
                            version: text(&p["version"]),
                            file,
                            spec,
                            enabled: p["enabled"].as_bool().unwrap_or(true),
                        }
                    })
                    .collect();
                let reasons = |list: &serde_json::Value| -> Vec<(String, String)> {
                    list.as_array().unwrap_or(&empty).iter().map(|f| (text(&f["file"]), text(&f["reason"]))).collect()
                };
                self.plugins.failed = reasons(&found["failed"]);
                self.plugins.incompatible = reasons(&found["incompatible"]);
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
        }
    }

    /// Does what the Settings page asked of the plug-in library. Folders change through their
    /// commands; a scan runs `Library::scan` (what `plugin.scan` runs) on a copy of the library
    /// on another thread, so the window keeps drawing, and the result replaces the library.
    pub(super) fn plugin_request(&mut self, request: PluginRequest) {
        // A scan's result replaces the library, so a folder or switch changed meanwhile would be lost.
        if self.plugins.scanning
            && matches!(
                request,
                PluginRequest::AddFolder(..)
                    | PluginRequest::PickFolder
                    | PluginRequest::RemoveFolder(..)
                    | PluginRequest::SetFolderEnabled(..)
                    | PluginRequest::SetEnabled(..)
                    | PluginRequest::SetEnabledMany(..)
            )
        {
            self.status =
                Status::Error("Wait for the scan to finish before changing plug-in folders or switches".into());
            return;
        }
        match request {
            PluginRequest::AddFolder(folder, format) => {
                let params = json!({ "folder": folder, "format": format.name() });
                match self.projects.execute("plugin.addFolder", &params) {
                    Ok(_) => {
                        self.plugins.new_folder.clear();
                        self.status = Status::Hint(format!("Added {folder}; Scan looks in it for plug-ins"));
                    }
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
                }
                self.plugins_read = false;
            }
            PluginRequest::PickFolder => self.ask_pick(PickField::PluginFolder),
            PluginRequest::SetFolderEnabled(folder, enabled) => {
                let params = json!({ "folder": folder, "enabled": enabled });
                if let Err(e) = self.projects.execute("plugin.setFolderEnabled", &params) {
                    self.status = Status::Error(e.message().to_owned());
                } else {
                    self.status = Status::Hint(format!(
                        "{folder} is {}; the next scan {}",
                        if enabled { "on" } else { "off" },
                        if enabled { "looks in it" } else { "drops its plug-ins" }
                    ));
                }
                self.plugins_read = false;
            }
            PluginRequest::SetEnabled(spec, enabled) => {
                match self.projects.execute("plugin.setEnabled", &json!({ "plugin": spec, "enabled": enabled })) {
                    Ok(result) => {
                        let name = result["plugin"].as_str().unwrap_or_default().to_string();
                        self.status = Status::Done(if enabled {
                            format!("{name} is on")
                        } else {
                            format!("{name} is off: not offered, and not loaded")
                        });
                        // The browser lists what sessions see, so it is listed again.
                        self.browser.relist();
                    }
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
                }
                self.plugins_read = false;
            }
            PluginRequest::SetEnabledMany(specs, enabled) => {
                let mut done = 0;
                for spec in &specs {
                    match self.projects.execute("plugin.setEnabled", &json!({ "plugin": spec, "enabled": enabled })) {
                        Ok(_) => done += 1,
                        Err(e) => {
                            self.status = Status::Error(e.message().to_owned());
                            break;
                        }
                    }
                }
                if done == specs.len() {
                    self.status = Status::Done(format!("{done} plug-ins are {}", if enabled { "on" } else { "off" }));
                }
                self.browser.relist();
                self.plugins_read = false;
            }
            PluginRequest::ScanFile(file) => {
                let mut library = self.projects.library_mut().clone();
                let (done, receive) = std::sync::mpsc::channel();
                let started = std::thread::Builder::new().name("suhina-plugin-scan".into()).spawn(move || {
                    let report =
                        library.scan_file_again(std::path::Path::new(&file)).map(|r| (r.read.len(), r.failed.len()));
                    let _unheard = done.send(ScanDone { library, report });
                });
                match started {
                    Ok(_) => {
                        self.scan = Some(receive);
                        self.plugins.scanning = true;
                        self.status = Status::Hint("Scanning the file again".into());
                    }
                    Err(e) => self.status = Status::Error(format!("could not start the scan: {e}")),
                }
            }
            PluginRequest::RemoveFolder(folder) => {
                if let Err(e) = self.projects.execute("plugin.removeFolder", &json!({ "folder": folder })) {
                    self.status = Status::Error(e.message().to_owned());
                }
                self.plugins_read = false;
            }
            PluginRequest::Scan { all } => {
                let mut library = self.projects.library_mut().clone();
                let (done, receive) = std::sync::mpsc::channel();
                let started = std::thread::Builder::new().name("suhina-plugin-scan".into()).spawn(move || {
                    let report = library.scan(all).map(|r| (r.read.len(), r.failed.len()));
                    let _unheard = done.send(ScanDone { library, report });
                });
                match started {
                    Ok(_) => {
                        self.scan = Some(receive);
                        self.plugins.scanning = true;
                        self.status = Status::Hint("Scanning for plug-ins".into());
                    }
                    Err(e) => self.status = Status::Error(format!("could not start the scan: {e}")),
                }
            }
        }
    }

    /// Picks up a finished background scan.
    pub(super) fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(receive) = &self.scan else { return };
        match receive.try_recv() {
            Ok(ScanDone { library, report }) => {
                *self.projects.library_mut() = library;
                self.status = match report {
                    Ok((read, failed)) => Status::Done(format!("Scan done: {read} files read, {failed} failed")),
                    Err(e) => Status::Error(e),
                };
                self.scan = None;
                self.plugins.scanning = false;
                self.plugins_read = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = Status::Error("the plug-in scan stopped without a result".into());
                self.scan = None;
                self.plugins.scanning = false;
            }
        }
    }

    /// Whether a plug-in scan is running.
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Hides the selected clip's editor (the detail area), or shows it again.
    pub(super) fn toggle_editor(&mut self) {
        if self.detail_open {
            self.hide_editor();
        } else {
            self.show_editor();
        }
    }
}
