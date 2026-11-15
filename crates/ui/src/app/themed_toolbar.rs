// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The toolbar's right-click menu and its editor, which moves modules by an offset from where
//! the toolbar's row puts them.
use super::*;
use crate::kit::skin;
use crate::settings::settings_view::ThemeRequest;

fn edit_id() -> Id {
    Id::unique("edit-theme-toolbar")
}

#[derive(Clone, Default)]
struct EditDraft {
    theme_id: String,
    original: skin::Skin,
    skin: skin::Skin,
}

fn rects_id() -> Id {
    Id::unique("toolbar-edit-rects")
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
    /// The toolbar's right-click menu over `rect`, its whole box: Edit toolbar (the modules'
    /// offsets) and Restore default toolbar. Returns the theme's skin the toolbar is drawn with.
    pub(super) fn toolbar_menu(&mut self, ui: &mut Ui, rect: Rect) -> std::sync::Arc<skin::Skin> {
        let response = ui.interact(rect, Id::unique("toolbar-context"), Sense::click());
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
        theme_skin
    }

    pub(super) fn toolbar_edit_overlay(&mut self, ui: &Ui, bar: Rect) {
        let Some(mut draft) = ui.ctx().data(|d| d.get_temp::<EditDraft>(edit_id())) else {
            return;
        };
        let rects = ui.ctx().data(|d| d.get_temp::<Vec<(String, Rect, Rect)>>(rects_id())).unwrap_or_default();
        let mut done = false;
        let mut cancel = ui.input(|i| i.key_pressed(Key::Escape));
        crate::kit::floating::area(ui.ctx(), Id::unique("toolbar-edit-overlay"), crate::kit::floating::Kind::Own)
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
                    let response = overlay.interact(*at, Id::unique(("move-toolbar", name)), Sense::click_and_drag());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Move toolbar {name}"))
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
            .id(Id::unique("toolbar-edit-tools"))
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
}

#[cfg(test)]
#[path = "themed_toolbar_tests.rs"]
mod tests;
