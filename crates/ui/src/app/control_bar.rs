// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control bar at the top of the window and the status bar at the bottom.

use super::*;
use crate::kit::element::{Look, caption_ink, error_ink, hint_ink, value_ink};
use suhina_engine::model::TimeSignature;

/// Seconds between the tempo sent while the tempo field is dragged.
const TEMPO_SEND: f64 = 0.1;

pub(crate) use crate::kit::widgets::{snap_tempo, tempo_place, tempo_place_span};

impl App {
    /// The toolbar in `ui`'s rect, its whole box, which the window painted; `inside` is its content
    /// box. Its modules are laid out in one CSS row (#1372): the default arrangement, or the
    /// order a theme's `toolbar` gives, each module as wide as it draws. In a narrow window the
    /// toolbar gives way in steps ([`App::toolbar_candidates`]) until a step fits.
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
        let rect = ui.max_rect();
        let theme_skin = self.toolbar_menu(ui, rect);
        let Some(grid) = grid else {
            ui.allocate_rect(inside, Sense::hover());
            widgets::text_in(
                &ui.painter_at(inside),
                inside,
                Align2::LEFT_CENTER,
                "Suhina",
                theme::ui_font(theme::TEXT),
                caption_ink(),
            );
            self.toolbar_edit_overlay(ui, rect);
            return;
        };
        let candidates = self.toolbar_candidates(ui, &theme_skin, inside.height(), cpu.is_some(), ram.is_some());
        // A step fits when the end and the middle have room; the start's, until it has left out
        // all it can (the count-in and the metronome), after which it may run under the middle.
        let start_done = candidates.len().min(3).saturating_sub(1);
        let row = crate::kit::native_css::row::fitting_with(
            &crate::kit::native_css::row::Bar::of(bar, rect.size()),
            &candidates,
            |index, items, row| {
                items
                    .iter()
                    .enumerate()
                    .all(|(at, item)| row.inside(at) || (index >= start_done && item.parent == Some(0)))
            },
        );
        let Some((chosen, row)) = row else { return };
        let Some(items) = candidates.get(chosen) else { return };
        let painter = ui.painter_at(rect);
        let mut hovered_switch = None;
        let mut underline = None;
        // A group's modules are drawn in its scope, so a rule through the group reaches them and a
        // group that hides its overflow cuts them. A module outside any group stands alone.
        let mut tempo_drawn = false;
        let shown = Shown { grid, cpu, ram };
        for (index, item) in items.iter().enumerate().filter(|(_, item)| item.parent.is_none()) {
            let (Some(id), Some(at)) = (item.id.as_deref(), row.at(index, rect.min)) else { continue };
            match item.name {
                "group" => {
                    let group = crate::kit::element::GROUP.id(id).attribute("kind", id).enter();
                    group.paint(&painter, at);
                    let clip = rect.intersect(group.input_clip(at));
                    let members = items.iter().enumerate().filter(|(_, member)| member.parent == Some(index));
                    for (member, module) in members {
                        let Some(module_id) = module.id.as_deref().filter(|_| module.name == "toolbar-module") else {
                            continue;
                        };
                        let Some(laid) = row.at(member, rect.min) else { continue };
                        tempo_drawn |= module_id == "tempo";
                        let place = (module_id, laid, clip);
                        let state = (&mut hovered_switch, &mut underline);
                        self.toolbar_member(ui, &painter, place, shown, actions, state);
                    }
                }
                "toolbar-module" => {
                    tempo_drawn |= id == "tempo";
                    let state = (&mut hovered_switch, &mut underline);
                    self.toolbar_member(ui, &painter, (id, at, rect), shown, actions, state);
                }
                _ => {}
            }
        }
        // A tempo drag whose field went away mid-drag (a rule hid it) cannot be finished: it is
        // dropped once the pointer is up, so no stale preview or underline is left.
        if !tempo_drawn && !ui.input(|i| i.pointer.any_down()) {
            (self.tempo_drag, self.tempo_grab, self.tempo_sent) = (None, None, None);
        }
        self.hover_switch(ui.ctx(), hovered_switch);
        // The underline slides to the view that was picked.
        // Painted where its module is, inside the group's clip.
        if let Some(((x, y, width, color), clip)) = underline {
            let painter = painter.with_clip_rect(painter.clip_rect().intersect(clip));
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::unique("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::unique("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)), 0.0, color);
        }
        self.toolbar_edit_overlay(ui, rect);
    }

    /// The toolbar's layouts this frame, from the fullest to the sparsest: the order a theme
    /// gives as it is, or the default arrangement giving way in the order the toolbar always has
    /// (the captions, then the count-in and the metronome, the agents, the mapping button, the RAM
    /// and the CPU). `height` is the toolbar's content box's.
    fn toolbar_candidates(
        &self,
        ui: &Ui,
        theme_skin: &crate::kit::skin::Skin,
        height: f32,
        cpu: bool,
        ram: bool,
    ) -> Vec<Vec<crate::kit::native_css::row::Item>> {
        use crate::kit::native_css::row::Item;
        let control = (theme::CONTROL_BAR - 2.0 * PAD).min(height.max(1.0));
        let font = theme::ui_font(theme::TEXT);
        let views = widgets::text_width(ui, "Launcher", font.clone())
            + 12.0
            + widgets::text_width(ui, "Arrangement", font.clone());
        let mapping = widgets::text_width(ui, "Map", font.clone());
        let agents: Vec<f32> = self
            .agents
            .iter()
            .map(|(_, client)| widgets::text_width(ui, &format!("Disconnect {client}"), font.clone()))
            .collect();
        let agents_width = agents.iter().sum::<f32>() + PAD * agents.len().saturating_sub(1) as f32;
        let module = |id: &str, width: f32| Item::named("toolbar-module", id).width(width.max(1.0)).height(control);
        let full =
            |id: &str, width: f32| Item::named("toolbar-module", id).width(width.max(1.0)).height(height.max(1.0));
        let width = |id: &str, captions: bool, performance: (bool, bool)| -> Option<f32> {
            Some(match id {
                "browser" => theme::ROW,
                // The time signature fits "99 / 16" and its chevron in the bar's font; a theme that
                // makes a module's font larger gives it the width as well, so styling never
                // reflows the bar.
                "tempo" => 56.0,
                "signature" => signature_width(ui).max(40.0),
                "keys" => 44.0,
                "grid" => GRID_MENU + if captions { GRID_CAPTION } else { 0.0 },
                "count-in" => COUNT_IN_MENU + if captions { COUNT_IN_CAPTION } else { 0.0 },
                "metronome" => 28.0,
                "transport" => TRANSPORT,
                "position" => 64.0,
                "mapping" if !self.projects.is_empty() => mapping,
                "performance" => match performance {
                    (true, true) => RAM_BLOCK + PAD + CPU_BLOCK,
                    (true, false) => CPU_BLOCK,
                    (false, true) => RAM_BLOCK,
                    (false, false) => return None,
                },
                "views" => views,
                "agents" if !agents.is_empty() => agents_width,
                _ => return None,
            })
        };
        let item = |id: &str, captions: bool, performance: (bool, bool)| {
            width(id, captions, performance).map(|w| if id == "views" { full(id, w) } else { module(id, w) })
        };
        if let Some(order) = &theme_skin.toolbar {
            let items = order
                .order
                .iter()
                .filter(|id| !order.hidden.contains(id))
                .filter_map(|id| item(id, true, (cpu, ram)))
                .collect();
            return vec![items];
        }
        // The default arrangement: what the toolbar has at its start, the transport and the
        // position in the middle, and its end. A narrow window takes the captions first, then the
        // count-in and the metronome, then from the end: the agents, the mapping button, the RAM
        // and the CPU. The view switch always stays.
        let step = |captions, count_in, agent, map, performance| Step { captions, count_in, agent, map, performance };
        let steps = [
            step(true, true, true, true, (cpu, ram)),
            step(false, true, true, true, (cpu, ram)),
            step(false, false, true, true, (cpu, ram)),
            step(false, false, false, true, (cpu, ram)),
            step(false, false, false, false, (cpu, ram)),
            step(false, false, false, false, (cpu, false)),
            step(false, false, false, false, (false, false)),
        ];
        let mut candidates: Vec<Vec<Item>> = Vec::new();
        for Step { captions, count_in, agent, map, performance } in steps {
            let mut items = vec![Item::named("group", "toolbar-start")];
            let start = ["browser", "tempo", "signature", "keys", "grid"]
                .into_iter()
                .chain(count_in.then_some(["count-in", "metronome"]).into_iter().flatten());
            items.extend(start.filter_map(|id| item(id, captions, performance)).map(|i| i.in_group(0)));
            items.extend(["transport", "position"].into_iter().filter_map(|id| item(id, captions, performance)));
            let end = items.len();
            items.push(Item::named("group", "toolbar-end"));
            let shown = [("agents", agent), ("mapping", map), ("performance", true), ("views", true)];
            items.extend(
                shown
                    .into_iter()
                    .filter(|(_, on)| *on)
                    .filter_map(|(id, _)| item(id, captions, performance))
                    .map(|i| i.in_group(end)),
            );
            if candidates.last() != Some(&items) {
                candidates.push(items);
            }
        }
        candidates
    }

    /// One module of the toolbar, given as its id, the rectangle the row gave it and the one its
    /// bar and groups leave to it: painting and input stop at the edge of the last.
    fn toolbar_member(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        (module, laid, clip): (&str, Rect, Rect),
        shown: Shown<'_>,
        actions: &mut Vec<Action>,
        (hovered_switch, underline): (&mut Option<View>, &mut Option<(Underline, Rect)>),
    ) {
        let at = themed_toolbar::place(ui, module, laid);
        let scope = crate::kit::element::TOOLBAR_MODULE.id(module).attribute("kind", module).enter();
        let mut module_ui = ui.new_child(egui::UiBuilder::new().id_salt(("toolbar", module)).max_rect(at));
        // A module that hides its overflow takes no input past its padding box either.
        module_ui.set_clip_rect(ui.clip_rect().intersect(clip).intersect(scope.input_clip(at)));
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(clip));
        let Shown { grid, cpu, ram } = shown;
        let cx = ModuleCx { grid, cpu, ram, scope: &scope };
        let mut own = None;
        self.toolbar_module(&mut module_ui, &painter, at, module, cx, actions, hovered_switch, &mut own);
        if let Some(own) = own {
            *underline = Some((own, painter.clip_rect()));
        }
    }

    /// One module of the toolbar in `outer`, its whole box, drawn the same wherever the row puts it;
    /// its contents go in the content box, inside the module's border and padding.
    #[allow(clippy::too_many_arguments)]
    fn toolbar_module(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        outer: Rect,
        module: &str,
        cx: ModuleCx<'_>,
        actions: &mut Vec<Action>,
        hovered_switch: &mut Option<View>,
        underline: &mut Option<(f32, f32, f32, egui::Color32)>,
    ) {
        let ModuleCx { grid, cpu, ram, scope } = cx;
        let rect = scope.content_in(outer);
        let surface = |fill| crate::kit::element::Native { radius: 0.0, fill };
        let box_fill = crate::kit::style_class::surface_or(crate::kit::element::app_fill("window"));
        match module {
            "browser" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                self.browser_toggle(ui, painter, rect);
            }
            "tempo" => {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sampler-tempo"), grid.tempo_bpm));
                self.tempo_field(ui, painter, outer, grid.tempo_bpm, Some(scope), actions);
            }
            "signature" => {
                if self.session().is_some() {
                    signature_field(ui, painter, outer, grid, Some(scope), actions);
                }
            }
            "keys" => {
                // The computer keyboard: lit while its keys play notes, with the octave of A (#944).
                let label = if self.typing.on {
                    format!("Computer keyboard on, {}", self.typing.label())
                } else {
                    "Computer keyboard".into()
                };
                let response = widgets::button_area(ui, rect, Id::unique("computer-keyboard"), label);
                if response.clicked() {
                    self.toggle_typing_keys();
                }
                // A `button`, `[checked]` while on: only a rule for that state lights it.
                let lit = widgets::lit_toggle(
                    ui,
                    &response,
                    crate::kit::element::BUTTON.builder(),
                    &[crate::kit::element::Checked],
                    self.typing.on,
                );
                let (fill, ink) = lit.colors();
                scope.paint_with(painter, outer, surface(fill));
                drop(lit);
                let text = if self.typing.on { self.typing.label() } else { "Keys".into() };
                widgets::text_in(painter, rect, Align2::CENTER_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink);
            }
            "grid" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                let (caption, menu) = captioned(rect, GRID_CAPTION, GRID_MENU);
                widgets::text_in(
                    painter,
                    caption,
                    Align2::LEFT_CENTER,
                    "Quantize",
                    theme::ui_font(theme::TEXT_SMALL),
                    hint_ink(),
                );
                let mono = theme::mono_font(theme::TEXT);
                // When launched clips start; the Arrangement has a grid of its own (its toolbar, Ctrl+1 to 4).
                let current = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
                let mut chosen = current;
                ui.scope_builder(egui::UiBuilder::new().max_rect(menu), |ui| {
                    let response = widgets::combo_box(
                        ui,
                        egui::ComboBox::from_id_salt("quantization")
                            .width(menu.width())
                            // Tall enough for every value at once, without a scroll bar.
                            .height(QUANTIZATION.len() as f32 * 2.0 * theme::ROW)
                            .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(scope.color())),
                        |ui| {
                            for q in QUANTIZATION {
                                if crate::kit::menu::item(ui, q, chosen == q).clicked() {
                                    chosen = q;
                                    ui.close();
                                }
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
            }
            "count-in" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                let (caption, menu) = captioned(rect, COUNT_IN_CAPTION, COUNT_IN_MENU);
                widgets::text_in(
                    painter,
                    caption,
                    Align2::LEFT_CENTER,
                    "Count-in",
                    theme::ui_font(theme::TEXT_SMALL),
                    hint_ink(),
                );
                let bars = self.session().map_or(0, |s| s.count_in_bars());
                crate::session::record_controls::count_in_menu(ui, menu, bars, actions);
            }
            "metronome" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                crate::session::record_controls::metronome_button(
                    ui,
                    painter,
                    rect,
                    &grid.metronome,
                    grid.click_route,
                    actions,
                );
            }
            "transport" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                self.transport(ui, painter, (scope, outer), grid, actions);
            }
            "position" => {
                scope.paint_with(painter, outer, surface(box_fill));
                widgets::text_in(
                    painter,
                    rect.shrink2(Vec2::new(PAD, 0.0)),
                    Align2::LEFT_CENTER,
                    &grid.position(),
                    theme::mono_font(theme::TEXT),
                    scope.color(),
                );
            }
            "mapping" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                // The mapping mode's button, lit while the mode is on.
                let on = self.view.launch_keys.mode.on;
                let response = widgets::button_area(ui, rect, Id::unique("map-keys"), "Map keys to slots and scenes");
                if response.clicked() {
                    self.toggle_mapping_mode();
                }
                // A `button[kind="map"]`, `[checked]` while on: its ink, lit and under the pointer.
                let el = crate::kit::element::BUTTON
                    .attribute("kind", "map")
                    .response(&response)
                    .attr(crate::kit::element::Checked, on)
                    .enter();
                let color = el.hover_fade(widgets::hover(ui, &response), Look::color);
                drop(el);
                widgets::text_in(painter, rect, Align2::LEFT_CENTER, "Map", theme::ui_font(theme::TEXT), color);
            }
            "performance" => {
                // The program's own RAM (its resident memory), then the CPU load from the engine's
                // monitor (per mille, smoothed): how much of the time the audio engine spends
                // rendering. With room for one, the CPU.
                let both = rect.width() + 0.5 >= RAM_BLOCK + PAD + CPU_BLOCK;
                let mut left = rect.left();
                if let Some(bytes) = ram.filter(|_| both || cpu.is_none()) {
                    let block =
                        Rect::from_min_max(rect.min, Pos2::new((left + RAM_BLOCK).min(rect.right()), rect.bottom()));
                    left = block.right() + PAD;
                    scope.paint_with(painter, block, surface(box_fill));
                    ram_block(ui, painter, block, bytes);
                }
                if let Some(milli) = cpu {
                    let block = Rect::from_min_max(Pos2::new(left.min(rect.right()), rect.top()), rect.max);
                    scope.paint_with(painter, block, surface(box_fill));
                    cpu_block(ui, painter, block, milli);
                }
            }
            "views" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                // The view switch, the current view underlined. A clip dragged in the Arrangement
                // view, or carried, switches the view when it hovers over the other view's name for
                // a moment.
                let carrying = self.carrying_clip(ui.ctx());
                let pointer = ui.ctx().pointer_latest_pos();
                let mut left = rect.left();
                let (shown_ink, other_ink) = (scope.color(), caption_ink());
                let target = crate::kit::element::DROP_TARGET.attribute("kind", "area").look().color();
                for (view, name) in [(View::Session, "Launcher"), (View::Arrangement, "Arrangement")] {
                    let color = if self.shown == view { shown_ink } else { other_ink };
                    let galley = widgets::fitted(painter, name, theme::ui_font(theme::TEXT), color, 120.0);
                    let size = galley.size();
                    let at = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(left + size.x, rect.bottom()));
                    left = at.right() + 12.0;
                    let response = widgets::button_area(ui, at, Id::unique(("view", name)), format!("{name} view"));
                    if response.clicked() {
                        self.shown = view;
                    }
                    // Only where the name shows: a name cut by its group or the bar is not a target.
                    let shown_at = ui.clip_rect();
                    if carrying
                        && self.shown != view
                        && pointer.is_some_and(|p| at.expand(4.0).contains(p) && shown_at.contains(p))
                    {
                        *hovered_switch = Some(view);
                        painter.rect_stroke(at.expand(3.0), 0.0, theme::hairline(target), egui::StrokeKind::Inside);
                    }
                    let color = motion::mix(color, shown_ink, widgets::hover(ui, &response));
                    let pos = Pos2::new(at.left(), at.center().y - size.y / 2.0);
                    widgets::paint_text(painter, pos, galley, color);
                    if self.shown == view {
                        *underline = Some((pos.x, pos.y + size.y + 1.0, size.x, shown_ink));
                    }
                }
            }
            "agents" => {
                scope.paint_with(painter, outer, surface(egui::Color32::TRANSPARENT));
                // Each agent the person let in (ADR 0028), lit; a click disconnects it.
                let mut left = rect.left();
                // Each a `button[kind="agent"]`: its ink, and under the pointer.
                let lit = crate::kit::element::BUTTON.attribute("kind", "agent").look().color();
                for (connection, client) in self.agents.clone() {
                    let text = format!("Disconnect {client}");
                    let galley = widgets::fitted(painter, &text, theme::ui_font(theme::TEXT), lit, 200.0);
                    let size = galley.size();
                    let at = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(left + size.x, rect.bottom()));
                    left = at.right() + PAD;
                    let response = widgets::button_area(ui, at, Id::unique(("agent", connection)), text.clone());
                    if response.clicked() {
                        self.agent_disconnects.push(connection);
                        self.status = Status::Done(format!("Disconnected {client}"));
                    }
                    let el = crate::kit::element::BUTTON.attribute("kind", "agent").response(&response).enter();
                    let color = el.hover_fade(widgets::hover(ui, &response), Look::color);
                    drop(el);
                    widgets::paint_text(painter, Pos2::new(at.left(), at.center().y - size.y / 2.0), galley, color);
                }
            }
            _ => {}
        }
    }

    /// Play, stop and record in the transport module (`scope`, its whole box `outer`), where the
    /// module lays them out (#1453): 28 points wide, a point apart, by the user-agent sheet.
    fn transport(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        (scope, outer): (&crate::kit::element::Entered, Rect),
        grid: &TrackGrid,
        actions: &mut Vec<Action>,
    ) {
        use crate::kit::native_css::row::{self, Bar, Item};
        let items: Vec<Item> = ["play", "stop", "record"]
            .into_iter()
            .map(|kind| Item::new("button").id(&format!("control-{kind}")).attribute("kind", kind).width(28.0))
            .collect();
        // Named as it is entered, for a frame the style engine does not run in.
        let bar = Bar { id: Some("transport"), ..Bar::of(scope, outer.size()) }.attribute("kind", "transport");
        let laid = row::row(&bar, &items).unwrap_or_default();
        let [play, stop, record] = [0, 1, 2].map(|index| laid.at(index, outer.min));
        if let Some(play) = play {
            let response = widgets::button_area(ui, play, Id::unique("play"), "Play");
            let play_style = crate::kit::element::BUTTON
                .id("play")
                .attribute("kind", "play")
                .response(&response)
                .attr(crate::kit::element::Playing, grid.running)
                .attr(crate::kit::element::Checked, grid.running)
                .enter();
            if response.clicked() {
                actions.push(self.play_action());
            }
            let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
            let hover = motion::mix(
                crate::kit::element::app_fill("control"),
                crate::kit::element::app_fill("control-hover"),
                widgets::hover(ui, &response),
            );
            widgets::button_surface(
                painter,
                play,
                &play_style,
                &response,
                grid.running,
                motion::mix(hover, crate::kit::element::app_fill("control-active"), running),
                0.0,
            );
            // A seek from the ruler waiting for its boundary flashes the triangle (#657).
            let pending = self.session().and_then(|s| s.player()).and_then(|p| p.monitor().pending_jump()).is_some();
            let flash = pending && (ui.input(|i| i.time) * 4.0).floor() as i64 % 2 == 0;
            if pending {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
            }
            // Its ink at rest, `[playing]` while the transport runs (the accent), fading between them;
            // a pending seek flashes the ink at rest.
            let rest = || crate::kit::element::BUTTON.id("play").attribute("kind", "play").probe().color();
            let glyph = if flash {
                rest()
            } else if running < 1.0 && grid.running {
                motion::mix(rest(), play_style.color(), running)
            } else if running > 0.0 && !grid.running {
                let playing = crate::kit::element::BUTTON
                    .id("play")
                    .attribute("kind", "play")
                    .attr(crate::kit::element::Playing, true)
                    .attr(crate::kit::element::Checked, true)
                    .probe()
                    .color();
                motion::mix(play_style.color(), playing, running)
            } else {
                play_style.color()
            };
            let c = play.center();
            if !crate::kit::skin::paint_as(
                &play_style,
                painter,
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
        if let Some(stop) = stop {
            let response = widgets::button_area(ui, stop, Id::unique("stop"), "Stop");
            let stop_style = crate::kit::element::BUTTON.id("stop").response(&response).enter();
            if response.clicked() {
                actions.push(self.stop_action());
            }
            widgets::button_surface(
                painter,
                stop,
                &stop_style,
                &response,
                false,
                stop_style.hover_fade(widgets::hover(ui, &response), Look::background),
                0.0,
            );
            let stop_ink = stop_style.color();
            if !crate::kit::skin::paint_as(
                &stop_style,
                painter,
                stop,
                "stop",
                crate::kit::skin::State::response(&response, false),
                stop_ink,
            ) {
                painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, stop_ink);
            }
            drop(stop_style);
        }
        let recording = self.session().is_some_and(|s| s.records_arrangement());
        if let Some(record) = record {
            crate::session::record_controls::record_button(ui, painter, record, recording, actions);
        }
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
            .attribute("kind", "browser")
            .response(&response)
            .attr(crate::kit::element::Checked, self.settings.browser_open)
            .enter();
        widgets::button_surface(
            painter,
            browser_rect,
            &browser_style,
            &response,
            self.settings.browser_open,
            browser_style.hover_fade(widgets::hover(ui, &response), Look::background),
            0.0,
        );
        // A panel with its left part filled while the browser shows.
        let icon = Rect::from_center_size(browser_rect.center(), Vec2::new(14.0, 11.0));
        // `button[kind="browser"]`'s ink, `[checked]` while the browser shows.
        let ink = browser_style.color();
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

    /// The status bar's items in `ui`'s rect, its whole box: `style_class` is the bar's element,
    /// which the window entered and painted. The message and the counts are laid out in a row.
    pub(super) fn status_bar(&self, ui: &mut Ui, style_class: &crate::kit::element::Entered, grid: Option<&TrackGrid>) {
        use crate::kit::native_css::row::{self, Bar, Item};
        let mut styled_ui = style_class.ui(ui);
        let ui = &mut *styled_ui;
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = if self.session().is_some_and(Session::audio_preparing) {
            ("Preparing audio for playback…", caption_ink())
        } else {
            match &self.status {
                Status::Hint(t) => (t.as_str(), caption_ink()),
                Status::Done(t) => (t.as_str(), caption_ink()),
                Status::Error(t) => (t.as_str(), error_ink()),
            }
        };
        let backed_up = self
            .session()
            .and_then(Session::last_backup)
            .map(|b| format!(" · backed up {}", suhina_session::backup::clock_time(b.written)));
        let counts = grid.map(|g| {
            let tracks = g.columns.iter().filter(|c| c.kind != crate::interaction::grid::ColumnKind::Return).count();
            format!("{tracks} tracks · {} scenes{}", g.scenes.len(), backed_up.as_deref().unwrap_or(""))
        });
        // The message takes the room left; the counts are as wide as their text.
        let font = theme::ui_font(theme::TEXT_SMALL);
        let mut items = vec![Item::named("bar-item", "status-message")];
        if let Some(counts) = &counts {
            items.push(Item::named("bar-item", "status-counts").width(widgets::text_width(ui, counts, font.clone())));
        }
        let Some(laid) = row::row(&Bar::of(style_class, bar.size()), &items) else { return };
        // The message's color says what it is (an error is red) unless a rule colors the item
        // itself; the bar's own `color` does not.
        if let Some(rect) = laid.at(0, bar.min) {
            let item = crate::kit::element::BAR_ITEM.id("status-message").attribute("kind", "status-message").enter();
            let content = item.paint(&painter, rect).content;
            let ink = item.own_color_or(color);
            widgets::text_in(&painter, content, Align2::LEFT_CENTER, text, font.clone(), ink);
        }
        if let (Some(counts), Some(rect)) = (counts, laid.at(1, bar.min)) {
            let item = crate::kit::element::BAR_ITEM.id("status-counts").attribute("kind", "status-counts").enter();
            let content = item.paint(&painter, rect).content;
            let ink = item.color();
            widgets::text_in(&painter, content, Align2::RIGHT_CENTER, &counts, font, ink);
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
    /// a second in one undo step (#916). `native`, the tempo module's element, paints the field's surface
    /// the tempo module's.
    pub(super) fn tempo_field(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        tempo_rect: Rect,
        bpm_now: f64,
        native: Option<&crate::kit::element::Entered>,
        actions: &mut Vec<Action>,
    ) {
        let grab = self.tempo_grab;
        let tempo_shown = self.tempo_drag.map_or(bpm_now, |bpm| snap_tempo(grab, bpm));
        let tempo_text = format!("{tempo_shown:.3}");
        let response = widgets::value_area(ui, tempo_rect, Id::unique("tempo"), "Tempo", tempo_text.clone());
        if let Some(module) = native {
            let fill = motion::mix(
                crate::kit::element::app_fill("control"),
                crate::kit::element::app_fill("control-hover"),
                widgets::hover(ui, &response),
            );
            module.paint_with(painter, tempo_rect, crate::kit::element::Native { radius: 0.0, fill });
        }
        let mono = theme::mono_font(theme::TEXT);
        // The module's text goes in its content box, inside its border and padding.
        let content = native.map_or(tempo_rect, |module| module.content_in(tempo_rect));
        let text_area = content.shrink2(Vec2::new(PAD, 0.0));
        let ink = match native {
            Some(module) => module.color(),
            None => value_ink(),
        };
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
            let line = if held.is_some() { crate::kit::element::accent_ink() } else { caption_ink() };
            painter.hline(underline, content.bottom() - 3.0, egui::Stroke::new(1.0, line));
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

/// The toolbar's measures, in points: its menus and their captions, the transport, the RAM and
/// CPU blocks.
const GRID_CAPTION: f32 = 26.0;
const GRID_MENU: f32 = 64.0;
const COUNT_IN_CAPTION: f32 = 50.0;
const COUNT_IN_MENU: f32 = 64.0;
const TRANSPORT: f32 = 28.0 * 3.0 + 2.0;
const RAM_BLOCK: f32 = 76.0;
const CPU_BLOCK: f32 = 56.0;

/// What the default toolbar shows at one step of giving way: the captions, the count-in and the
/// metronome, the agents, the mapping button, and the CPU and the RAM.
struct Step {
    captions: bool,
    count_in: bool,
    agent: bool,
    map: bool,
    performance: (bool, bool),
}

/// Where the view switch's underline goes: its x, y, width and color.
type Underline = (f32, f32, f32, egui::Color32);

/// What the toolbar's modules draw from, before a module's element is entered.
#[derive(Clone, Copy)]
struct Shown<'a> {
    grid: &'a TrackGrid,
    cpu: Option<u32>,
    ram: Option<u64>,
}

/// What a toolbar module draws from.
#[derive(Clone, Copy)]
struct ModuleCx<'a> {
    grid: &'a TrackGrid,
    cpu: Option<u32>,
    ram: Option<u64>,
    /// The module's element, entered.
    scope: &'a crate::kit::element::Entered,
}

/// A module's caption, `caption` wide at its left when the module has room for it beside a menu
/// `menu` wide, and the menu in the rest.
fn captioned(rect: Rect, caption: f32, menu: f32) -> (Rect, Rect) {
    let caption = if rect.width() + 0.5 >= caption + menu { caption } else { 0.0 };
    let left = Rect::from_min_size(rect.min, Vec2::new(caption, rect.height()));
    (left, Rect::from_min_max(Pos2::new(left.right(), rect.top()), rect.max))
}

/// The RAM block in `rect`: the program's resident memory.
fn ram_block(ui: &Ui, painter: &egui::Painter, rect: Rect, bytes: u64) {
    let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
    widgets::text_in(painter, inner, Align2::LEFT_CENTER, "RAM", theme::ui_font(theme::TEXT_TINY), hint_ink());
    widgets::text_in(
        painter,
        inner,
        Align2::RIGHT_CENTER,
        &ram_text(bytes),
        theme::mono_font(theme::TEXT_SMALL),
        caption_ink(),
    );
    let response = ui.interact(rect, Id::unique("ram-usage"), Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("RAM usage {}", ram_text(bytes))));
}

/// The CPU block in `rect`: the engine's load, `milli` per mille, and a bar of it.
fn cpu_block(ui: &Ui, painter: &egui::Painter, rect: Rect, milli: u32) {
    let percent = f64::from(milli.min(1000)) / 10.0;
    let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
    widgets::text_in(painter, inner, Align2::LEFT_CENTER, "CPU", theme::ui_font(theme::TEXT_TINY), hint_ink());
    widgets::text_in(
        painter,
        inner,
        Align2::RIGHT_CENTER,
        &format!("{percent:.0}%"),
        theme::mono_font(theme::TEXT_SMALL),
        caption_ink(),
    );
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(track, 0.0, crate::kit::element::METER.attribute("kind", "cpu").look().background());
    let fill = track.width() * milli.min(1000) as f32 / 1000.0;
    let full = Rect::from_min_max(track.min, Pos2::new(track.left() + fill, track.bottom()));
    let color = if milli >= 800 {
        crate::kit::element::METER_LEVEL.attribute("kind", "hot").look().color()
    } else {
        crate::kit::element::METER_LEVEL.look().color()
    };
    painter.rect_filled(full, 0.0, color);
    let response = ui.interact(rect, Id::unique("cpu-load"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("CPU load {percent:.1}%")));
}

/// The values a time signature's denominator takes, in the order a drag or an arrow steps
/// through them (`transport.setTimeSignature` refuses any other).
pub(crate) const DENOMINATORS: [u8; 5] = [1, 2, 4, 8, 16];

/// The width of the chevron at the right of the time signature that opens the common ones.
const SIGNATURE_CHEVRON: f32 = 14.0;

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
    outer: Rect,
    grid: &TrackGrid,
    native: Option<&crate::kit::element::Entered>,
    actions: &mut Vec<Action>,
) {
    use crate::interaction::menus::{SameSignature, signature_choice};
    // The numbers and the chevron go in the module's content box, inside its border and padding.
    let rect = native.map_or(outer, |module| module.content_in(outer));
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
    if let Some(module) = native {
        let hovered =
            [&numerator, &denominator, &chevron].into_iter().map(|r| widgets::hover(ui, r)).fold(0.0, f32::max);
        let fill = motion::mix(
            crate::kit::element::app_fill("control"),
            crate::kit::element::app_fill("control-hover"),
            hovered,
        );
        module.paint_with(painter, outer, crate::kit::element::Native { radius: 0.0, fill });
    }

    // Typing: a click on a number gives the field the keys, from that number.
    let ctx = ui.ctx().clone();
    let typing_id = Id::unique("time-signature-typing");
    // While typing, a click on the other number goes on typing there and keeps what was typed.
    for (response, in_denominator) in [(&numerator, false), (&denominator, true)] {
        if response.clicked() {
            let typing = match ctx.data(|d| d.get_temp::<SignatureTyping>(typing_id)) {
                Some(typed) if widgets::holds_value_keys(&ctx, typing_id) => {
                    SignatureTyping { in_denominator, ..typed }
                }
                _ => SignatureTyping::from(in_denominator),
            };
            ctx.data_mut(|d| d.insert_temp(typing_id, typing));
            widgets::hold_value_keys(&ctx, typing_id, true);
        }
    }
    let mut typing = ctx.data(|d| d.get_temp::<SignatureTyping>(typing_id));
    let mut changes: Vec<(serde_json::Value, serde_json::Value, bool)> = Vec::new();
    // Another value control clicked or a press anywhere else lets the keys go and sets what was
    // typed, as Enter would; only Escape leaves it. A drag on the field drops it: the drag sets
    // the number.
    let pressed_elsewhere = ui.input(|i| i.pointer.any_pressed()) && !numerator.hovered() && !denominator.hovered();
    let drag_started = [&numerator, &denominator].into_iter().any(|r| r.drag_started());
    let keys_gone = !widgets::holds_value_keys(&ctx, typing_id);
    if let Some(typed) = typing.as_ref()
        && (keys_gone || pressed_elsewhere || drag_started)
    {
        if !drag_started && let Some((n, d)) = typed.signature(current) {
            changes.push((n, d, false));
        }
        typing = None;
    }
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
    let ink = native.map_or_else(caption_ink, |module| module.color());
    let half_slash = widgets::text_width(ui, " / ", mono.clone()) / 2.0;
    let left = Rect::from_min_max(numbers.min, Pos2::new(middle - half_slash, numbers.bottom()));
    let right = Rect::from_min_max(Pos2::new(middle + half_slash, numbers.top()), numbers.max);
    let numerator_now = numerator_preview.map_or(shown.numerator, |n| n.round().clamp(1.0, 99.0) as u8).to_string();
    let denominator_now = denominator_preview.map_or(shown.denominator, denominator_at).to_string();
    // A number not typed into yet shows its value dimmed, the outlined one too, so the arrow keys'
    // steps show while it holds the keys; the first digit replaces it.
    let part = |typed: Option<&String>, now: String| match typed {
        Some(t) if !t.is_empty() => (t.clone(), value_ink()),
        Some(_) => (now, hint_ink()),
        None => (now, ink),
    };
    let (numerator_text, numerator_ink) = part(typing.as_ref().map(|t| &t.numerator), numerator_now);
    let (denominator_text, denominator_ink) = part(typing.as_ref().map(|t| &t.denominator), denominator_now);
    widgets::text_in(painter, left, Align2::RIGHT_CENTER, &numerator_text, mono.clone(), numerator_ink);
    widgets::text_in(painter, numbers, Align2::CENTER_CENTER, "/", mono.clone(), ink);
    widgets::text_in(painter, right, Align2::LEFT_CENTER, &denominator_text, mono.clone(), denominator_ink);
    // The number typed into is outlined.
    if let Some(typed) = &typing {
        let half = if typed.in_denominator { denominator_rect } else { numerator_rect };
        painter.rect_stroke(
            half.shrink2(Vec2::new(1.0, 2.0)),
            3.0,
            egui::Stroke::new(1.0, caption_ink()),
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
            let line = if dragged { crate::kit::element::accent_ink() } else { caption_ink() };
            painter.hline(span, rect.bottom() - 3.0, egui::Stroke::new(1.0, line));
        }
    }

    // Only the chevron opens the list of common signatures.
    let menu_id = chevron.id.with("menu");
    let open = egui::Popup::is_id_open(ui.ctx(), menu_id);
    let chevron_ink = if chevron.hovered() || open { crate::kit::element::name_ink() } else { caption_ink() };
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
