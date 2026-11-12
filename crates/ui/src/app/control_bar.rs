// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control bar at the top of the window and the status bar at the bottom.

use super::*;
use suhina_engine::model::TimeSignature;

/// Seconds between the tempo sent while the tempo field is dragged.
const TEMPO_SEND: f64 = 0.1;

pub(crate) use crate::kit::widgets::{snap_tempo, tempo_place, tempo_place_span};

impl App {
    /// The toolbar in `ui`'s rect, its whole box (a theme's toolbar layout keeps its border and
    /// padding itself); Suhina's own layout goes in `inside`, its content box.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn control_bar(
        &mut self,
        ui: &mut Ui,
        bar: &crate::kit::element::Entered,
        inside: Rect,
        grid: Option<&TrackGrid>,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let mut styled_ui = bar.ui(ui);
        let ui = &mut *styled_ui;
        if self.themed_toolbar(ui, inside, grid, cpu, ram, actions) {
            return;
        }
        let bar = inside;
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
        // Each field is measured in its module's style, which a theme may give a larger font.
        native_styles.enter("tempo");
        let tempo_rect = row(x, tempo_width(ui));
        x = tempo_rect.right() + PAD;
        let tempo_rect = native_styles.place(ui, "tempo", tempo_rect);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sampler-tempo"), grid.tempo_bpm));
        self.tempo_field(ui, &painter, tempo_rect, grid.tempo_bpm, Some(&native_styles), actions);
        native_styles.enter("signature");
        let ts_rect = row(x, signature_width(ui));
        x = ts_rect.right() + PAD;
        let ts_rect = native_styles.place(ui, "signature", ts_rect);
        if self.session().is_some() {
            signature_field(ui, &painter, ts_rect, grid, Some(&native_styles), actions);
        }
        // The computer keyboard: lit while its keys play notes, with the octave of A (#944).
        let keys_rect = row(x, 44.0);
        let label = if self.typing.on {
            format!("Computer keyboard on, {}", self.typing.label())
        } else {
            "Computer keyboard".into()
        };
        let response = widgets::button_area(ui, keys_rect, Id::unique("computer-keyboard"), label);
        if response.clicked() {
            self.toggle_typing_keys();
        }
        let (fill, ink) = widgets::toggle_colors(ui, &response, self.typing.on);
        painter.rect_filled(keys_rect, 0.0, fill);
        let text = if self.typing.on { self.typing.label() } else { "Keys".into() };
        widgets::text_in(&painter, keys_rect, Align2::CENTER_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink);
        x = keys_rect.right() + PAD;
        // At narrow widths the captions go, then the count-in and the metronome (the main strip
        // has one too), then the launch quantisation, so nothing runs under the transport in the
        // middle.
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
        let quantization_fits = quant_base.right() <= transport_left;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            if !quantization_fits {
                return;
            }
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
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, QUANTIZATION_LABEL));
            response.on_hover_text("Launch quantisation: when launched clips start");
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
            let response = widgets::button_area(ui, play, Id::unique("play"), "Play");
            let play_style = crate::kit::element::BUTTON
                .id("play")
                .response(&response)
                .attr(crate::kit::element::Playing, grid.running)
                .attr(crate::kit::element::Checked, grid.running)
                .enter();
            if response.clicked() {
                actions.push(self.play_action());
            }
            let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
            let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            widgets::button_surface(
                &painter,
                play,
                &play_style,
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
                play_style.color_or(motion::mix(theme::text_primary(), theme::accent(), running))
            };
            let c = play.center();
            if !crate::kit::skin::paint_as(
                &play_style,
                &painter,
                play,
                "play",
                crate::kit::skin::State::response(&response, grid.running),
                glyph,
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
            let response = widgets::button_area(ui, stop, Id::unique("stop"), "Stop");
            let stop_style = crate::kit::element::BUTTON.id("stop").response(&response).enter();
            if response.clicked() {
                actions.push(self.stop_action());
            }
            widgets::button_surface(
                &painter,
                stop,
                &stop_style,
                &response,
                false,
                motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
                0.0,
            );
            if !crate::kit::skin::paint_as(
                &stop_style,
                &painter,
                stop,
                "stop",
                crate::kit::skin::State::response(&response, false),
                theme::text_primary(),
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
        native_styles.paint(&painter, position, crate::kit::style_class::surface_or(theme::window_background()));
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
        let carrying = self.carrying_clip(ui.ctx());
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
            let response = widgets::button_area(ui, rect, Id::unique(("view", name)), format!("{name} view"));
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
            native_styles.paint(&painter, rect, crate::kit::style_class::surface_or(theme::window_background()));
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
            let response = ui.interact(rect, Id::unique("cpu-load"), Sense::hover());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("CPU load {percent:.1}%")));
        }
        // Left of that, the program's own RAM (its resident memory), in megabytes or gigabytes.
        if let Some(bytes) = ram.filter(|_| fits(right, 76.0)) {
            let rect = row(right - 76.0, 76.0);
            right = rect.left() - PAD;
            let rect = native_styles.place(ui, "performance", rect);
            native_styles.paint(&painter, rect, crate::kit::style_class::surface_or(theme::window_background()));
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
            let response = ui.interact(rect, Id::unique("ram-usage"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Label, true, format!("RAM usage {}", ram_text(bytes)))
            });
        }
        // Left of that, the mapping mode's button, lit while the mode is on.
        if !self.projects.is_empty() {
            native_styles.enter("mapping");
            let on = self.view.launch_keys.mode.on;
            let color = if on { theme::accent() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, "Map", theme::ui_font(theme::TEXT), color, 80.0);
            let size = galley.size();
            if fits(right, size.x) {
                let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
                right = rect.left() - PAD;
                let rect = native_styles.place(ui, "mapping", rect);
                let response = widgets::button_area(ui, rect, Id::unique("map-keys"), "Map keys to slots and scenes");
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
            let response = widgets::button_area(ui, rect, Id::unique(("agent", connection)), text.clone());
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
            let x = motion::value(ctx, Id::unique("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::unique("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)), 0.0, color);
        }
        self.toolbar_edit_overlay(ui, bar);
    }

    /// The toolbar's button that shows or hides the browser (`#control-browser-toggle`), the same
    /// in the native toolbar and in one a theme lays out.
    pub(crate) fn browser_toggle(&mut self, ui: &Ui, painter: &egui::Painter, browser_rect: Rect) {
        let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
        let response = widgets::button_area(ui, browser_rect, Id::unique("browser-toggle"), label);
        let response = match self.keymap.binding_text("view.browser") {
            Some(keys) => response.on_hover_text(format!("{label} ({keys})")),
            None => response.on_hover_text(label),
        };
        let browser_style = crate::kit::element::BUTTON
            .id("browser-toggle")
            .response(&response)
            .attr(crate::kit::element::Checked, self.settings.browser_open)
            .enter();
        widgets::button_surface(
            painter,
            browser_rect,
            &browser_style,
            &response,
            self.settings.browser_open,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
            0.0,
        );
        // A panel with its left part filled while the browser shows.
        let icon = Rect::from_center_size(browser_rect.center(), Vec2::new(14.0, 11.0));
        let ink = browser_style.color_or(if self.settings.browser_open {
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

    /// The status bar's contents in `ui`'s rect, its content box: `style_class` is the bar's
    /// element, which the window entered and painted.
    pub(super) fn status_bar(&self, ui: &mut Ui, style_class: &crate::kit::element::Entered, grid: Option<&TrackGrid>) {
        let mut styled_ui = style_class.ui(ui);
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
    /// a second in one undo step (#916). `native`, the plain bar's modules, draws the field's surface as
    /// the tempo module's.
    pub(super) fn tempo_field(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        tempo_rect: Rect,
        bpm_now: f64,
        native: Option<&super::themed_toolbar::NativeStyles>,
        actions: &mut Vec<Action>,
    ) {
        let grab = self.tempo_grab;
        let tempo_shown = self.tempo_drag.map_or(bpm_now, |bpm| snap_tempo(grab, bpm));
        let tempo_text = format!("{tempo_shown:.3}");
        let response = widgets::value_area(ui, tempo_rect, Id::unique("tempo"), "Tempo", tempo_text.clone());
        if let Some(styles) = native {
            let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            styles.paint(painter, tempo_rect, fill);
        }
        let mono = theme::mono_font(theme::TEXT);
        let text_area = tempo_rect.shrink2(Vec2::new(PAD, 0.0));
        let ink = if native.is_some() { theme::text_primary() } else { theme::text_value() };
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
            Drag::Done(bpm) | Drag::Set(bpm) => {
                // The drag's last value joins the step its moves made.
                let bpm = snap_tempo(self.tempo_grab, bpm);
                self.tempo_grab = None;
                let dragged = self.tempo_sent.take().is_some();
                let mut params = json!({ "bpm": bpm });
                if dragged {
                    params["gesture"] = crate::interaction::commands::gesture("tempo");
                }
                actions.push(Action::new("transport.setTempo", params));
            }
            // While it moves the project follows, a few times a second, in one undo step, so
            // clips that follow the tempo change length as it moves (#916).
            Drag::Moving(bpm) => {
                let bpm = snap_tempo(self.tempo_grab, bpm);
                let now = ui.input(|i| i.time);
                if self.tempo_sent.is_none_or(|sent| now - sent >= TEMPO_SEND) {
                    self.tempo_sent = Some(now);
                    let gesture = crate::interaction::commands::gesture("tempo");
                    actions.push(Action::new("transport.setTempo", json!({ "bpm": bpm, "gesture": gesture })));
                }
            }
            Drag::None => {}
        }
    }
}

/// The values a time signature's denominator takes, in the order a drag or an arrow steps
/// through them (`transport.setTimeSignature` refuses any other).
pub(crate) const DENOMINATORS: [u8; 5] = [1, 2, 4, 8, 16];

/// The width of the chevron at the right of the time signature that opens the common ones.
const SIGNATURE_CHEVRON: f32 = 14.0;

/// The width the tempo field needs for its widest value in the font it is drawn with (a theme
/// may make it larger), with its padding.
pub(super) fn tempo_width(ui: &Ui) -> f32 {
    widgets::text_width(ui, "999.999", theme::mono_font(theme::TEXT)) + 2.0 * PAD + 4.0
}

/// The width the time-signature field needs for its widest value, its padding and its chevron.
pub(super) fn signature_width(ui: &Ui) -> f32 {
    widgets::text_width(ui, "99 / 16", theme::mono_font(theme::TEXT)) + 2.0 * PAD + SIGNATURE_CHEVRON
}

/// What is being typed into the time signature: the numerator's and the denominator's digits,
/// and which of the two the next digit goes to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SignatureTyping {
    pub(crate) in_denominator: bool,
    pub(crate) numerator: String,
    pub(crate) denominator: String,
}

/// A key typed into the time signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignatureKey {
    Digit(char),
    /// `/`, `-` or Space: on to the denominator.
    Separator,
    Backspace,
}

