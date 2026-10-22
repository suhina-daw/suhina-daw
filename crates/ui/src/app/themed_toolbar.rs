// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reorderable native toolbar modules. Normal controls retain their command paths and ids.
use super::*;
use crate::kit::skin::{self, State, Toolbar};
use crate::settings::settings_view::ThemeRequest;

fn edit_id() -> Id {
    Id::new("edit-theme-toolbar")
}

#[derive(Clone, Default)]
struct EditDraft {
    theme_id: String,
    original: skin::Skin,
    skin: skin::Skin,
}

fn rects_id() -> Id {
    Id::new("toolbar-edit-rects")
}

/// Restores each sibling module before entering the next one; geometry remains native.
#[derive(Default)]
pub(super) struct NativeStyles {
    module: String,
    scope: Option<crate::kit::style_class::Scope>,
}
impl NativeStyles {
    pub(super) fn enter(&mut self, module: &str) {
        if self.module != module {
            self.scope.take();
            self.module = module.into();
            self.scope = Some(crate::kit::style_class::enter_named("toolbar-module", module));
        }
    }
    pub(super) fn place(&mut self, ui: &Ui, module: &str, base: Rect) -> Rect {
        self.enter(module);
        place(ui, module, base)
    }
}

/// A module's offset is relative to its existing responsive anchor, never a new layout.
pub(super) fn place(ui: &Ui, module: &str, base: Rect) -> Rect {
    let draft = ui.ctx().data(|d| d.get_temp::<EditDraft>(edit_id()));
    let offset = draft
        .as_ref()
        .map(|d| &d.skin)
        .map_or_else(|| skin::active_toolbar_offset(module), |s| s.toolbar_offsets.get(module).copied().unwrap_or(0));
    let at = base.translate(Vec2::new(offset as f32, 0.0));
    let at = crate::kit::control_layout::place(&format!("toolbar-{module}"), at, ui.max_rect());
    ui.ctx().data_mut(|d| {
        let rects = d.get_temp_mut_or_default::<Vec<(String, Rect, Rect)>>(rects_id());
        if let Some(item) = rects.iter_mut().find(|item| item.0 == module) {
            item.1 = item.1.union(base);
            item.2 = item.2.union(at);
        } else {
            rects.push((module.to_owned(), base, at));
        }
    });
    at
}

