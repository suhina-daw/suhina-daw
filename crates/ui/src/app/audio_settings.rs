// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings dialog and its audio and MIDI pages: devices listed, chosen and opened.

use super::*;

impl App {
    /// Asks the ASIO driver for its buffer sizes again (`audio.checkDriver`) when Settings › Audio
    /// opens or closes, and when the window gets the focus back while it shows: a size set in the
    /// driver's control panel that the driver did not announce opens the output again, with no
    /// refresh to press. Asking is cheap: the driver playing is asked on its own thread.
    pub(super) fn follow_audio_page(&mut self, ctx: &egui::Context) {
        let shown = self.settings_open && self.page.shows_devices();
        let viewport =
            if self.page.window { egui::ViewportId::from_hash_of("settings-window") } else { egui::ViewportId::ROOT };
        // The root frame receives metadata for every native viewport. Its own focus
        // need not change when the driver panel returns focus to floating Settings.
        let focused = ctx.input(|i| i.raw.viewports.get(&viewport).and_then(|v| v.focused).unwrap_or(false));
        let returned = shown && focused && !self.window_focused;
        if shown != self.audio_page_shown || returned {
            // No output open, or not ASIO: nothing to ask.
            let _ = self.projects.execute("audio.checkDriver", &json!({}));
        }
        self.audio_page_shown = shown;
        self.window_focused = focused;
    }

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
                Err(e) => {
                    // A rejected command leaves the old choice; an open failure keeps the
                    // requested choice for retry. Reflect whichever the session retained.
                    self.settings.set_output_choice(self.projects.output_choice());
                    self.status = Status::Error(e.message().to_owned());
                }
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
                Err(e) => {
                    // A rejected command leaves the old choice; an open failure keeps the
                    // requested choice for retry. Reflect whichever the session retained.
                    self.settings.set_output_choice(self.projects.output_choice());
                    self.status = Status::Error(e.message().to_owned());
                }
            }
            return;
        }
        let previous_output_open =
            self.session().and_then(Session::player).and_then(suhina_session::Player::output_info).is_some();
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
                let default = self.about.audio_hosts.iter().find(|h| h.default).map(|h| h.name.as_str());
                // A failed choice remains selected for retry, but is not a working device.
                if previous_output_open {
                    self.settings.remember_output(&current, default);
                }
                self.settings.remember_output(choice, default);
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
            Err(e) => {
                // A rejected command leaves the old choice; an open failure keeps the
                // requested choice for retry. Reflect whichever the session retained.
                self.settings.set_output_choice(self.projects.output_choice());
                self.status = Status::Error(e.message().to_owned());
            }
        }
    }

    /// New audio tracks hear the default track input the settings keep for the audio device
    /// chosen (`audio.setTrackInput`, #1191); the tracks that are there keep theirs.
    pub(super) fn choose_track_input(&mut self) {
        let input = suhina_session::audio::input_channels_text(self.settings.track_input());
        match self.projects.execute("audio.setTrackInput", &json!({ "input": input })) {
            Ok(result) => {
                let shown = match result["track_input"].as_str() {
                    Some("none") | None => "none".to_string(),
                    Some(input) => format!("In {input}"),
                };
                self.status = Status::Done(format!("Default track input on this audio device: {shown}"));
            }
            Err(e) => {
                let kept = self.projects.track_input();
                self.settings.set_track_input(kept);
                self.status = Status::Error(e.message().to_owned());
            }
        }
    }

    /// Settings can stay over the main view or move into an independent native window.
    pub(super) fn settings_dialog(&mut self, ui: &Ui, window: Rect) {
        if suhina_plugins::trace::on() {
            suhina_plugins::trace::state(
                "settings placement",
                if self.page.window { "native" } else { "embedded" }.into(),
            );
        }
        if self.page.window {
            let ctx = ui.ctx().clone();
            let builder = egui::ViewportBuilder::default()
                .with_title("Settings - Suhina")
                .with_decorations(true)
                .with_resizable(true)
                .with_inner_size(SETTINGS_SIZE)
                .with_min_inner_size([600.0, 320.0]);
            suhina_plugins::trace::timed("settings native viewport", || {
                ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings-window"), builder, |ui, _| {
                    let whole = ui.available_rect_before_wrap();
                    // The main window's status bar may be out of sight: this window has its own.
                    let top = (whole.bottom() - theme::STATUS_BAR).max(whole.top());
                    let panel = Rect::from_min_max(whole.min, Pos2::new(whole.right(), top));
                    let bar = Rect::from_min_max(Pos2::new(whole.left(), top), whole.max);
                    suhina_plugins::trace::timed("settings native contents", || self.settings_contents(ui, panel));
                    let status_bar = crate::kit::element::STATUS_BAR.enter();
                    let _status = ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                        self.status_bar(ui, &status_bar, None)
                    });
                    if ui.input(|input| {
                        input.viewport().close_requested()
                            || (self.page.capture.is_none() && input.modifiers.command && input.key_pressed(Key::Comma))
                    }) {
                        self.settings_open = false;
                    }
                });
            });
            return;
        }
        crate::kit::floating::area(ui.ctx(), Id::unique("settings-dialog"), crate::kit::floating::Kind::Own)
            .order(egui::Order::Foreground)
            .fixed_pos(window.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(window.size());
                let behind = ui.interact(window, Id::unique("settings-behind"), Sense::click_and_drag());
                let _swallowed = behind.clicked();
                let tint = ui.painter().add(egui::Shape::Noop);
                // The panel stays above the status bar, which shows what the page reports.
                let mut bounds = window.shrink(PAD);
                bounds.max.y = bounds.max.y.min(window.bottom() - theme::STATUS_BAR);
                let size = Vec2::new(SETTINGS_SIZE.x.min(bounds.width()), SETTINGS_SIZE.y.min(bounds.height()));
                let position = ui
                    .ctx()
                    .data(|data| data.get_temp::<Pos2>(Id::unique("settings-panel-position")))
                    .unwrap_or(window.center() - size / 2.0);
                let position = egui::pos2(
                    position.x.clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left())),
                    position.y.clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top())),
                );
                let panel = Rect::from_min_size(position, size);
                // Only holding a control clears the dimming: a click that changes a setting or
                // chooses a theme does not, or the window behind would flash for one frame.
                let previewing = self.settings_contents(ui, panel);
                if suhina_plugins::trace::on() {
                    suhina_plugins::trace::state("settings overlay", format!("previewing={previewing} rect={panel:?}"));
                }
                if !previewing {
                    // The status bar stays undimmed at the bottom: an error or a confirmation from
                    // the page is seen while the dialog is open, not only behind it.
                    let top = (window.bottom() - theme::STATUS_BAR).max(window.min.y);
                    let dim = Rect::from_min_max(window.min, Pos2::new(window.right(), top));
                    ui.painter().set(
                        tint,
                        egui::Shape::rect_filled(
                            dim,
                            0.0,
                            crate::kit::style_class::opaque_surface_or(theme::window_background()).gamma_multiply(0.6),
                        ),
                    );
                }
                ui.painter().rect_stroke(panel, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
            });
    }

    pub(super) fn settings_contents(&mut self, ui: &mut Ui, panel: Rect) -> bool {
        if self.plugins.manager.open {
            if !self.plugins_read {
                self.read_plugins();
            }
            let out = ui
                .scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                    settings_view::input_style(ui);
                    crate::settings::plugin_manager::show(ui, panel, &mut self.plugins)
                })
                .inner;
            if out.close {
                self.plugins.manager.open = false;
            }
            if let Some(request) = out.request {
                self.plugin_request(request);
            }
            false
        } else {
            ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| self.settings_page(ui)).inner
        }
    }

    /// The Settings page, applying each change as it is made.
    pub(super) fn settings_page(&mut self, ui: &mut Ui) -> bool {
        if !self.plugins_read {
            self.read_plugins();
        }
        let mut themes = self.themes.view(&self.settings.theme);
        themes.global_liquid = self.settings.view.liquid;
        self.about.projects_default = self.default_folders.0.display().to_string();
        self.about.templates_default = self.default_folders.1.display().to_string();
        self.about.themes_default = self.themes_default_folder.display().to_string();
        self.about.data_default = self.data_default_folder.display().to_string();
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
            let names = self.projects.audio_system().and_then(|system| system.device_names());
            // An output opened again (after a size set in the ASIO driver's control panel, say)
            // has asked its driver for its sizes anew, so the list is made again from them. What
            // the output measures per callback is not followed: it changes from one callback to
            // the next on some hosts, and the list would be made again many times a second.
            let opened = self.projects.outputs_opened();
            if self.audio_devices.changed(names) | self.audio_output_watch.changed(Some(opened)) {
                self.list_audio_devices();
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
            if let Some(notice) = self.themes.take_notice() {
                self.status = Status::Hint(notice);
            }
            theme::set_palette(ui.ctx(), self.themes.palette(&self.settings.theme));
        }
        if outcome.keymap_changed {
            self.keymap_changed = true;
        }
        if let Some(limit) = outcome.tag_limit
            && let Err(e) = self.projects.execute("library.setTag", &json!({ "limit": limit }))
        {
            self.status = Status::Error(e.message().to_string());
        }
        if let Some(request) = outcome.plugins.clone() {
            self.plugin_request(request);
        }
        if outcome.audio_refresh {
            self.list_audio_devices();
        }
        if outcome.audio_control_panel
            && let Err(error) = self.projects.execute("audio.openControlPanel", &json!({}))
        {
            self.status = Status::Error(error.message().to_string());
        }
        if let Some(field) = outcome.pick {
            self.ask_pick(field);
        }
        if let Some(field) = outcome.folder_reset {
            self.folder_move = Some((field, None));
        }
        if let Some(choice) = outcome.audio_output.clone() {
            self.choose_output(&choice);
        }
        if let Some((port, enabled)) = outcome.midi_input.clone() {
            self.set_midi_input(&port, enabled);
        }
        if outcome.changed && self.settings.track_input() != self.projects.track_input() {
            self.choose_track_input();
        }
        if outcome.changed {
            // Each audio device keeps its input and default track input as the session has them.
            self.settings.keep_device_inputs(self.projects.device_inputs());
            self.projects.set_backup_settings(self.settings.backup, self.settings.backup_clean_up);
            motion::set_enabled(ui.ctx(), self.settings.animations);
            crate::kit::widgets::set_drag_hides_pointer(ui.ctx(), self.settings.drag_hides_pointer);
            self.arrangement.headers_left = headers_left(self.settings.track_headers);
            self.arrangement.returns_hidden = self.settings.view.returns_hidden;
            self.arrangement.waveform_zoom = self.settings.view.waveform_zoom as f32 / 100.0;
            self.arrangement.waveform_appearance = self.settings.view.waveform;
            self.arrangement.audio_colour_strength = self.settings.view.audio_colour_strength;
            self.arrangement.header_colour_strength = self.settings.view.header_colour_strength;
            self.settings_changed = true;
        }
        if outcome.close {
            self.settings_open = false;
        }
        if outcome.move_by.is_finite() && outcome.move_by != Vec2::ZERO {
            let position = ui.max_rect().min + outcome.move_by;
            ui.ctx().data_mut(|data| data.insert_temp(Id::unique("settings-panel-position"), position));
            ui.ctx().request_repaint();
        }
        outcome.adjusting
    }
}

#[cfg(test)]
#[path = "audio_settings_tests.rs"]
mod tests;
