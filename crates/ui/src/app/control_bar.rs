// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control bar at the top of the window and the status bar at the bottom.

use super::*;

impl App {
    pub(super) fn control_bar(
        &mut self,
        ui: &mut Ui,
        grid: Option<&TrackGrid>,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let Some(grid) = grid else {
            widgets::text_in(
                &painter,
                bar.shrink2(Vec2::new(PAD, 0.0)),
                Align2::LEFT_CENTER,
                "Suhina",
                theme::ui_font(theme::TEXT),
                theme::text_secondary(),
            );
            return;
        };
        let row = |x: f32, width: f32| {
            Rect::from_min_size(Pos2::new(x, bar.center().y - theme::ROW / 2.0), Vec2::new(width, theme::ROW))
        };

        // Left: the browser's show/hide button, tempo, time signature, launch quantisation.
        let mut x = bar.left() + PAD;
        let browser_rect = row(x, theme::ROW);
        let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
        let response = widgets::button_area(ui, browser_rect, Id::new("browser-toggle"), label);
        let response = match self.keymap.binding_text("view.browser") {
            Some(keys) => response.on_hover_text(format!("{label} ({keys})")),
            None => response.on_hover_text(label),
        };
        painter.rect_filled(
            browser_rect,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        // A panel with its left part filled while the browser shows.
        let icon = Rect::from_center_size(browser_rect.center(), Vec2::new(14.0, 11.0));
        let ink = if self.settings.browser_open { theme::text_primary() } else { theme::text_secondary() };
        painter.rect_stroke(icon, 0.0, theme::hairline(ink), egui::StrokeKind::Inside);
        let side = Rect::from_min_size(icon.min, Vec2::new(5.0, icon.height()));
        if self.settings.browser_open {
            painter.rect_filled(side, 0.0, ink);
        } else {
            painter.vline(side.right() + 0.5, icon.y_range(), theme::hairline(ink));
        }
        if response.clicked() {
            self.toggle_browser();
        }
        x = browser_rect.right() + 4.0;
        let tempo_rect = row(x, 56.0);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sampler-tempo"), grid.tempo_bpm));
        let tempo_shown = self.tempo_drag.unwrap_or(grid.tempo_bpm);
        let response = widgets::value_area(ui, tempo_rect, Id::new("tempo"), "Tempo", format!("{tempo_shown:.2}"));
        match widgets::drag_value(ui, &response, &mut self.tempo_drag, grid.tempo_bpm, 0.1, 1.0, (20.0, 999.0), 120.0) {
            Drag::Done(bpm) => {
                actions.push(Action::new("transport.setTempo", json!({ "bpm": (bpm * 100.0).round() / 100.0 })))
            }
            Drag::Moving(_) | Drag::None => {}
        }
        painter.rect_filled(
            tempo_rect,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        let mono = theme::mono_font(theme::TEXT);
        widgets::text_in(
            &painter,
            tempo_rect.shrink2(Vec2::new(PAD, 0.0)),
            Align2::RIGHT_CENTER,
            &format!("{tempo_shown:.2}"),
            mono.clone(),
            theme::text_primary(),
        );
        x = tempo_rect.right() + PAD;
        // The time signature playing, like the tempo: a launched scene may have changed it.
        let ts = grid.time_signature();
        let signature =
            if self.session().is_some() { format!("{} / {}", ts.numerator, ts.denominator) } else { String::new() };
        let ts_rect = row(x, 40.0);
        widgets::text_in(&painter, ts_rect, Align2::CENTER_CENTER, &signature, mono.clone(), theme::text_secondary());
        x = ts_rect.right() + PAD;
        let small = theme::ui_font(theme::TEXT_SMALL);
        // At narrow widths the captions go, then the count-in and the metronome (the main strip
        // has one too), so nothing runs under the transport in the middle.
        let transport_left = bar.center().x - (28.0 * 3.0 + 2.0 + PAD + 64.0) / 2.0 - PAD;
        let captions = x + 26.0 + 64.0 + PAD + 50.0 + 64.0 + PAD + 28.0 <= transport_left;
        let grid_caption = row(x, if captions { 26.0 } else { 0.0 });
        if captions {
            widgets::text_in(&painter, grid_caption, Align2::LEFT_CENTER, "Grid", small.clone(), theme::text_hint());
        }
        let quant_rect = row(grid_caption.right(), 64.0);
        // One value for both: when launched clips start, and the Arrangement view's grid.
        let current = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
        let mut chosen = current;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            let response = egui::ComboBox::from_id_salt("quantization")
                .width(quant_rect.width())
                // Tall enough for every value at once, without a scroll bar.
                .height(QUANTIZATION.len() as f32 * 2.0 * theme::ROW)
                .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::text_value()))
                .show_ui(ui, |ui| {
                    for q in QUANTIZATION {
                        ui.selectable_value(&mut chosen, q, egui::RichText::new(q).font(mono.clone()));
                    }
                })
                .response;
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, QUANTIZATION_LABEL));
            response.on_hover_text(
                "Grid: when launched clips start, and the Arrangement view's grid lines and snapping (Alt: off the \
                 grid)",
            );
        });
        if chosen != current {
            actions.push(Action::new("launcher.setQuantization", json!({ "value": chosen })));
        }
        let count_caption = row(quant_rect.right() + PAD, if captions { 50.0 } else { 0.0 });
        if captions {
            widgets::text_in(&painter, count_caption, Align2::LEFT_CENTER, "Count-in", small, theme::text_hint());
        }
        let count_in = row(count_caption.right(), 64.0);
        let metronome = row(count_in.right() + PAD, 28.0);
        if metronome.right() <= transport_left {
            let bars = self.session().map_or(0, |s| s.count_in_bars());
            crate::session::record_controls::count_in_menu(ui, count_in, bars, actions);
            crate::session::record_controls::metronome_button(ui, &painter, metronome, grid.metronome, actions);
        }

        // Centre: play, stop, record, position.
        let transport_width = 28.0 * 3.0 + 2.0 + PAD + 64.0;
        let mut x = bar.center().x - transport_width / 2.0;
        let play = row(x, 28.0);
        let response = widgets::button_area(ui, play, Id::new("play"), "Play");
        if response.clicked() {
            actions.push(self.play_action(false));
        }
        let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
        let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(play, 0.0, motion::mix(hover, theme::control_active(), running));
        let glyph = motion::mix(theme::text_primary(), theme::accent(), running);
        let c = play.center();
        painter.add(egui::Shape::convex_polygon(
            vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)],
            glyph,
            egui::Stroke::NONE,
        ));
        x = play.right() + 1.0;
        let stop = row(x, 28.0);
        let response = widgets::button_area(ui, stop, Id::new("stop"), "Stop");
        if response.clicked() {
            actions.push(self.stop_action());
        }
        painter.rect_filled(
            stop,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, theme::text_primary());
        let record = row(stop.right() + 1.0, 28.0);
        let recording = self.session().is_some_and(|s| s.records_arrangement());
        crate::session::record_controls::record_button(ui, &painter, record, recording, actions);
        x = record.right() + PAD;
        let position = row(x, 64.0);
        painter.rect_filled(position, 0.0, theme::window_background());
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            mono,
            theme::text_primary(),
        );

        // Right, from the edge in: the view switch, the current view underlined, so it is
        // the last control at the right end. Left of it the CPU load, from the engine's monitor
        // (per mille, smoothed): how much of the time the audio engine spends rendering. In a
        // narrow window a block that would reach the position readout is left out, the RAM
        // first, so nothing overlaps and the order stays.
        let centre_end = position.right() + PAD;
        let fits = |right: f32, width: f32| right - width >= centre_end;
        let mut right = bar.right() - PAD;
        let mut underline_at = None;
        // A clip dragged in the Arrangement view, or carried, switches the view when it hovers
        // over the other view's name for a moment.
        let carrying = self.carry.is_some() || self.arrangement.dragged_clip().is_some();
        let pointer = ui.ctx().pointer_latest_pos();
        let mut hovered_switch = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Session")] {
            let color = if self.shown == view { theme::text_primary() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("view", name)), format!("{name} view"));
            if response.clicked() {
                self.shown = view;
            }
            if carrying && self.shown != view && pointer.is_some_and(|p| rect.expand(4.0).contains(p)) {
                hovered_switch = Some(view);
                painter.rect_stroke(rect.expand(3.0), 0.0, theme::hairline(theme::accent()), egui::StrokeKind::Inside);
            }
            let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
            let pos = Pos2::new(rect.left(), rect.center().y - size.y / 2.0);
            painter.galley(pos, galley, color);
            if self.shown == view {
                underline_at = Some((pos.x, pos.y + size.y + 1.0, size.x));
            }
            right = rect.left() - 12.0;
        }
        right += 12.0 - PAD;
        if let Some(milli) = cpu.filter(|_| fits(right, 56.0)) {
            let rect = row(right - 56.0, 56.0);
            let percent = f64::from(milli.min(1000)) / 10.0;
            painter.rect_filled(rect, 0.0, theme::window_background());
            let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
            widgets::text_in(
                &painter,
                inner,
                Align2::LEFT_CENTER,
                "CPU",
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
            widgets::text_in(
                &painter,
                inner,
                Align2::RIGHT_CENTER,
                &format!("{percent:.0}%"),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            let track = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
                Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
            );
            painter.rect_filled(track, 0.0, theme::control_background());
            let fill = track.width() * milli.min(1000) as f32 / 1000.0;
            let full = Rect::from_min_max(track.min, Pos2::new(track.left() + fill, track.bottom()));
            let color = if milli >= 800 { theme::meter_hot() } else { theme::meter_normal() };
            painter.rect_filled(full, 0.0, color);
            let response = ui.interact(rect, Id::new("cpu-load"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("CPU load {percent:.1}%"))
            });
            right = rect.left() - PAD;
        }
        // Left of that, the program's own RAM (its resident memory), in megabytes or gigabytes.
        if let Some(bytes) = ram.filter(|_| fits(right, 76.0)) {
            let rect = row(right - 76.0, 76.0);
            painter.rect_filled(rect, 0.0, theme::window_background());
            let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
            widgets::text_in(
                &painter,
                inner,
                Align2::LEFT_CENTER,
                "RAM",
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
            widgets::text_in(
                &painter,
                inner,
                Align2::RIGHT_CENTER,
                &ram_text(bytes),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            let response = ui.interact(rect, Id::new("ram-usage"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("RAM usage {}", ram_text(bytes)))
            });
            right = rect.left() - PAD;
        }
        // Left of that, the mapping mode's button, lit while the mode is on.
        if !self.projects.is_empty() {
            let on = self.view.launch_keys.mode.on;
            let color = if on { theme::accent() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, "Map keys", theme::ui_font(theme::TEXT), color, 80.0);
            let size = galley.size();
            if fits(right, size.x) {
                let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
                let response = widgets::button_area(ui, rect, Id::new("map-keys"), "Map keys to slots and scenes");
                if response.clicked() {
                    self.toggle_mapping_mode();
                }
                let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
                painter.galley(Pos2::new(rect.left(), rect.center().y - size.y / 2.0), galley, color);
                right = rect.left() - PAD;
            }
        }
        // Left of that, each agent the person let in (ADR 0028), lit; a click disconnects it.
        for (connection, client) in self.agents.clone() {
            let text = format!("Disconnect {client}");
            let galley = widgets::fitted(&painter, &text, theme::ui_font(theme::TEXT), theme::accent(), 200.0);
            let size = galley.size();
            if !fits(right, size.x) {
                break;
            }
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("agent", connection)), text.clone());
            if response.clicked() {
                self.agent_disconnects.push(connection);
                self.status = Status::Done(format!("Disconnected {client}"));
            }
            let color = motion::mix(theme::accent(), theme::text_primary(), widgets::hover(ui, &response));
            painter.galley(Pos2::new(rect.left(), rect.center().y - size.y / 2.0), galley, color);
            right = rect.left() - PAD;
        }
        self.hover_switch(ui.ctx(), hovered_switch);
        // The underline slides to the view that was picked.
        if let Some((x, y, width)) = underline_at {
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::new("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::new("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)),
                0.0,
                theme::text_primary(),
            );
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::border()));
    }

    pub(super) fn status_bar(&self, ui: &mut Ui, grid: Option<&TrackGrid>) {
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = match &self.status {
            Status::Hint(t) => (t.as_str(), theme::text_secondary()),
            Status::Done(t) => (t.as_str(), theme::text_secondary()),
            Status::Error(t) => (t.as_str(), theme::recording()),
        };
        let backed_up = self
            .session()
            .and_then(Session::last_backup)
            .map(|b| format!(" · backed up {}", suhina_session::backup::clock_time(b.written)));
        let right = grid.map(|g| {
            let tracks = g.columns.iter().filter(|c| c.kind != crate::interaction::grid::ColumnKind::Return).count();
            format!("{tracks} tracks · {} scenes{}", g.scenes.len(), backed_up.as_deref().unwrap_or(""))
        });
        let right_width = match (&right, &backed_up) {
            (None, _) => 0.0,
            (Some(_), None) => 140.0,
            (Some(_), Some(_)) => 240.0,
        };
        let left =
            Rect::from_min_max(bar.min + Vec2::new(PAD, 0.0), Pos2::new(bar.right() - right_width - PAD, bar.bottom()));
        widgets::text_in(&painter, left, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
        if let Some(right) = right {
            let rect =
                Rect::from_min_max(Pos2::new(bar.right() - right_width, bar.top()), bar.max - Vec2::new(PAD, 0.0));
            widgets::text_in(
                &painter,
                rect,
                Align2::RIGHT_CENTER,
                &right,
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
        }
    }
}

/// The control bar's value box, as screen readers and tests name it.
pub(super) const QUANTIZATION_LABEL: &str = "Grid and launch quantisation";

/// The control bar's text for `quantization`: the choice that parses to it.
pub(super) fn quantization_label(quantization: suhina_engine::Quantization) -> &'static str {
    QUANTIZATION
        .into_iter()
        .find(|q| suhina_session::parse_quantization(q).ok() == Some(quantization))
        .unwrap_or(DEFAULT_QUANTIZATION)
}

/// The RAM as the control bar shows it: megabytes, or gigabytes with a decimal from 1 GiB on.
pub(super) fn ram_text(bytes: u64) -> String {
    let mib = bytes as f64 / (1024.0 * 1024.0);
    if mib >= 1024.0 { format!("{:.1} GB", mib / 1024.0) } else { format!("{mib:.0} MB") }
}
