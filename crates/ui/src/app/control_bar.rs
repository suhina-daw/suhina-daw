// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control bar at the top of the window and the status bar at the bottom.

use super::*;

/// Seconds between the tempo sent while the tempo field is dragged.
const TEMPO_SEND: f64 = 0.1;

/// `bpm` on the steps of the place a drag holds, counted from where it started (`grab`: the step
/// and the start), to a thousandth of a BPM, within the tempo's range (#829).
pub(crate) fn snap_tempo(grab: Option<(f64, f64)>, bpm: f64) -> f64 {
    let snapped = match grab {
        Some((step, from)) if step > 0.0 => from + ((bpm - from) / step).round() * step,
        _ => bpm,
    };
    ((snapped * 1000.0).round() / 1000.0).clamp(20.0, 999.0)
}

/// The step of the place in the tempo `text` (right-aligned at `right`, characters `width`
/// apart) under `x`: 1 BPM for the integer part, 0.1, 0.01 or 0.001 for the decimals (#829).
pub(crate) fn tempo_place(text: &str, right: f32, width: f32, x: f32) -> f64 {
    let len = text.chars().count();
    let Some(dot) = text.find('.') else { return 1.0 };
    if width <= 0.0 {
        return 1.0;
    }
    let from_right = ((right - x) / width).floor().max(0.0) as usize;
    let index = len.saturating_sub(from_right + 1);
    if index <= dot {
        return 1.0;
    }
    let decimals = (index - dot).min(3);
    10f64.powi(-(decimals as i32))
}

/// The characters of the tempo `text` the place of `step` covers, as a range of indices: the
/// whole integer part for 1 BPM, one decimal otherwise.
pub(crate) fn tempo_place_span(text: &str, step: f64) -> (usize, usize) {
    let dot = text.find('.').unwrap_or(text.len());
    if step >= 1.0 {
        return (0, dot);
    }
    let decimals = (-step.log10()).round().max(1.0) as usize;
    (dot + decimals, dot + decimals + 1)
}

impl App {
    pub(super) fn control_bar(
        &mut self,
        ui: &mut Ui,
        grid: Option<&TrackGrid>,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let mut styled_ui = crate::kit::style_class::ui(ui);
        let ui = &mut *styled_ui;
        if self.themed_toolbar(ui, grid, cpu, ram, actions) {
            return;
        }
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
            self.toolbar_edit_overlay(ui, bar);
            return;
        };
        // The transport has its own comfortable hit height, independent of dense list rows.
        let row = |x: f32, width: f32| {
            let height = theme::CONTROL_BAR - 2.0 * PAD;
            Rect::from_min_size(Pos2::new(x, bar.center().y - height / 2.0), Vec2::new(width, height))
        };

