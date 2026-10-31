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

/// A module's caption at its left, `width` wide when it fits beside a usable control, and the rest.
fn captioned(rect: Rect, width: f32) -> (Rect, Rect) {
    let width = if rect.width() - width >= 48.0 { width } else { 0.0 };
    let caption = Rect::from_min_size(rect.min, Vec2::new(width, rect.height()));
    (caption, Rect::from_min_max(egui::pos2(caption.right(), rect.top()), rect.max))
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
        grid: Option<&TrackGrid>,
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
                            // Without a project the modules are empty: no boxes where nothing is.
                            if let Some(grid) = grid {
                                painter.rect_filled(
                                    at,
                                    module.style.radius,
                                    module.style.background.gamma_multiply(module.style.opacity),
                                );
                                crate::kit::native_css::background_with(&painter, at, &module.style);
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
            // A caption, as in the native toolbar, and the menu.
            "grid" => 26.0 + 70.0,
            "count-in" => 50.0 + 70.0,
            "transport" => 86.0,
            "position" => 78.0,
            "mapping" => 64.0,
            "performance" => 145.0,
            "views" => 172.0,
            "agents" => (self.agents.len() as f32 * 140.0).max(1.0),
            _ => 0.0,
        }
    }

    /// Uses a theme with `skin`, its sheet included, at once, as one read from its files is.
    #[cfg(test)]
    pub(super) fn use_test_skin(&mut self, skin: skin::Skin) {
        let theme = crate::kit::theme::Theme { name: "Test skin".into(), skin, ..Default::default() };
        self.themes.put(crate::settings::themes::ThemeEntry { id: "test-skin".into(), theme });
        self.settings.theme = "test-skin".into();
        self.settings_changed = true;
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
        grid: &TrackGrid,
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
            "browser" => self.browser_toggle(ui, &painter, rect),
            "tempo" => {
                ui.ctx().data_mut(|d| d.insert_temp(Id::new("sampler-tempo"), grid.tempo_bpm));
                self.tempo_field(ui, &painter, rect, grid.tempo_bpm, false, actions);
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
                let (caption, rect) = captioned(rect, 26.0);
                widgets::text_in(
                    &painter,
                    caption,
                    Align2::LEFT_CENTER,
                    "Quantize",
                    theme::ui_font(theme::TEXT_SMALL),
                    theme::text_hint(),
                );
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
            "count-in" => {
                let (caption, rect) = captioned(rect, 50.0);
                widgets::text_in(
                    &painter,
                    caption,
                    Align2::LEFT_CENTER,
                    "Count-in",
                    theme::ui_font(theme::TEXT_SMALL),
                    theme::text_hint(),
                );
                crate::session::record_controls::count_in_menu(
                    ui,
                    rect,
                    self.session().map_or(0, |s| s.count_in_bars()),
                    actions,
                );
            }
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
                for (index, view, name) in [(0, View::Session, "Launcher"), (1, View::Arrangement, "Arrangement")] {
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
#[path = "themed_toolbar_tests.rs"]
mod tests;