impl SignatureTyping {
    /// Typing that starts in the numerator, or in the denominator when that was clicked.
    pub(crate) fn from(in_denominator: bool) -> Self {
        Self { in_denominator, ..Self::default() }
    }

    /// Takes one key: a digit goes to the number typed into (two at most), a separator after
    /// the numerator's digits goes on to the denominator, and Backspace in an empty denominator
    /// goes back to the numerator.
    pub(crate) fn key(&mut self, key: SignatureKey) {
        match key {
            SignatureKey::Digit(digit) => {
                let part = if self.in_denominator { &mut self.denominator } else { &mut self.numerator };
                if digit.is_ascii_digit() && part.len() < 2 {
                    part.push(digit);
                }
            }
            SignatureKey::Separator => {
                if !self.in_denominator && !self.numerator.is_empty() {
                    self.in_denominator = true;
                }
            }
            SignatureKey::Backspace => {
                if self.in_denominator {
                    if self.denominator.pop().is_none() {
                        self.in_denominator = false;
                    }
                } else {
                    self.numerator.pop();
                }
            }
        }
    }

    /// The numerator and denominator Enter sets: what was typed, the other as `current` has it.
    /// A number the command does not take still goes, so its error is shown. `None` when nothing
    /// was typed.
    pub(crate) fn signature(&self, current: TimeSignature) -> Option<(serde_json::Value, serde_json::Value)> {
        if self.numerator.is_empty() && self.denominator.is_empty() {
            return None;
        }
        let part = |typed: &str, now: u8| typed.parse::<u64>().map_or_else(|_| json!(now), |n| json!(n));
        Some((part(&self.numerator, current.numerator), part(&self.denominator, current.denominator)))
    }
}

