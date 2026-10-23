// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Project tabs: the tab row, Ctrl+Tab's switcher, and what each tab keeps of the views.

use super::*;

impl App {
    /// Puts the shown project's view state aside, under its id.
    pub(super) fn stash_shown(&mut self) {
        let Some(id) = self.shown_project.take() else { return };
        if self.projects.get(id).is_none() {
            return;
        }
        let stash = Stash {
            view: std::mem::take(&mut self.view),
            arrangement: Some(std::mem::take(&mut self.arrangement)),
            selected_device: self.selected_device.take(),
            // The removal is another project's: its undo and redo say nothing here.
            revision: self.revision,
            timeline: self.timeline.take(),
            waveforms: std::mem::take(&mut self.waveforms),
        };
        self.pending_device_remove = None;
        self.stashes.insert(id, stash);
    }

    /// Shows project `id` with `stash` as its view state; the window's own preferences (the
    /// header column's side and width) carry over.
    pub(super) fn restore(&mut self, id: ProjectId, stash: Stash) {
        let (headers_left, header_width) = (self.arrangement.headers_left, self.arrangement.header_width);
        let returns_hidden = self.arrangement.returns_hidden;
        let waveform_zoom = self.arrangement.waveform_zoom;
        self.view = stash.view;
        self.arrangement = stash.arrangement.unwrap_or_default();
        self.arrangement.headers_left = headers_left;
        self.arrangement.returns_hidden = returns_hidden;
        self.arrangement.waveform_zoom = waveform_zoom;
        self.arrangement.header_width = header_width;
        self.selected_device = stash.selected_device;
        self.revision = stash.revision.wrapping_add(1);
        self.timeline = stash.timeline;
        self.waveforms = stash.waveforms;
        self.shown_project = Some(id);
    }

    /// Follows a change of the active project (a tab switched or closed): swaps the view state
    /// and asks the app for an audio output when the newly active project has none.
    pub(super) fn follow_active(&mut self) {
        let active = self.projects.active();
        if active == self.shown_project {
            return;
        }
        self.stash_shown();
        for gone in self.stashes.keys().copied().filter(|id| self.projects.get(*id).is_none()).collect::<Vec<_>>() {
            self.stashes.remove(&gone);
        }
        match active {
            Some(id) => {
                let stash = self.stashes.remove(&id).unwrap_or_default();
                self.restore(id, stash);
                if self.projects.get(id).is_some_and(|s| s.player().is_none()) {
                    self.player_request = Some(id);
                }
            }
            None => {
                self.shown_project = None;
                self.view = SessionViewState::default();
                self.timeline = None;
            }
        }
    }