impl App {
    /// Returns true when the custom layout has rendered the bar.
    pub(super) fn themed_toolbar(
        &mut self,
        ui: &mut Ui,
        grid: Option<&Grid>,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
    ) -> bool {
        let rect = ui.max_rect();
        let response = ui.interact(rect, Id::new("toolbar-context"), Sense::click());
        ui.ctx().data_mut(|d| d.insert_temp(rects_id(), Vec::<(String, Rect, Rect)>::new()));
        ui.ctx().data_mut(|d| {
            if d.get_temp::<EditDraft>(edit_id()).is_some_and(|draft| draft.theme_id != self.settings.theme) {
                d.remove::<EditDraft>(edit_id());
            }
        });
        let editing = ui.ctx().data(|d| d.get_temp::<EditDraft>(edit_id())).is_some();
        let mut theme_skin = skin::current();
        let mut changed = false;
        response.context_menu(|ui| {
            if ui.button("Edit interface").clicked() {
                self.start_control_editor(ui.ctx());
                ui.ctx().data_mut(|data| data.remove::<EditDraft>(edit_id()));
                ui.close();
            }
            if !editing && ui.button("Edit toolbar").clicked() {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        edit_id(),
                        EditDraft {
                            theme_id: self.settings.theme.clone(),
                            original: theme_skin.as_ref().clone(),
                            skin: theme_skin.as_ref().clone(),
                        },
                    )
                });
                ui.close();
            }
            if ui.button("Restore default toolbar").clicked() {
                std::sync::Arc::make_mut(&mut theme_skin).toolbar = None;
                std::sync::Arc::make_mut(&mut theme_skin).toolbar_offsets.clear();
                changed = true;
                ui.ctx().data_mut(|d| d.remove::<EditDraft>(edit_id()));
                ui.close();
            }
        });
        if changed {
            self.save_toolbar_skin(theme_skin.as_ref().clone());
        }
        if editing {
            ui.ctx().memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        let default_layout = theme_skin.toolbar.is_none();
        let Some(toolbar) = theme_skin.toolbar.clone().or_else(|| theme_skin.css.as_ref().map(|_| Toolbar::default()))
        else {
            return false;
        };
        {
            let modules: Vec<_> = toolbar
                .order
                .iter()
                .filter(|id| !toolbar.hidden.contains(id))
                .map(|id| (id.clone(), self.toolbar_module_width(id)))
                .collect();
            if let Some(modules) = crate::kit::native_css::toolbar(&modules, rect.size()) {
                if default_layout && !crate::kit::native_css::toolbar_layout_requested() {
                    return false;
                }
                let mut hovered_switch = None;
                let extent = modules.iter().map(|module| module.rect.right()).fold(rect.width(), f32::max);
                egui::ScrollArea::horizontal().id_salt("custom-toolbar-scroll").auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        let (_, content) = ui.allocate_space(Vec2::new(extent, rect.height()));
                        for module in modules {
                            let at = place(ui, &module.name, module.rect.translate(content.min.to_vec2()));
                            if !at.intersects(ui.clip_rect()) {
                                continue;
                            }
                            let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(at));
                            painter.rect_filled(
                                at,
                                module.style.radius,
                                module.style.background.gamma_multiply(module.style.opacity),
                            );
                            crate::kit::native_css::background_with(&painter, at, &module.style);
                            if let Some(grid) = grid {
                                let mut module_ui = ui.new_child(
                                    egui::UiBuilder::new().id_salt(("css-toolbar", &module.name)).max_rect(at),
                                );
                                module_ui.set_clip_rect(ui.clip_rect().intersect(at));
                                self.toolbar_module(
                                    &mut module_ui,
                                    at,
                                    &module.name,
                                    grid,
                                    cpu,
                                    ram,
                                    actions,
                                    &mut hovered_switch,
                                );
                            }
                        }
                    },
                );
                self.hover_switch(ui.ctx(), hovered_switch);
                self.toolbar_edit_overlay(ui, rect);
                return true;
            }
        }
        let mut hovered_switch = None;
        egui::ScrollArea::horizontal().id_salt("custom-toolbar-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_height(rect.height());
            ui.horizontal(|ui| {
                for module in &toolbar.order {
                    let hidden = toolbar.hidden.contains(module);
                    if hidden {
                        continue;
                    }
                    let width = self.toolbar_module_width(module);
                    let (_, at) = ui.allocate_space(Vec2::new(width, (rect.height() - 4.0).max(24.0)));
                    let at = place(ui, module, at);
                    if let Some(grid) = grid {
                        self.toolbar_module(ui, at, module, grid, cpu, ram, actions, &mut hovered_switch);
                    }
                }
            });
        });
        self.hover_switch(ui.ctx(), hovered_switch);
        self.toolbar_edit_overlay(ui, rect);
        true
    }

    pub(super) fn toolbar_edit_overlay(&mut self, ui: &Ui, bar: Rect) {
        let Some(mut draft) = ui.ctx().data(|d| d.get_temp::<EditDraft>(edit_id())) else {
            return;
        };
        let rects = ui.ctx().data(|d| d.get_temp::<Vec<(String, Rect, Rect)>>(rects_id())).unwrap_or_default();
        let mut done = false;
        let mut cancel = ui.input(|i| i.key_pressed(Key::Escape));
        egui::Area::new(Id::new("toolbar-edit-overlay"))
            .order(egui::Order::Foreground)
            .movable(false)
            .fixed_pos(bar.min)
            .show(ui.ctx(), |overlay| {
                overlay.set_min_size(bar.size());
                overlay.set_clip_rect(bar);
                let painter = overlay.painter();
                let ink = theme::accent();
                for x in (0..=(bar.width().min(16384.0) / 8.0) as usize).map(|n| bar.left() + n as f32 * 8.0) {
                    painter.vline(x, bar.y_range(), egui::Stroke::new(0.5, ink.gamma_multiply(0.18)));
                }
                for y in (0..=(bar.height().min(1024.0) / 8.0) as usize).map(|n| bar.top() + n as f32 * 8.0) {
                    painter.hline(bar.x_range(), y, egui::Stroke::new(0.5, ink.gamma_multiply(0.18)));
                }
                for (name, base, at) in &rects {
                    let response = overlay.interact(*at, Id::new(("move-toolbar", name)), Sense::click_and_drag());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Move toolbar {name}"))
                    });
                    overlay.painter().rect_stroke(*at, 2.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
                    if response.drag_started() {
                        overlay.ctx().data_mut(|d| d.insert_temp(response.id.with("origin"), at.left()));
                    }
                    if response.dragged() {
                        let origin =
                            overlay.ctx().data(|d| d.get_temp::<f32>(response.id.with("origin"))).unwrap_or(at.left());
                        let left = ((origin + response.total_drag_delta().unwrap_or_default().x - bar.left()) / 8.0)
                            .round()
                            * 8.0
                            + bar.left();
                        let left = left.clamp(bar.left(), (bar.right() - at.width()).max(bar.left()));
                        draft
                            .skin
                            .toolbar_offsets
                            .insert(name.clone(), (left - base.left()).round().clamp(-16384.0, 16384.0) as i32);
                        overlay.ctx().request_repaint();
                    }
                    response.on_hover_text(format!("Drag {name} · snaps to 8 point grid"));
                }
            });
        egui::Window::new("Edit toolbar")
            .id(Id::new("toolbar-edit-tools"))
            .default_pos(bar.left_bottom() + Vec2::new(8.0, 8.0))
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |tools| {
                tools.label("Drag the outlined controls · 8 point snap grid");
                tools.horizontal(|tools| {
                    done = tools.button("Done").clicked();
                    cancel |= tools.button("Cancel").clicked();
                    if tools.button("Reset positions").clicked() {
                        draft.skin.toolbar_offsets.clear();
                    }
                });
            });
        if done || cancel {
            ui.ctx().data_mut(|d| d.remove::<EditDraft>(edit_id()));
            if done && draft.skin.toolbar_offsets != draft.original.toolbar_offsets {
                let mut current = self.themes.get(&self.settings.theme).map(|t| t.skin.clone()).unwrap_or_default();
                current.toolbar_offsets = draft.skin.toolbar_offsets;
                self.save_toolbar_skin(current);
            }
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(edit_id(), draft));
        }
    }

    fn toolbar_module_width(&self, module: &str) -> f32 {
        match module {
            "browser" | "metronome" => 28.0,
            "tempo" => 62.0,
            "signature" => 45.0,
            "grid" | "count-in" => 70.0,
            "transport" => 86.0,
            "position" => 78.0,
            "mapping" => 64.0,
            "performance" => 145.0,
            "views" => 172.0,
            "agents" => (self.agents.len() as f32 * 140.0).max(1.0),
            _ => 0.0,
        }
    }

    pub(super) fn save_toolbar_skin(&mut self, skin: skin::Skin) {
        if let Some(id) = self.themes.request(&self.settings.theme, ThemeRequest::EditSkin(skin)) {
            self.settings.theme = id;
            self.settings_changed = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn toolbar_module(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        module: &str,
        grid: &Grid,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
        hovered_switch: &mut Option<View>,
    ) {
        let _module_style = crate::kit::style_class::enter_named("toolbar-module", module);
        let painter = ui.painter_at(rect);
        let middle = |index: f32, width: f32| {
            Rect::from_min_size(egui::pos2(rect.left() + index, rect.top()), Vec2::new(width, rect.height()))
        };
        match module {
            "browser" => {
                if widgets::button(
                    ui,
                    &painter,
                    rect,
                    "☰",
                    if self.settings.browser_open { "Hide browser" } else { "Show browser" },
                ) {
                    self.toggle_browser();
                }
            }
            "tempo" => {
                ui.ctx().data_mut(|d| d.insert_temp(Id::new("sampler-tempo"), grid.tempo_bpm));
                let shown = self.tempo_drag.unwrap_or(grid.tempo_bpm);
                let response = widgets::value_area(ui, rect, Id::new("tempo"), "Tempo", format!("{shown:.2}"));
                if let Drag::Done(bpm) = widgets::drag_value(
                    ui,
                    &response,
                    &mut self.tempo_drag,
                    grid.tempo_bpm,
                    0.1,
                    1.0,
                    (20.0, 999.0),
                    120.0,
                ) {
                    actions.push(Action::new("transport.setTempo", json!({"bpm":(bpm*100.0).round()/100.0})));
                }
                widgets::text_in(
                    &painter,
                    rect,
                    Align2::CENTER_CENTER,
                    &format!("{shown:.2}"),
                    theme::mono_font(theme::TEXT),
                    theme::text_value(),
                );
            }
            "signature" => {
                let ts = grid.time_signature();
                widgets::text_in(
                    &painter,
                    rect,
                    Align2::CENTER_CENTER,
                    &format!("{} / {}", ts.numerator, ts.denominator),
                    theme::mono_font(theme::TEXT),
                    theme::text_secondary(),
                );
            }
            "grid" => {
                let current = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
                let mut chosen = current;
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    let response = widgets::combo_box(
                        ui,
                        egui::ComboBox::from_id_salt("quantization")
                            .width(rect.width())
                            .height(QUANTIZATION.len() as f32 * 2.0 * theme::ROW)
                            .selected_text(chosen),
                        |ui| {
                            for q in QUANTIZATION {
                                ui.selectable_value(&mut chosen, q, q);
                            }
                        },
                    )
                    .response;
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, QUANTIZATION_LABEL)
                    });
                });
                if chosen != current {
                    actions.push(Action::new("launcher.setQuantization", json!({"value":chosen})));
                }
            }
            "count-in" => crate::session::record_controls::count_in_menu(
                ui,
                rect,
                self.session().map_or(0, |s| s.count_in_bars()),
                actions,
            ),
            "metronome" => {
                crate::session::record_controls::metronome_button(ui, &painter, rect, grid.metronome, actions)
            }
            "transport" => {
                for (index, role, label, on) in [(0, "play", "Play", grid.running), (1, "stop", "Stop", false)] {
                    if crate::kit::control_layout::placement(&format!("transport-{role}")).hidden {
                        continue;
                    }
                    let at = middle(index as f32 * 29.0, 28.0);
                    let at = crate::kit::control_layout::place(&format!("transport-{role}"), at, ui.max_rect());
                    let response = widgets::button_area(ui, at, Id::new(role), label);
                    let _style = crate::kit::style_class::enter_named_response("button", role, &response);
                    if response.clicked() {
                        actions.push(if index == 0 { self.play_action(false) } else { self.stop_action() });
                    }
                    let state = State::response(&response, on);
                    let ink =
                        crate::kit::style_class::foreground(if on { theme::accent() } else { theme::text_primary() });
                    widgets::button_surface(
                        &painter,
                        at,
                        &response,
                        on,
                        if on { theme::control_active() } else { theme::control_background() },
                        0.0,
                    );
                    if !skin::paint(&painter, at, role, state, ink, 0.0) {
                        if index == 0 {
                            widgets::play_glyph(&painter, at.center(), false, ink, 10.0);
                        } else {
                            painter.rect_filled(Rect::from_center_size(at.center(), Vec2::splat(8.0)), 0.0, ink);
                        }
                    }
                }
                crate::session::record_controls::record_button(
                    ui,
                    &painter,
                    middle(58.0, 28.0),
                    self.session().is_some_and(|s| s.records_arrangement()),
                    actions,
                );
            }
            "position" => widgets::text_in(
                &painter,
                rect,
                Align2::CENTER_CENTER,
                &grid.position(),
                theme::mono_font(theme::TEXT),
                theme::text_primary(),
            ),
            "mapping" => {
                if widgets::button(ui, &painter, rect, "Map keys", "Map keys to slots and scenes") {
                    self.toggle_mapping_mode();
                }
            }
            "performance" => {
                let text = format!(
                    "CPU {}  RAM {}",
                    cpu.map_or_else(|| "—".into(), |n| format!("{:.0}%", f64::from(n.min(1000)) / 10.0)),
                    ram.map_or_else(|| "—".into(), ram_text)
                );
                widgets::text_in(
                    &painter,
                    rect,
                    Align2::CENTER_CENTER,
                    &text,
                    theme::mono_font(theme::TEXT_SMALL),
                    theme::text_secondary(),
                );
                let response = ui.interact(rect, Id::new("cpu-ram"), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
            }
            "views" => {
                let carrying = self.carry.is_some() || self.arrangement.dragged_clip().is_some();
                for (index, view, name) in [(0, View::Session, "Session"), (1, View::Arrangement, "Arrangement")] {
                    let at = middle(index as f32 * 86.0, 86.0);
                    if widgets::button(ui, &painter, at, name, &format!("{name} view")) {
                        self.shown = view;
                    }
                    if self.shown == view {
                        painter.hline(at.x_range(), at.bottom() - 1.0, theme::hairline(theme::accent()));
                    }
                    if carrying && self.shown != view && ui.ctx().pointer_latest_pos().is_some_and(|p| at.contains(p)) {
                        *hovered_switch = Some(view);
                    }
                }
            }
            "agents" => {
                for (index, (connection, client)) in self.agents.clone().into_iter().enumerate() {
                    let label = format!("Disconnect {client}");
                    if widgets::button(ui, &painter, middle(index as f32 * 140.0, 140.0), &label, &label) {
                        self.agent_disconnects.push(connection);
                        self.status = Status::Done(format!("Disconnected {client}"));
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn responsive_theme_survives_live_reload_and_resize() {
        std::thread::Builder::new()
            .name("native-window-stack".into())
            .stack_size(1024 * 1024)
            .spawn(|| {
                let css = include_str!("../../tests/fixtures/responsive-theme.css");
                let ctx = egui::Context::default();
                let mut app = App::new();
                app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
                app.open_project(
                    suhina_session::Session::new(crate::demo::demo_project()),
                    &[],
                    crate::app::OpenAs::NewTab,
                );
                app.shown = View::Arrangement;
                app.detail_open = true;
                app.set_browser_open(false);
                let mut theme = theme::Theme::default();
                theme.skin.css = Some(std::sync::Arc::from(css));
                app.set_themes(
                    &ctx,
                    vec![crate::settings::themes::ThemeEntry { id: "responsive".into(), theme }],
                    None,
                );
                app.use_theme(&ctx, "responsive");
                theme::install(&ctx);
                for (browser_index, browser) in [false, true].into_iter().enumerate() {
                    app.set_browser_open(browser);
                    App::set_animations(&ctx, !browser);
                    for (sample, size) in [
                        Vec2::new(1138.0, 1032.0),
                        Vec2::new(1000.0, 1032.0),
                        Vec2::new(820.0, 1032.0),
                        Vec2::new(800.0, 700.0),
                        Vec2::splat(1.0),
                        Vec2::ZERO,
                        Vec2::new(1138.0, 1032.0),
                    ]
                    .into_iter()
                    .cycle()
                    .take(35)
                    .enumerate()
                    {
                        if sample == 0 || sample == 2 {
                            let mut replacement = theme::Theme::default();
                            replacement.skin.css = Some(std::sync::Arc::from(if sample == 0 {
                                ".window { background-color:#0A1120; }"
                            } else {
                                css
                            }));
                            app.put_theme(
                                &ctx,
                                crate::settings::themes::ThemeEntry { id: "responsive".into(), theme: replacement },
                            );
                        }
                        ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                                time: Some((browser_index * 35 + sample) as f64 * 0.025),
                                events: vec![egui::Event::PointerMoved(egui::pos2(
                                    if sample % 2 == 0 { 532.0 } else { 560.0 },
                                    69.0,
                                ))],
                                ..Default::default()
                            },
                            |ui| app.ui(ui),
                        )
                        .textures_delta
                        .clear();
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    struct Fixture {
        app: App,
        grid: Grid,
        actions: Vec<Action>,
    }

    fn native_bar() -> Harness<'static, Fixture> {
        let fixture =
            Fixture { app: App::new(), grid: Grid::build(&crate::demo::demo_project(), None), actions: Vec::new() };
        let mut harness = Harness::builder().with_size((1920.0, 240.0)).build_ui_state(
            |ui, fixture: &mut Fixture| {
                let skin =
                    fixture.app.themes.get(&fixture.app.settings.theme).map(|t| t.skin.clone()).unwrap_or_default();
                skin::configure(&skin);
                let _frame = crate::kit::native_css::frame(ui);
                let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.available_width(), 56.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    fixture.app.control_bar(ui, Some(&fixture.grid), Some(10), Some(1024), &mut fixture.actions);
                });
            },
            fixture,
        );
        theme::install(&harness.ctx);
        harness.run();
        harness
    }

    fn start_edit(harness: &mut Harness<'static, Fixture>) {
        let skin = harness
            .state()
            .app
            .themes
            .get(&harness.state().app.settings.theme)
            .map(|t| t.skin.clone())
            .unwrap_or_default();
        let theme_id = harness.state().app.settings.theme.clone();
        harness.ctx.data_mut(|d| d.insert_temp(edit_id(), EditDraft { theme_id, original: skin.clone(), skin }));
        harness.run();
    }

    #[test]
    fn native_toolbar_css_styles_modules_and_buttons_without_reflow_or_sibling_leaks() {
        let mut harness = native_bar();
        let play = harness.get_by_label("Play").rect();
        let tempo = harness.get_by_label("Tempo").rect();
        harness.state_mut().app.save_toolbar_skin(skin::Skin {
            css:Some(std::sync::Arc::from("#tempo {color:rgb(12 34 56);font-size:18px;} #transport > #control-play {color:rgb(120 40 180);background:linear-gradient(90deg,red,blue);border-radius:7px;}")),
            ..Default::default()
        });
        harness.run();
        assert_eq!(harness.get_by_label("Play").rect(), play);
        assert_eq!(harness.get_by_label("Tempo").rect(), tempo);
        let tempo_text = format!("{:.2}", harness.state().grid.tempo_bpm);
        let position_text = harness.state().grid.position();
        let text = |label: &str| {
            harness
                .output()
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(&text.galley.job.sections[0].format),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(text(&tempo_text).color, egui::Color32::from_rgb(12, 34, 56));
        assert_eq!(text(&tempo_text).font_id.size, 18.0);
        assert_eq!(text(&position_text).font_id.size, theme::TEXT, "font must not leak into the next module");
        assert!(harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == play && rect.brush.is_some() && rect.corner_radius.nw == 7)));
        assert!(
            harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.calc_bounds() == play && mesh.vertices.len() > 4)),
            "gradient must remain visible above the inherited SVG body"
        );
        harness.get_by_label("Play").click();
        harness.run();
        assert!(harness.state().actions.iter().any(|action| action.id == "transport.play"));
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn dark_gloss_example_keeps_native_layout_and_transport_commands() {
        assert!(crate::kit::native_css::inspect(theme::GLOSS_EXAMPLE).unwrap().is_empty());
        let mut harness = native_bar();
        let play = harness.get_by_label("Play").rect();
        let tempo = harness.get_by_label("Tempo").rect();
        harness.state_mut().app.save_toolbar_skin(skin::Skin {
            css: Some(std::sync::Arc::from(theme::GLOSS_EXAMPLE)),
            ..Default::default()
        });
        harness.run();
        assert_eq!(play, harness.get_by_label("Play").rect());
        assert_eq!(tempo, harness.get_by_label("Tempo").rect());
        assert!(!crate::kit::native_css::toolbar_layout_requested());
        harness.get_by_label("Play").click();
        harness.run();
        assert!(harness.state().actions.iter().any(|action| action.id == "transport.play"));
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn editing_keeps_real_controls_and_done_without_changes_preserves_native_layout() {
        let mut harness = native_bar();
        let play = harness.get_by_label("Play").rect();
        let tempo = harness.get_by_label("Tempo").rect();
        let theme_id = harness.state().app.settings.theme.clone();
        start_edit(&mut harness);
        assert_eq!(play, harness.get_by_label("Play").rect());
        assert_eq!(tempo, harness.get_by_label("Tempo").rect());
        harness.get_by_label("Move toolbar transport").click();
        harness.run();
        assert!(harness.state().actions.is_empty(), "editing must not play audio");
        harness.get_by_label("Done").click();
        harness.run();
        assert_eq!(harness.state().app.settings.theme, theme_id);
        assert_eq!(play, harness.get_by_label("Play").rect());
        assert_eq!(tempo, harness.get_by_label("Tempo").rect());
        assert!(harness.state().app.themes.get(&theme_id).unwrap().skin.toolbar.is_none());
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn edit_overlay_preserves_css_grid_geometry_without_writing_a_layout() {
        let mut harness = native_bar();
        harness.state_mut().app.save_toolbar_skin(skin::Skin {
            css: Some(std::sync::Arc::from(".toolbar {display:grid;grid-template-columns:100px 120px 90px;gap:8px;} #tempo {grid-column:1;grid-row:1;} #transport {grid-column:2;grid-row:1;} #position {grid-column:3;grid-row:1;} #browser,#signature,#grid,#count-in,#metronome,#mapping,#performance,#views,#agents {display:none;}")),
            ..Default::default()
        });
        harness.run();
        let play = harness.get_by_label("Play").rect();
        let tempo = harness.get_by_label("Tempo").rect();
        let theme_id = harness.state().app.settings.theme.clone();
        start_edit(&mut harness);
        assert_eq!(play, harness.get_by_label("Play").rect());
        assert_eq!(tempo, harness.get_by_label("Tempo").rect());
        harness.get_by_label("Done").click();
        harness.run();
        assert_eq!(play, harness.get_by_label("Play").rect());
        assert_eq!(harness.state().app.settings.theme, theme_id);
        assert!(harness.state().app.themes.get(&theme_id).unwrap().skin.toolbar.is_none());
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn dragging_real_transport_snaps_and_saves_only_on_done_and_cancel_discards() {
        let mut harness = native_bar();
        let original = harness.get_by_label("Play").rect();
        start_edit(&mut harness);
        let from = harness.get_by_label("Move toolbar transport").rect().center();
        harness.hover_at(from);
        harness.step();
        harness.drag_at(from);
        harness.step();
        let to = from + Vec2::new(53.0, 0.0);
        harness.hover_at(to);
        harness.step();
        harness.drop_at(to);
        harness.run();
        let moved = harness.get_by_label("Play").rect();
        let bar_left = 8.0; // egui-kittest's default central-panel margin.
        assert!((moved.left() - original.left()).abs() > 40.0);
        assert!(((moved.left() - bar_left) / 8.0).fract().abs() < 0.001);
        assert!(harness.state().actions.is_empty());
        harness.get_by_label("Done").click();
        harness.run();
        assert_eq!(harness.get_by_label("Play").rect(), moved);
        let saved = harness.state().app.themes.get(&harness.state().app.settings.theme).unwrap().skin.clone();
        assert!(saved.toolbar.is_none(), "moving native modules must preserve native layout");
        assert!(saved.toolbar_offsets.contains_key("transport"));
        harness.set_size(Vec2::new(1600.0, 240.0));
        harness.run();
        assert!(
            (harness.get_by_label("Play").rect().left() - (moved.left() - 160.0)).abs() < 0.01,
            "transport keeps its centre anchor when the window narrows"
        );
        harness.set_size(Vec2::new(1920.0, 240.0));
        harness.run();
        start_edit(&mut harness);
        harness.get_by_label("Reset positions").click();
        harness.run();
        assert_eq!(harness.get_by_label("Play").rect(), original);
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(harness.get_by_label("Play").rect(), moved);
        harness.get_by_label("Play").click();
        harness.run();
        assert!(harness.state().actions.iter().any(|a| a.id == "transport.play"));
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn css_toolbar_targets_keep_native_commands_with_svg_artwork() {
        let mut app = App::new();
        let layout = Toolbar {
            hidden: skin::MODULES
                .iter()
                .filter(|id| !["tempo", "transport", "mapping"].contains(id))
                .map(|id| (*id).into())
                .collect(),
            ..Default::default()
        };
        app.save_toolbar_skin(skin::Skin { assets:skin::example_assets(),toolbar:Some(layout),css:Some(std::sync::Arc::from(".toolbar { display:grid; grid-template-columns:80px 120px; gap:8px; } #tempo { grid-column:1; grid-row:1; width:auto; } #transport { grid-column:2; width:auto; } #mapping { display:none; }")),..Default::default() });
        let fixture = Fixture { app, grid: Grid::build(&crate::demo::demo_project(), None), actions: Vec::new() };
        let mut harness = Harness::builder().with_size((640.0, 56.0)).build_ui_state(
            |ui, fixture: &mut Fixture| {
                let skin = fixture
                    .app
                    .themes
                    .get(&fixture.app.settings.theme)
                    .map(|theme| theme.skin.clone())
                    .unwrap_or_default();
                skin::configure(&skin);
                let _frame = crate::kit::native_css::frame(ui);
                fixture.app.control_bar(ui, Some(&fixture.grid), None, None, &mut fixture.actions);
            },
            fixture,
        );
        theme::install(&harness.ctx);
        harness.run();
        let tempo = harness.get_by_label("Tempo").rect();
        let play = harness.get_by_label("Play").rect();
        assert!(play.left() > tempo.right());
        assert!(harness.query_by_label("Map keys to slots and scenes").is_none());
        harness.get_by_label("Play").click();
        harness.run();
        assert!(harness.state().actions.contains(&Action::new("transport.play", json!({}))));
        harness.state_mut().actions.clear();
        harness.get_by_label("Stop").click();
        harness.run();
        assert!(harness.state().actions.iter().any(|action| action.id == "transport.stop"));
        skin::configure(&skin::Skin::default());
    }
}