/// The place of `denominator` in [`DENOMINATORS`]; one not in the list counts as 4.
fn denominator_place(denominator: u8) -> usize {
    DENOMINATORS.iter().position(|d| *d == denominator).unwrap_or(2)
}

/// The denominator at `place` in [`DENOMINATORS`] (rounded, at either end at most).
pub(crate) fn denominator_at(place: f64) -> u8 {
    let last = DENOMINATORS.len() - 1;
    let at = if place.is_finite() { place.round().clamp(0.0, last as f64) as usize } else { 2 };
    DENOMINATORS.get(at).copied().unwrap_or(4)
}

/// The keys the time signature takes this frame while it holds them: arrow steps (up is +1),
/// Enter, Escape, and the digits, separators and Backspace, which go into `typing`.
fn signature_keys(ui: &Ui, typing: &mut SignatureTyping) -> (i32, bool, bool) {
    let (mut steps, mut enter, mut escape) = (0, false, false);
    ui.ctx().input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key, pressed, modifiers, .. } if !modifiers.command => match key {
                Key::ArrowUp | Key::ArrowDown | Key::Enter | Key::Escape | Key::Backspace if *pressed => {
                    match key {
                        Key::ArrowUp => steps += 1,
                        Key::ArrowDown => steps -= 1,
                        Key::Enter => enter = true,
                        Key::Escape => escape = true,
                        _ => typing.key(SignatureKey::Backspace),
                    }
                    false
                }
                // Space comes as text too, read below; its key must not start playback.
                Key::Space => false,
                _ => true,
            },
            egui::Event::Text(text) if text.chars().all(|c| c.is_ascii_digit() || matches!(c, '/' | '-' | ' ')) => {
                for c in text.chars() {
                    typing.key(if c.is_ascii_digit() { SignatureKey::Digit(c) } else { SignatureKey::Separator });
                }
                false
            }
            _ => true,
        });
    });
    (steps, enter, escape)
}