        // Left: the browser's show/hide button, tempo, time signature, launch quantisation.
        let mut native_styles = themed_toolbar::NativeStyles::default();
        let mut x = bar.left() + PAD;
        let browser_rect = row(x, theme::ROW);
        x = browser_rect.right() + 4.0;
        let browser_rect = native_styles.place(ui, "browser", browser_rect);
        self.browser_toggle(ui, &painter, browser_rect);
        let tempo_rect = row(x, 56.0);
        x = tempo_rect.right() + PAD;
        let tempo_rect = native_styles.place(ui, "tempo", tempo_rect);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sampler-tempo"), grid.tempo_bpm));
        self.tempo_field(ui, &painter, tempo_rect, grid.tempo_bpm, true, actions);
        // The time signature playing, like the tempo: a launched scene may have changed it.
        let ts = grid.time_signature();
        let signature =
            if self.session().is_some() { format!("{} / {}", ts.numerator, ts.denominator) } else { String::new() };
        let ts_rect = row(x, 40.0);
        x = ts_rect.right() + PAD;
        let ts_rect = native_styles.place(ui, "signature", ts_rect);
        widgets::text_in(
            &painter,
            ts_rect,
            Align2::CENTER_CENTER,
            &signature,
            theme::mono_font(theme::TEXT),
            theme::text_secondary(),
        );
        // The computer keyboard: lit while its keys play notes, with the octave of A (#944).
        let keys_rect = row(x, 44.0);
        let label = if self.typing.on {
            format!("Computer keyboard on, {}", self.typing.label())
        } else {
            "Computer keyboard".into()
        };
        let response = widgets::button_area(ui, keys_rect, Id::new("computer-keyboard"), label);
        if response.clicked() {
            self.toggle_typing_keys();
        }
        let (fill, ink) = widgets::toggle_colors(ui, &response, self.typing.on);
        painter.rect_filled(keys_rect, 0.0, fill);
        let text = if self.typing.on { self.typing.label() } else { "Keys".into() };
        widgets::text_in(&painter, keys_rect, Align2::CENTER_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink);
        x = keys_rect.right() + PAD;
        // At narrow widths the captions go, then the count-in and the metronome (the main strip
        // has one too), so nothing runs under the transport in the middle.
        let transport_left = bar.center().x - (28.0 * 3.0 + 2.0 + PAD + 64.0) / 2.0 - PAD;
        let captions = x + 26.0 + 64.0 + PAD + 50.0 + 64.0 + PAD + 28.0 <= transport_left;
        let grid_caption = row(x, if captions { 26.0 } else { 0.0 });
        if captions {
            let grid_caption = native_styles.place(ui, "grid", grid_caption);
            widgets::text_in(
                &painter,
                grid_caption,
                Align2::LEFT_CENTER,
                "Quantize",
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
        }
        let quant_rect = row(grid_caption.right(), 64.0);
        let quant_base = quant_rect;
        let quant_rect = native_styles.place(ui, "grid", quant_rect);
        let mono = theme::mono_font(theme::TEXT);
        // When launched clips start; the Arrangement has a grid of its own (its toolbar, Ctrl+1 to 4).
        let current = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
        let mut chosen = current;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            let response = crate::kit::widgets::combo_box(
                ui,
                egui::ComboBox::from_id_salt("quantization")
                    .width(quant_rect.width())
                    // Tall enough for every value at once, without a scroll bar.
                    .height(QUANTIZATION.len() as f32 * 2.0 * theme::ROW)
                    .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::text_value())),
                |ui| {
                    for q in QUANTIZATION {
                        ui.selectable_value(&mut chosen, q, egui::RichText::new(q).font(mono.clone()));
                    }
                },
            )
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
        let count_caption = row(quant_base.right() + PAD, if captions { 50.0 } else { 0.0 });
        if captions {
            let count_caption = native_styles.place(ui, "count-in", count_caption);
            widgets::text_in(
                &painter,
                count_caption,
                Align2::LEFT_CENTER,
                "Count-in",
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
        }
        let count_in = row(count_caption.right(), 64.0);
        let metronome = row(count_in.right() + PAD, 28.0);
        if metronome.right() <= transport_left {
            let count_in = native_styles.place(ui, "count-in", count_in);
            let bars = self.session().map_or(0, |s| s.count_in_bars());
            crate::session::record_controls::count_in_menu(ui, count_in, bars, actions);
            let metronome = native_styles.place(ui, "metronome", metronome);
            crate::session::record_controls::metronome_button(ui, &painter, metronome, grid.metronome, actions);
        }

        // Centre: play, stop, record, position.
        let transport_width = 28.0 * 3.0 + 2.0 + PAD + 64.0;
        let mut x = bar.center().x - transport_width / 2.0;
        let transport = native_styles.place(ui, "transport", row(x, 86.0));
        let transport_offset = transport.left() - x;
        let position_base = row(x + 86.0 + PAD, 64.0);
        x += transport_offset;
        let play_base = row(x, 28.0);
        let play = crate::kit::control_layout::place("transport-play", play_base, bar);
        if !crate::kit::control_layout::placement("transport-play").hidden {
            let response = widgets::button_area(ui, play, Id::new("play"), "Play");
            let play_style = crate::kit::style_class::enter_named_response("button", "play", &response);
            crate::kit::native_css::mark(crate::kit::native_css::HostState::Playing, grid.running);
            if response.clicked() {
                actions.push(self.play_action(false));
            }
            let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
            let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            widgets::button_surface(
                &painter,
                play,
                &response,
                grid.running,
                motion::mix(hover, theme::control_active(), running),
                0.0,
            );
            // A seek from the ruler waiting for its boundary flashes the triangle (#657).
            let pending = self.session().and_then(|s| s.player()).and_then(|p| p.monitor().pending_jump()).is_some();
            let flash = pending && (ui.input(|i| i.time) * 4.0).floor() as i64 % 2 == 0;
            if pending {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
            }
            let glyph = if flash {
                theme::text_primary()
            } else {
                crate::kit::style_class::foreground(motion::mix(theme::text_primary(), theme::accent(), running))
            };
            let c = play.center();
            if !crate::kit::skin::paint(
                &painter,
                play,
                "play",
                crate::kit::skin::State::response(&response, grid.running),
                glyph,
                0.0,
            ) {
                painter.add(egui::Shape::convex_polygon(
                    vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)],
                    glyph,
                    egui::Stroke::NONE,
                ));
            }
            drop(play_style);
        }
        x = play_base.right() + 1.0;
        let stop_base = row(x, 28.0);
        let stop = crate::kit::control_layout::place("transport-stop", stop_base, bar);
        if !crate::kit::control_layout::placement("transport-stop").hidden {
            let response = widgets::button_area(ui, stop, Id::new("stop"), "Stop");
            let stop_style = crate::kit::style_class::enter_named_response("button", "stop", &response);
            if response.clicked() {
                actions.push(self.stop_action());
            }
            widgets::button_surface(
                &painter,
                stop,
                &response,
                false,
                motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
                0.0,
            );
            if !crate::kit::skin::paint(
                &painter,
                stop,
                "stop",
                crate::kit::skin::State::response(&response, false),
                theme::text_primary(),
                0.0,
            ) {
                painter.rect_filled(
                    Rect::from_center_size(stop.center(), Vec2::splat(8.0)),
                    0.0,
                    theme::text_primary(),
                );
            }
            drop(stop_style);
        }
        let record = row(stop_base.right() + 1.0, 28.0);
        let recording = self.session().is_some_and(|s| s.records_arrangement());
        crate::session::record_controls::record_button(ui, &painter, record, recording, actions);
        let position = native_styles.place(ui, "position", position_base);
        crate::kit::native_css::surface(&painter, position, 0.0, theme::window_background());
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            theme::mono_font(theme::TEXT),
            theme::text_primary(),
        );

        // Right, from the edge in: the view switch, the current view underlined, so it is
        // the last control at the right end. Left of it the CPU load, from the engine's monitor
        // (per mille, smoothed): how much of the time the audio engine spends rendering. In a
        // narrow window a block that would reach the position readout is left out, the RAM
        // first, so nothing overlaps and the order stays.
        let centre_end = position_base.right() + PAD;
        let fits = |right: f32, width: f32| right - width >= centre_end;
        let mut right = bar.right() - PAD;
        let mut underline_at = None;
        // A clip dragged in the Arrangement view, or carried, switches the view when it hovers
        // over the other view's name for a moment.
        let carrying = self.carry.is_some() || self.arrangement.dragged_clip().is_some();
        let pointer = ui.ctx().pointer_latest_pos();
        let mut hovered_switch = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Launcher")] {
            native_styles.enter("views");
            let color = if self.shown == view { theme::text_primary() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            right = rect.left() - 12.0;
            let rect = native_styles.place(ui, "views", rect);
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
            widgets::paint_text(&painter, pos, galley, color);
            if self.shown == view {
                underline_at = Some((pos.x, pos.y + size.y + 1.0, size.x, theme::text_primary()));
            }
        }
        right += 12.0 - PAD;
        if let Some(milli) = cpu.filter(|_| fits(right, 56.0)) {
            let rect = row(right - 56.0, 56.0);
            right = rect.left() - PAD;
            let rect = native_styles.place(ui, "performance", rect);
            let percent = f64::from(milli.min(1000)) / 10.0;
            crate::kit::native_css::surface(&painter, rect, 0.0, theme::window_background());
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
            painter.fill_background(track, 0.0, theme::control_background());
            let fill = track.width() * milli.min(1000) as f32 / 1000.0;
            let full = Rect::from_min_max(track.min, Pos2::new(track.left() + fill, track.bottom()));
            let color = if milli >= 800 { theme::meter_hot() } else { theme::meter_normal() };
            painter.rect_filled(full, 0.0, color);
            let response = ui.interact(rect, Id::new("cpu-load"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("CPU load {percent:.1}%"))
            });
        }
        // Left of that, the program's own RAM (its resident memory), in megabytes or gigabytes.
        if let Some(bytes) = ram.filter(|_| fits(right, 76.0)) {
            let rect = row(right - 76.0, 76.0);
            right = rect.left() - PAD;
            let rect = native_styles.place(ui, "performance", rect);
            crate::kit::native_css::surface(&painter, rect, 0.0, theme::window_background());
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
        }
        // Left of that, the mapping mode's button, lit while the mode is on.
        if !self.projects.is_empty() {
            native_styles.enter("mapping");
            let on = self.view.launch_keys.mode.on;
            let color = if on { theme::accent() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, "Map keys", theme::ui_font(theme::TEXT), color, 80.0);
            let size = galley.size();
            if fits(right, size.x) {
                let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
                right = rect.left() - PAD;
                let rect = native_styles.place(ui, "mapping", rect);
                let response = widgets::button_area(ui, rect, Id::new("map-keys"), "Map keys to slots and scenes");
                if response.clicked() {
                    self.toggle_mapping_mode();
                }
                let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
                widgets::paint_text(&painter, Pos2::new(rect.left(), rect.center().y - size.y / 2.0), galley, color);
            }
        }
        // Left of that, each agent the person let in (ADR 0028), lit; a click disconnects it.
        for (connection, client) in self.agents.clone() {
            native_styles.enter("agents");
            let text = format!("Disconnect {client}");
            let galley = widgets::fitted(&painter, &text, theme::ui_font(theme::TEXT), theme::accent(), 200.0);
            let size = galley.size();
            if !fits(right, size.x) {
                break;
            }
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            right = rect.left() - PAD;
            let rect = native_styles.place(ui, "agents", rect);
            let response = widgets::button_area(ui, rect, Id::new(("agent", connection)), text.clone());
            if response.clicked() {
                self.agent_disconnects.push(connection);
                self.status = Status::Done(format!("Disconnected {client}"));
            }
            let color = motion::mix(theme::accent(), theme::text_primary(), widgets::hover(ui, &response));
            widgets::paint_text(&painter, Pos2::new(rect.left(), rect.center().y - size.y / 2.0), galley, color);
        }
        drop(native_styles);
        self.hover_switch(ui.ctx(), hovered_switch);
        // The underline slides to the view that was picked.
        if let Some((x, y, width, color)) = underline_at {
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::new("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::new("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)), 0.0, color);
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::border()));
        self.toolbar_edit_overlay(ui, bar);
    }

    /// The toolbar's button that shows or hides the browser (`#control-browser-toggle`), the same
    /// in the native toolbar and in one a theme lays out.
    pub(crate) fn browser_toggle(&mut self, ui: &Ui, painter: &egui::Painter, browser_rect: Rect) {
        let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
        let response = widgets::button_area(ui, browser_rect, Id::new("browser-toggle"), label);
        let response = match self.keymap.binding_text("view.browser") {
            Some(keys) => response.on_hover_text(format!("{label} ({keys})")),
            None => response.on_hover_text(label),
        };
        let browser_style = crate::kit::style_class::enter_named_response("button", "browser-toggle", &response);
        widgets::button_surface(
            painter,
            browser_rect,
            &response,
            self.settings.browser_open,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
            0.0,
        );
        // A panel with its left part filled while the browser shows.
        let icon = Rect::from_center_size(browser_rect.center(), Vec2::new(14.0, 11.0));
        let ink = crate::kit::style_class::foreground(if self.settings.browser_open {
            theme::text_primary()
        } else {
            theme::text_secondary()
        });
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
        drop(browser_style);
    }

    pub(super) fn status_bar(&self, ui: &mut Ui, grid: Option<&TrackGrid>) {
        let _style_class = crate::kit::style_class::enter("status-bar");
        let mut styled_ui = crate::kit::style_class::ui(ui);
        let ui = &mut *styled_ui;
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
pub(super) const QUANTIZATION_LABEL: &str = "Launch quantisation";

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

impl App {
    /// The tempo field, in the control bar or a theme's toolbar: its value with three decimals, a
    /// drag steps the place under the pointer where it was pressed (the integer part 1 BPM, each
    /// decimal a tenth of the one before, #829), and while it moves the project follows a few times
    /// a second in one undo step (#916). `native` draws the field's surface, as the plain bar does.
    pub(super) fn tempo_field(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        tempo_rect: Rect,
        bpm_now: f64,
        native: bool,
        actions: &mut Vec<Action>,
    ) {
        let grab = self.tempo_grab;
        let tempo_shown = self.tempo_drag.map_or(bpm_now, |bpm| snap_tempo(grab, bpm));
        let tempo_text = format!("{tempo_shown:.3}");
        let response = widgets::value_area(ui, tempo_rect, Id::new("tempo"), "Tempo", tempo_text.clone());
        if native {
            crate::kit::native_css::surface(
                painter,
                tempo_rect,
                0.0,
                motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
            );
        }
        let mono = theme::mono_font(theme::TEXT);
        let text_area = tempo_rect.shrink2(Vec2::new(PAD, 0.0));
        let ink = if native { theme::text_primary() } else { theme::text_value() };
        widgets::text_in(painter, text_area, Align2::RIGHT_CENTER, &tempo_text, mono.clone(), ink);
        // The place under the pointer, or the one the drag holds: the integer part steps 1 BPM,
        // each decimal a tenth of the one before (#829).
        let char_width = widgets::text_width(ui, "0", mono);
        let place_at = |x: f32| tempo_place(&tempo_text, text_area.right(), char_width, x);
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            self.tempo_grab = Some((place_at(origin.x), bpm_now));
        }
        let held = self.tempo_grab.map(|(step, _)| step);
        let shown_place = held.or_else(|| response.hover_pos().map(|p| place_at(p.x)));
        if let Some(step) = shown_place {
            let (from, to) = tempo_place_span(&tempo_text, step);
            let right = text_area.right();
            let len = tempo_text.chars().count() as f32;
            let underline =
                egui::Rangef::new(right - (len - from as f32) * char_width, right - (len - to as f32) * char_width);
            let line = if held.is_some() { theme::accent() } else { theme::text_secondary() };
            painter.hline(underline, tempo_rect.bottom() - 3.0, egui::Stroke::new(1.0, line));
        }
        // Four points of drag a step of the place grabbed; the arrow keys and the wheel 1 BPM.
        let per_point = held.unwrap_or(1.0) / 4.0;
        match widgets::drag_value(ui, &response, &mut self.tempo_drag, bpm_now, per_point, 1.0, (20.0, 999.0), 120.0) {
            Drag::Done(bpm) => {
                // The drag's last value joins the step its moves made.
                let bpm = snap_tempo(self.tempo_grab, bpm);
                self.tempo_grab = None;
                let more = self.tempo_sent.take().is_some();
                actions.push(Action::new("transport.setTempo", json!({ "bpm": bpm, "continue": more })));
            }
            // While it moves the project follows, a few times a second, in one undo step, so
            // clips that follow the tempo change length as it moves (#916).
            Drag::Moving(bpm) => {
                let bpm = snap_tempo(self.tempo_grab, bpm);
                let now = ui.input(|i| i.time);
                if self.tempo_sent.is_none_or(|sent| now - sent >= TEMPO_SEND) {
                    let more = self.tempo_sent.is_some();
                    self.tempo_sent = Some(now);
                    actions.push(Action::new("transport.setTempo", json!({ "bpm": bpm, "continue": more })));
                }
            }
            Drag::None => {}
        }
    }
}

