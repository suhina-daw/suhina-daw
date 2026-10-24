// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings dialog and its audio and MIDI pages: devices listed, chosen and opened.

use super::*;

impl App {
    /// Lists the audio hosts and devices for Settings › Audio (`audio.devices`).
    pub(super) fn list_audio_devices(&mut self) {
        match self.projects.execute("audio.devices", &json!({})) {
            Ok(_) => {
                self.about.audio_hosts = self.projects.audio_system().map(|s| s.hosts()).unwrap_or_default();
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
        }
        self.list_midi_inputs();
    }

    /// Lists the MIDI input ports for Settings › Audio (`midi.inputs`).
    pub(super) fn list_midi_inputs(&mut self) {
        self.midi_listed = Some(std::time::Instant::now());
        self.about.midi_inputs = self.projects.execute("midi.inputs", &json!({})).map_err(String::from).map(|listed| {
            listed["inputs"]
                .as_array()
                .map(|ports| {
                    ports
                        .iter()
                        .map(|p| settings_view::MidiPort {
                            name: p["name"].as_str().unwrap_or_default().to_string(),
                            enabled: p["enabled"].as_bool().unwrap_or(false),
                            open: p["open"].as_bool().unwrap_or(false),
                            error: p["error"].as_str().map(String::from),
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
    }

    /// Turns a MIDI input port on or off (`midi.setInput`) and keeps the choice in the settings.
    pub(super) fn set_midi_input(&mut self, port: &str, enabled: bool) {
        match self.projects.execute("midi.setInput", &json!({ "port": port, "enabled": enabled })) {
            Ok(_) => {
                self.settings.midi_inputs_off = self.projects.midi_inputs_off();
                self.settings_changed = true;
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
        }
        self.list_midi_inputs();
    }

    /// Plays on the output `choice` names from now on (`audio.setOutput`): the active project's
    /// output opens again there, the transport where it was. When only the cue changed, the
    /// cue's command runs instead (`output.setCuePair`, `output.setCueVolume`).
    pub(super) fn choose_output(&mut self, choice: &suhina_session::OutputChoice) {
        let current = self.projects.output_choice().clone();
        if choice.device_only() == current.device_only() && choice.cue_pair == current.cue_pair {
            let db =
                if choice.cue_gain > 0.0 { json!(20.0 * f64::from(choice.cue_gain).log10()) } else { json!("-inf") };
            match self.projects.execute("output.setCueVolume", &json!({ "db": db })) {
                Ok(result) => {
                    let db =
                        result["cue_db"].as_f64().map_or_else(|| "silent".to_string(), |db| format!("{db:+.0} dB"));
                    self.status = Status::Done(format!("Cue volume: {db}"));
                }
                Err(e) => self.status = Status::Error(e.message().to_owned()),
            }
            return;
        }
        if choice.device_only() == current.device_only() {
            match self.projects.execute("output.setCuePair", &json!({ "pair": choice.cue_pair.unwrap_or(0) })) {
                Ok(result) => {
                    self.status = match (result["notice"].as_str(), choice.cue_pair) {
                        (Some(notice), _) => Status::Hint(notice.to_string()),
                        (None, Some(pair)) => {
                            let (first, second) = suhina_session::audio::cue_channels(pair);
                            Status::Done(format!("Cue: outputs {first}/{second}"))
                        }
                        (None, None) => Status::Done("Cue: with the main output".to_string()),
                    };
                }
                Err(e) => self.status = Status::Error(e.message().to_owned()),
            }
            return;
        }
        let params = choice.to_json();
        let params = json!({
            "host": params["host"].as_str().unwrap_or(""),
            "device": params["device"].as_str().unwrap_or(""),
            "sampleRate": choice.sample_rate.unwrap_or(0),
            "bufferFrames": choice.buffer_frames.unwrap_or(0),
            "exclusive": choice.exclusive,
            "input": choice.input.as_text(),
        });
        match self.projects.execute("audio.setOutput", &params) {
            Ok(result) => {
                self.status = match result["notice"].as_str() {
                    Some(notice) => Status::Hint(notice.to_string()),
                    None => Status::Done(format!(
                        "Audio output: {}",
                        self.session()
                            .and_then(Session::player)
                            .and_then(suhina_session::Player::output_info)
                            .map_or_else(|| "chosen; it opens with the next project".to_string(), |i| i.describe())
                    )),
                };
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
        }
    }

    /// Settings as a dialog over the window: the view stays behind it, dimmed, and its clicks
    /// are swallowed. Escape, Close and the Settings key close it.
    pub(super) fn settings_dialog(&mut self, ui: &Ui, window: Rect) {
        egui::Area::new(Id::new("settings-dialog")).order(egui::Order::Foreground).fixed_pos(window.min).show(
            ui.ctx(),
            |ui| {
                ui.set_min_size(window.size());
                let behind = ui.interact(window, Id::new("settings-behind"), Sense::click_and_drag());
                let _swallowed = behind.clicked();
                ui.painter().rect_filled(window, 0.0, theme::window_background().gamma_multiply(0.6));
                let size = Vec2::new(
                    SETTINGS_SIZE.x.min(window.width() - 2.0 * PAD),
                    SETTINGS_SIZE.y.min(window.height() - 2.0 * PAD),
                );
                let panel = Rect::from_center_size(window.center(), size);
                if self.plugins.manager.open {
                    // The plug-in manager takes the dialog's place until it is closed.
                    if !self.plugins_read {
                        self.read_plugins();
                    }
                    let out = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                            crate::settings::plugin_manager::show(ui, panel, &mut self.plugins)
                        })
                        .inner;
                    if out.close {
                        self.plugins.manager.open = false;
                    }
                    if let Some(request) = out.request {
                        self.plugin_request(request);
                    }
                } else {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| self.settings_page(ui));
                }
                ui.painter().rect_stroke(panel, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
            },
        );
    }

    /// The Settings page, applying each change as it is made.
    pub(super) fn settings_page(&mut self, ui: &mut Ui) {
        if !self.plugins_read {
            self.read_plugins();
        }
        let themes = self.themes.view(&self.settings.theme);
        self.about.projects_default = self.default_folders.0.display().to_string();
        self.about.templates_default = self.default_folders.1.display().to_string();
        self.about.nam_models_default = self.nam_default_folders.0.display().to_string();
        self.about.irs_default = self.nam_default_folders.1.display().to_string();
        self.about.mcp_project_folder =
            self.session().and_then(Session::source).and_then(std::path::Path::parent).map(|p| p.display().to_string());
        self.about.last_backup = match self.session().and_then(Session::last_backup) {
            Some(backup) => {
                format!("{} to {}", suhina_session::backup::date_time(backup.written), backup.path.display())
            }
            None => "None written for this project yet".into(),
        };
        let tag_limit = self.projects.file_library_mut().tag_limit();
        self.about.audio_live = self.session().and_then(Session::player).and_then(suhina_session::Player::output_info);
        if self.page.shows_devices() {
            // The latency and underruns change while it plays, and controllers come and go.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            if self.midi_listed.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2)) {
                self.list_midi_inputs();
            }
        }
        let sources = Sources {
            about: &self.about,
            plugins: &mut self.plugins,
            themes: &themes,
            keymap: &mut self.keymap,
            tag_limit,
        };
        let outcome = settings_view::show(ui, &mut self.settings, sources, &mut self.page);
        if let Some(request) = outcome.themes.clone() {
            if let Some(id) = self.themes.request(&self.settings.theme, request) {
                self.settings.theme = id;
                self.settings_changed = true;
            }
            theme::set_palette(ui.ctx(), self.themes.palette(&self.settings.theme));
        }
        if outcome.keymap_changed {
            self.keymap_changed = true;
        }
        if let Some(limit) = outcome.tag_limit
            && let Err(e) = self.projects.execute("library.setTag", &json!({ "limit": limit }))
        {
            self.status = Status::Error(e.message().to_owned());
        }
        if let Some(request) = outcome.plugins.clone() {
            self.plugin_request(request);
        }
        if outcome.audio_refresh {
            self.list_audio_devices();
        }
        if let Some(field) = outcome.pick {
            self.ask_pick(field);
        }
        if let Some(choice) = outcome.audio_output.clone() {
            self.choose_output(&choice);
        }
        if let Some((port, enabled)) = outcome.midi_input.clone() {
            self.set_midi_input(&port, enabled);
        }
        if outcome.changed {
            self.projects.set_backup_settings(self.settings.backup, self.settings.backup_clean_up);
            motion::set_enabled(ui.ctx(), self.settings.animations);
            self.arrangement.headers_left = self.settings.track_headers == Side::Left;
            self.arrangement.returns_hidden = self.settings.view.returns_hidden;
            self.arrangement.waveform_zoom = self.settings.view.waveform_zoom as f32 / 100.0;
            self.settings_changed = true;
        }
        if outcome.close {
            self.settings_open = false;
        }
    }
}