/// The time signature in the control bar: the one playing, like the tempo (a launched scene may
/// have changed it), as two numbers that work like the tempo field. Dragging the numerator up or
/// down changes it from 1 to 99, dragging the denominator steps it through 1, 2, 4, 8 and 16, and
/// the project follows the drag in one undo step, so the grids change while it moves. Clicked,
/// a number takes the keys: the arrow keys step it, digits are typed into it, `/`, `-` or Space
/// goes on to the denominator, Backspace in an empty denominator back to the numerator, and
/// Enter sets what was typed. Only the chevron opens the common signatures
/// ([`crate::interaction::menus::time_signature_list`]). Each change is a command on the
/// project's signature at the playhead, from the bar it starts at (`docs/spec/tempo-map.md` ›
/// Editing in the window, #1259, #1264).
pub(super) fn signature_field(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    grid: &TrackGrid,
    native: Option<&super::themed_toolbar::NativeStyles>,
    actions: &mut Vec<Action>,
) {
    use crate::interaction::menus::{SameSignature, signature_choice};
    let shown = grid.time_signature();
    let current = grid.project_time_signature();
    let bar = grid.signature_bar();
    // The project's signature picked while a launched scene's drives the clock puts the clock
    // back on the tempo map (`docs/spec/tempo-map.md` › Editing in the window).
    let same = if grid.meter.follows_map { SameSignature::Nothing } else { SameSignature::FollowMap };
    let chevron_rect = Rect::from_min_max(Pos2::new(rect.right() - SIGNATURE_CHEVRON, rect.top()), rect.max);
    let numbers = Rect::from_min_max(rect.min, Pos2::new(chevron_rect.left(), rect.bottom()));
    let middle = numbers.center().x;
    let numerator_rect = Rect::from_min_max(numbers.min, Pos2::new(middle, numbers.bottom()));
    let denominator_rect = Rect::from_min_max(Pos2::new(middle, numbers.top()), numbers.max);
    let hint = |part: &str| {
        if bar <= 1 {
            format!("Time signature, {part}: drag, or click and type such as 6/8")
        } else {
            format!("Time signature from bar {bar}, {part}: drag, or click and type such as 6/8")
        }
    };
    let numerator_id = Id::unique("time-signature-numerator");
    let numerator = widgets::value_area(ui, numerator_rect, numerator_id, "Time signature numerator", shown.numerator)
        .on_hover_text(hint("beats in a bar"));
    let denominator_id = Id::unique("time-signature-denominator");
    let denominator =
        widgets::value_area(ui, denominator_rect, denominator_id, "Time signature denominator", shown.denominator)
            .on_hover_text(hint("the note that counts as one beat"));
    let chevron = widgets::button_area(ui, chevron_rect, Id::unique("time-signature-menu"), "Time signatures")
        .on_hover_text("Common time signatures");
    if let Some(styles) = native {
        let hovered =
            [&numerator, &denominator, &chevron].into_iter().map(|r| widgets::hover(ui, r)).fold(0.0, f32::max);
        styles.paint(painter, rect, motion::mix(theme::control_background(), theme::control_hover(), hovered));
    }

    // Typing: a click on a number gives the field the keys, from that number.
    let ctx = ui.ctx().clone();
    let typing_id = Id::unique("time-signature-typing");
    for (response, in_denominator) in [(&numerator, false), (&denominator, true)] {
        if response.clicked() {
            ctx.data_mut(|d| d.insert_temp(typing_id, SignatureTyping::from(in_denominator)));
            widgets::hold_value_keys(&ctx, typing_id, true);
        }
    }
    let mut typing = ctx.data(|d| d.get_temp::<SignatureTyping>(typing_id));
    // Another value control clicked, a press anywhere else or a drag lets the keys go.
    let pressed_elsewhere = ui.input(|i| i.pointer.any_pressed()) && !numerator.hovered() && !denominator.hovered();
    let drag_started = [&numerator, &denominator].into_iter().any(|r| r.drag_started());
    if typing.is_some() && (!widgets::holds_value_keys(&ctx, typing_id) || pressed_elsewhere || drag_started) {
        typing = None;
    }
    let mut changes: Vec<(serde_json::Value, serde_json::Value, bool)> = Vec::new();
    if let Some(typed) = typing.as_mut() {
        widgets::hold_value_keys(&ctx, typing_id, true);
        let (steps, enter, escape) = signature_keys(ui, typed);
        if steps != 0 {
            let to = if typed.in_denominator {
                let place = denominator_place(current.denominator) as f64 + f64::from(steps);
                (json!(current.numerator), json!(denominator_at(place)))
            } else {
                let n = (i64::from(current.numerator) + i64::from(steps)).clamp(1, 99);
                (json!(n), json!(current.denominator))
            };
            changes.push((to.0, to.1, false));
        }
        if enter && let Some((n, d)) = typed.signature(current) {
            changes.push((n, d, false));
        }
        if enter || escape {
            typing = None;
        }
    }
    match &typing {
        Some(typed) => {
            ctx.data_mut(|d| d.insert_temp(typing_id, typed.clone()));
        }
        None => {
            ctx.data_mut(|d| d.remove::<SignatureTyping>(typing_id));
            widgets::release_value_keys(&ctx, typing_id);
        }
    }

    // A drag moves the number, and the project follows it in one undo step (a gesture), so the
    // grids change while it moves; the wheel (Ctrl) steps it.
    let preview = |id: Id| ctx.data(|d| d.get_temp::<Option<f64>>(id.with("preview"))).flatten();
    let (mut numerator_preview, mut denominator_preview) = (preview(numerator_id), preview(denominator_id));
    let numerator_dragged = numerator_preview.is_some();
    // Six points of drag a beat; a wheel tick one.
    match widgets::drag_value_without_keys(
        ui,
        &numerator,
        &mut numerator_preview,
        f64::from(current.numerator),
        1.0 / 6.0,
        1.0,
        (1.0, 99.0),
        4.0,
    ) {
        Drag::Moving(n) => changes.push((json!(n.round() as u64), json!(current.denominator), true)),
        Drag::Done(n) | Drag::Set(n) => {
            changes.push((json!(n.round() as u64), json!(current.denominator), numerator_dragged));
        }
        Drag::None => {}
    }
    // The denominator by its place in the list: sixteen points of drag a step.
    let denominator_dragged = denominator_preview.is_some();
    match widgets::drag_value_without_keys(
        ui,
        &denominator,
        &mut denominator_preview,
        denominator_place(current.denominator) as f64,
        1.0 / 16.0,
        1.0,
        (0.0, (DENOMINATORS.len() - 1) as f64),
        2.0,
    ) {
        Drag::Moving(place) => changes.push((json!(current.numerator), json!(denominator_at(place)), true)),
        Drag::Done(place) | Drag::Set(place) => {
            changes.push((json!(current.numerator), json!(denominator_at(place)), denominator_dragged));
        }
        Drag::None => {}
    }
    ctx.data_mut(|d| {
        d.insert_temp(numerator_id.with("preview"), numerator_preview);
        d.insert_temp(denominator_id.with("preview"), denominator_preview);
    });
    for (n, d, dragged) in changes {
        // A drag's own value is already the project's while it moves: nothing to put back.
        let same = if dragged { SameSignature::Nothing } else { same };
        if let Some(mut action) = signature_choice(bar, current, same, n, d) {
            if dragged {
                action.params["gesture"] = crate::interaction::commands::gesture("time-signature");
            }
            actions.push(action);
        }
    }

    // The numbers: while one is typed into, what was typed (the other dimmed until typed).
    let mono = theme::mono_font(theme::TEXT);
    let ink = if native.is_some() { theme::text_primary() } else { theme::text_secondary() };
    let half_slash = widgets::text_width(ui, " / ", mono.clone()) / 2.0;
    let left = Rect::from_min_max(numbers.min, Pos2::new(middle - half_slash, numbers.bottom()));
    let right = Rect::from_min_max(Pos2::new(middle + half_slash, numbers.top()), numbers.max);
    let numerator_now = numerator_preview.map_or(shown.numerator, |n| n.round().clamp(1.0, 99.0) as u8).to_string();
    let denominator_now = denominator_preview.map_or(shown.denominator, denominator_at).to_string();
    let part = |typed: Option<&String>, active: bool, now: String| match typed {
        Some(t) if !t.is_empty() => (t.clone(), theme::text_value()),
        Some(_) if active => (String::new(), ink),
        Some(_) => (now, theme::text_hint()),
        None => (now, ink),
    };
    let (numerator_text, numerator_ink) =
        part(typing.as_ref().map(|t| &t.numerator), typing.as_ref().is_some_and(|t| !t.in_denominator), numerator_now);
    let (denominator_text, denominator_ink) = part(
        typing.as_ref().map(|t| &t.denominator),
        typing.as_ref().is_some_and(|t| t.in_denominator),
        denominator_now,
    );
    widgets::text_in(painter, left, Align2::RIGHT_CENTER, &numerator_text, mono.clone(), numerator_ink);
    widgets::text_in(painter, numbers, Align2::CENTER_CENTER, "/", mono.clone(), ink);
    widgets::text_in(painter, right, Align2::LEFT_CENTER, &denominator_text, mono.clone(), denominator_ink);
    // The number typed into is outlined.
    if let Some(typed) = &typing {
        let half = if typed.in_denominator { denominator_rect } else { numerator_rect };
        painter.rect_stroke(
            half.shrink2(Vec2::new(1.0, 2.0)),
            3.0,
            egui::Stroke::new(1.0, theme::text_secondary()),
            egui::StrokeKind::Inside,
        );
    }
    // The number under the pointer, or the one dragged, is underlined, as the tempo's place is.
    let char_width = widgets::text_width(ui, "0", mono);
    for (response, dragged, text, area, at_right) in [
        (&numerator, numerator_preview.is_some(), &numerator_text, left, true),
        (&denominator, denominator_preview.is_some(), &denominator_text, right, false),
    ] {
        if (dragged || response.hovered()) && typing.is_none() {
            let width = text.chars().count() as f32 * char_width;
            let span = if at_right {
                egui::Rangef::new(area.right() - width, area.right())
            } else {
                egui::Rangef::new(area.left(), area.left() + width)
            };
            let line = if dragged { theme::accent() } else { theme::text_secondary() };
            painter.hline(span, rect.bottom() - 3.0, egui::Stroke::new(1.0, line));
        }
    }

    // Only the chevron opens the list of common signatures.
    let menu_id = chevron.id.with("menu");
    let open = egui::Popup::is_id_open(ui.ctx(), menu_id);
    let chevron_ink = if chevron.hovered() || open { theme::text_primary() } else { theme::text_secondary() };
    widgets::chevron(painter, chevron_rect.center(), true, chevron_ink);
    egui::Popup::menu(&chevron)
        .id(menu_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| crate::interaction::menus::time_signature_list(ui, bar, current, same, actions));
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

#[cfg(test)]
mod signature_tests {
    use super::{SignatureKey, SignatureTyping, TimeSignature, denominator_at};
    use serde_json::json;

    /// `typing` after the keys of `text`: digits, `/`, `-` or a space, and `<` for Backspace.
    fn typed(mut typing: SignatureTyping, text: &str) -> SignatureTyping {
        for c in text.chars() {
            typing.key(match c {
                '<' => SignatureKey::Backspace,
                '0'..='9' => SignatureKey::Digit(c),
                _ => SignatureKey::Separator,
            });
        }
        typing
    }

    #[test]
    fn a_separator_goes_on_to_the_denominator_and_backspace_comes_back() {
        let current = TimeSignature { numerator: 6, denominator: 8 };
        let from_numerator = SignatureTyping::from(false);
        for text in ["1/16", "1-16", "1 16"] {
            let typing = typed(from_numerator.clone(), text);
            assert!(typing.in_denominator, "{text}");
            assert_eq!(typing.signature(current), Some((json!(1), json!(16))), "{text}");
        }
        // Backspace through the denominator, then back into the numerator.
        let typing = typed(from_numerator.clone(), "12/16<<<");
        assert!(!typing.in_denominator);
        assert_eq!((typing.numerator.as_str(), typing.denominator.as_str()), ("12", ""));
        let typing = typed(typing, "<3/4");
        assert_eq!(typing.signature(current), Some((json!(13), json!(4))));
        // A separator before any digit stays in the numerator; two digits a number at most.
        let typing = typed(from_numerator.clone(), "/123");
        assert_eq!((typing.in_denominator, typing.numerator.as_str()), (false, "12"));
        // One number keeps the other as it is; nothing typed sets nothing.
        assert_eq!(typed(from_numerator.clone(), "5").signature(current), Some((json!(5), json!(8))));
        assert_eq!(typed(SignatureTyping::from(true), "16").signature(current), Some((json!(6), json!(16))));
        assert_eq!(from_numerator.signature(current), None);
        // A number the command does not take still goes, for its error to be shown.
        assert_eq!(typed(from_numerator, "7/5").signature(current), Some((json!(7), json!(5))));
    }

    #[test]
    fn a_denominator_steps_through_the_powers_of_two() {
        assert_eq!([0.0, 1.0, 2.0, 3.0, 4.0].map(denominator_at), [1, 2, 4, 8, 16]);
        assert_eq!(denominator_at(2.6), 8, "a drag rounds to the nearest step");
        assert_eq!(denominator_at(9.0), 16);
        assert_eq!(denominator_at(-3.0), 1);
        assert_eq!(denominator_at(f64::NAN), 4);
    }
}