#[cfg(test)]
mod tempo_tests {
    use super::{snap_tempo, tempo_place, tempo_place_span};

    #[test]
    fn the_place_under_the_pointer_picks_the_step_of_a_drag() {
        // "124.567" right-aligned at x = 70, characters 10 wide: "7" spans 60..70.
        let text = "124.567";
        assert_eq!(tempo_place(text, 70.0, 10.0, 65.0), 0.001);
        assert_eq!(tempo_place(text, 70.0, 10.0, 55.0), 0.01);
        assert_eq!(tempo_place(text, 70.0, 10.0, 45.0), 0.1);
        assert_eq!(tempo_place(text, 70.0, 10.0, 35.0), 1.0, "the point counts with the integer part");
        assert_eq!(tempo_place(text, 70.0, 10.0, 5.0), 1.0);
        assert_eq!(tempo_place_span(text, 1.0), (0, 3));
        assert_eq!(tempo_place_span(text, 0.01), (5, 6));
    }

    #[test]
    fn a_drag_moves_in_steps_of_its_place_and_keeps_the_rest() {
        // The integer part: 124.567 stays .567 and carries across 125.
        assert_eq!(snap_tempo(Some((1.0, 124.567)), 125.4), 125.567);
        assert_eq!(snap_tempo(Some((1.0, 124.567)), 123.2), 123.567);
        // A hundredth: 124.567 → 124.597 three steps up, across the tenth.
        assert_eq!(snap_tempo(Some((0.01, 124.567)), 124.598), 124.597);
        assert_eq!(snap_tempo(Some((0.01, 124.567)), 124.611), 124.607);
        // Within the tempo's range, to a thousandth.
        assert_eq!(snap_tempo(Some((1.0, 998.0)), 1005.0), 999.0);
        assert_eq!(snap_tempo(None, 120.12345), 120.123);
    }
}