    /// The project switcher, while Ctrl+Tab holds it open: the open projects, the one picked
    /// lit. Letting go of Ctrl (Cmd) switches to it, a click on one switches there, Escape
    /// leaves the project as it is.
    pub(super) fn tab_switcher_ui(&mut self, ctx: &egui::Context) {
        let Some(picked) = self.tab_switcher else { return };
        let ids = self.projects.ids();
        if ids.len() < 2 || picked >= ids.len() {
            self.tab_switcher = None;
            return;
        }
        let (held, escape) = ctx.input(|i| (i.modifiers.command, i.key_pressed(egui::Key::Escape)));
        if escape {
            self.tab_switcher = None;
            return;
        }
        // Down and Up move through the list too, as Tab and Shift+Tab do; the keys go no further
        // while it is open (#681).
        let (down, up) = ctx.input_mut(|i| {
            let mut moves = (0usize, 0usize);
            i.events.retain(|event| match event {
                egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, .. } => {
                    moves.0 += 1;
                    false
                }
                egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, .. } => {
                    moves.1 += 1;
                    false
                }
                _ => true,
            });
            moves
        });
        let count = ids.len();
        let picked = (picked + down % count + count - up % count) % count;
        self.tab_switcher = Some(picked);
        let mut chosen = None;
        egui::Area::new(egui::Id::new("project-switcher"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(360.0);
                    ui.label(egui::RichText::new("Open projects").color(theme::text_hint()).size(theme::TEXT_SMALL));
                    for (place, id) in ids.iter().enumerate() {
                        let name = self.projects.name(*id).unwrap_or("Untitled").to_string();
                        let source = self
                            .projects
                            .get(*id)
                            .and_then(Session::source)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Not saved yet".into());
                        let lit = place == picked;
                        let text = egui::RichText::new(format!("{name}\n{source}")).color(if lit {
                            theme::text_primary()
                        } else {
                            theme::text_secondary()
                        });
                        let row = ui.add(egui::Button::selectable(lit, text).min_size(Vec2::new(360.0, 34.0)));
                        row.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Button, true, lit, format!("Switch to {name}"))
                        });
                        if row.clicked() {
                            chosen = Some(place);
                        }
                    }
                });
            });
        // Ctrl let go (or a click): to the picked project.
        let to = chosen.or((!held).then_some(picked));
        if let Some(place) = to {
            self.tab_switcher = None;
            if let Some(id) = ids.get(place)
                && self.projects.active() != Some(*id)
            {
                self.run(&Action::new("project.switch", json!({ "project": id.0 })));
            }
        }
    }

    /// The tab row: one tab per open project, the active one lit; a tab's × closes it.
    pub(super) fn tab_row(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let active = self.projects.active();
        let mut x = bar.left();
        for id in self.projects.ids() {
            let name = self.projects.name(id).unwrap_or("Untitled").to_string();
            // A dot after the name while the project has unsaved changes.
            let unsaved = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
            let shown = if unsaved { format!("{name} •") } else { name.clone() };
            let galley = widgets::fitted(&painter, &shown, theme::ui_font(theme::TEXT), theme::text_primary(), 180.0);
            let width = galley.size().x + PAD * 2.0 + 16.0;
            let tab = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(width, bar.height()));
            x = tab.right() + 1.0;
            let close = Rect::from_center_size(Pos2::new(tab.right() - PAD - 4.0, tab.center().y), Vec2::splat(12.0));
            let response = widgets::button_area(ui, tab, Id::new(("tab", id.0)), format!("Tab {name}"));
            let close_response = widgets::button_area(ui, close, Id::new(("close-tab", id.0)), format!("Close {name}"));
            if close_response.clicked() || response.middle_clicked() {
                actions.push(Action::new("project.close", json!({ "project": id.0 })));
            } else if response.clicked() && Some(id) != active {
                actions.push(Action::new("project.switch", json!({ "project": id.0 })));
            }
            let lit = motion::toward(ui.ctx(), response.id.with("active"), Some(id) == active, motion::VALUE);
            let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
            painter.rect_filled(tab, 0.0, motion::mix(idle, theme::panel_background(), lit));
            let text = motion::mix(theme::text_secondary(), theme::text_primary(), lit);
            painter.galley(Pos2::new(tab.left() + PAD, tab.center().y - galley.size().y / 2.0), galley, text);
            let cross = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &close_response));
            let c = close.center();
            for (a, b) in [(Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)), (Vec2::new(-3.0, 3.0), Vec2::new(3.0, -3.0))] {
                painter.line_segment([c + a, c + b], theme::hairline(cross));
            }
            painter.vline(tab.right() + 0.5, bar.y_range(), theme::hairline(theme::border()));
        }
    }

    /// The question asked when a project is dropped while another is open.
    pub(super) fn open_prompt_bar(&mut self, ui: &mut Ui) {
        let Some(path) = self.open_prompt.clone() else { return };
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, theme::panel_background());
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let current = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        let mut choice = None;
        let mut cancel = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(format!("Open {name}:")).color(theme::text_primary()));
                if ui.button("In a new tab").clicked() {
                    choice = Some(OpenAs::NewTab);
                }
                if ui.button(format!("In place of {current}")).clicked() {
                    choice = Some(OpenAs::Replace);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.checkbox(&mut self.remember_choice, "Remember");
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if let Some(how) = choice {
            if self.remember_choice {
                self.settings.on_open = match how {
                    OpenAs::NewTab => OnOpen::NewTab,
                    OpenAs::Replace => OnOpen::Replace,
                };
                self.settings_changed = true;
            }
            self.open_as(path, how);
            self.open_prompt = None;
            self.remember_choice = false;
        } else if cancel {
            self.open_prompt = None;
            self.remember_choice = false;
        }
    }
}
