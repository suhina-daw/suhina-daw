// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Edit actual native elements, retaining their anchors and command widgets underneath.
use super::App;
use crate::kit::{
    control_layout::{self, Direction, Entry, Placements},
    theme,
};
use egui::{Id, Sense, Vec2};

#[derive(Clone)]
struct Draft {
    theme: String,
    original: Placements,
    controls: Placements,
    selected: String,
}
fn id() -> Id {
    Id::new("interface-edit-draft")
}

/// Grid placement with optional peer edge/centre alignment; Shift refines the grid; Alt+Shift bypasses snapping.
/// Whether an anchor holds the control on the horizontal and on the vertical axis.
fn anchored_axes(layout: &control_layout::Placement) -> (bool, bool) {
    use suhina_style::anchor::Edge;
    let held = |edges: [Edge; 2]| edges.iter().any(|edge| layout.anchors.get(*edge).is_some());
    (held([Edge::Left, Edge::Right]), held([Edge::Top, Edge::Bottom]))
}

/// `start` dragged by `moved`: each anchor's gap grows or shrinks by the move along its axis (a
/// left or top anchor's gap is the distance past its target, a right or bottom one's before it).
fn anchored_drag(start: control_layout::Placement, moved: egui::Vec2) -> control_layout::Placement {
    use suhina_style::anchor::Edge;
    let mut layout = start;
    for edge in Edge::ALL {
        if let Some(mut anchor) = start.anchors.get(edge) {
            let by = match edge {
                Edge::Left => moved.x,
                Edge::Right => -moved.x,
                Edge::Top => moved.y,
                Edge::Bottom => -moved.y,
            };
            anchor.gap = anchor
                .gap
                .saturating_add(by.round() as i32)
                .clamp(-suhina_style::anchor::MAX_GAP, suhina_style::anchor::MAX_GAP);
            layout.anchors.set(edge, Some(anchor));
        }
    }
    layout
}

fn drag_position(
    desired: egui::Pos2,
    entry: &Entry,
    entries: &[Entry],
    modifiers: egui::Modifiers,
) -> (egui::Pos2, [Option<f32>; 2]) {
    if modifiers.shift && modifiers.alt {
        return (desired, [None; 2]);
    }
    let step = if modifiers.shift { 1.0 } else { 8.0 };
    let mut position = egui::pos2((desired.x / step).round() * step, (desired.y / step).round() * step);
    let mut guides = [None; 2];
    let mut best = [4.0; 2];
    for peer in entries.iter().filter(|p| p.name != entry.name && p.bounds == entry.bounds) {
        for axis in 0..2 {
            let extent = entry.at.size()[axis];
            for source in [0.0, extent * 0.5, extent] {
                for target in [peer.at.min[axis], peer.at.center()[axis], peer.at.max[axis]] {
                    let delta = target - (desired[axis] + source);
                    if delta.abs() < best[axis] {
                        best[axis] = delta.abs();
                        position[axis] = desired[axis] + delta;
                        guides[axis] = Some(target);
                    }
                }
            }
        }
    }
    (position, guides)
}

/// The fields that make one edge of `selected` follow an edge of another element in the same
/// container, with a gap: a persistent anchor written to the theme, unlike alignment while dragging.
fn anchor_fields(ui: &mut egui::Ui, selected: &str, layout: &mut control_layout::Placement, entries: &[Entry]) {
    use suhina_style::anchor::{Anchor, Edge, Name};
    let containers: Vec<egui::Rect> = entries.iter().filter(|e| e.name == selected).map(|e| e.bounds).collect();
    let peers: std::collections::BTreeSet<String> = entries
        .iter()
        .filter(|e| e.name != selected && containers.iter().any(|b| b.intersects(e.bounds)))
        .map(|e| e.name.clone())
        .chain(layout.anchors.iter().map(|(_, a)| a.target.as_str().to_owned()))
        .collect();
    ui.label("Follow another element (kept in the theme):");
    for own in Edge::ALL {
        let mut anchor = layout.anchors.get(own);
        ui.horizontal(|ui| {
            ui.label(format!("{} edge", own.name()));
            let current = anchor.map_or_else(|| "none".to_owned(), |a| a.target.as_str().to_owned());
            egui::ComboBox::from_id_salt(("interface-anchor-target", own.name())).selected_text(&current).show_ui(
                ui,
                |ui| {
                    if ui.selectable_label(anchor.is_none(), "none").clicked() {
                        anchor = None;
                    }
                    for peer in &peers {
                        let chosen = anchor.is_some_and(|a| a.target.as_str() == peer);
                        if ui.selectable_label(chosen, peer).clicked()
                            && let Some(target) = Name::new(peer)
                        {
                            // A new target starts at the opposite edge: next to it, not over it.
                            let edge = match own {
                                Edge::Left => Edge::Right,
                                Edge::Right => Edge::Left,
                                Edge::Top => Edge::Bottom,
                                Edge::Bottom => Edge::Top,
                            };
                            anchor = Some(Anchor { target, edge, gap: anchor.map_or(0, |a| a.gap) });
                        }
                    }
                },
            );
            if let Some(a) = &mut anchor {
                let options = if own.axis() == 0 { [Edge::Left, Edge::Right] } else { [Edge::Top, Edge::Bottom] };
                for edge in options {
                    ui.selectable_value(&mut a.edge, edge, edge.name());
                }
                ui.label("gap");
                ui.add(egui::DragValue::new(&mut a.gap).range(-16384..=16384).suffix(" px"));
            }
        });
        layout.anchors.set(own, anchor);
    }
}

impl App {
    pub(super) fn control_editor_theme_changed(&self, ctx: &egui::Context) {
        if ctx.data(|data| data.get_temp::<Draft>(id())).is_some_and(|draft| draft.theme != self.settings.theme) {
            control_layout::draft(ctx, None);
            ctx.data_mut(|data| data.remove::<Draft>(id()));
        }
    }
    pub(super) fn start_control_editor(&self, ctx: &egui::Context) {
        let original =
            self.themes.get(&self.settings.theme).map(|theme| theme.skin.controls.clone()).unwrap_or_default();
        control_layout::draft(ctx, Some(original.clone()));
        ctx.data_mut(|data| {
            data.insert_temp(
                id(),
                Draft {
                    theme: self.settings.theme.clone(),
                    controls: original.clone(),
                    original,
                    selected: String::new(),
                },
            )
        });
        ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        ctx.request_repaint();
    }

    pub(super) fn control_editor(&mut self, ui: &egui::Ui, entries: &[Entry]) {
        let Some(mut draft) = ui.ctx().data(|data| data.get_temp::<Draft>(id())) else {
            return;
        };
        if draft.theme != self.settings.theme {
            control_layout::draft(ui.ctx(), None);
            ui.ctx().data_mut(|data| data.remove::<Draft>(id()));
            return;
        }
        let mut done = false;
        let mut cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let area = ui.max_rect();
        ui.ctx().set_sublayer(
            egui::LayerId::new(egui::Order::Foreground, Id::new("interface-edit-overlay")),
            egui::LayerId::new(egui::Order::Foreground, Id::new("interface-edit-tools")),
        );
        egui::Area::new(Id::new("interface-edit-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .movable(false)
            .show(ui.ctx(), |overlay| {
                overlay.set_min_size(area.size());
                overlay.set_clip_rect(area);
                // Consume input across the workspace, including the gaps between controls.
                overlay.interact(area, Id::new("interface-edit-shield"), Sense::click_and_drag());
                let ink = theme::accent();
                for x in (0..=(area.width().min(16384.0) / 8.0) as usize).map(|n| area.left() + n as f32 * 8.0) {
                    overlay.painter().vline(x, area.y_range(), egui::Stroke::new(0.5, ink.gamma_multiply(0.12)));
                }
                for y in (0..=(area.height().min(16384.0) / 8.0) as usize).map(|n| area.top() + n as f32 * 8.0) {
                    overlay.painter().hline(area.x_range(), y, egui::Stroke::new(0.5, ink.gamma_multiply(0.12)));
                }
                for (index, entry) in entries.iter().enumerate() {
                    let response = overlay.interact(
                        entry.at.intersect(area),
                        Id::new(("move-interface", &entry.name, index)),
                        Sense::click_and_drag(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Move {} instance {}", entry.name, index + 1),
                        )
                    });
                    let selected = draft.selected == entry.name;
                    overlay.painter().rect_stroke(
                        entry.at,
                        1.0,
                        egui::Stroke::new(if selected { 2.0 } else { 1.0 }, ink),
                        egui::StrokeKind::Inside,
                    );
                    if response.clicked() || response.drag_started() {
                        draft.selected = entry.name.clone();
                    }
                    if response.drag_started() {
                        let start = draft.controls.get(&entry.name).copied().unwrap_or_default();
                        overlay.ctx().data_mut(|data| {
                            data.insert_temp(response.id.with("origin"), entry.at.min);
                            data.insert_temp(response.id.with("start"), start);
                        });
                    }
                    if response.dragged()
                        && (draft.controls.contains_key(&entry.name)
                            || draft.controls.len() < control_layout::MAX_CONTROLS)
                    {
                        let origin = overlay
                            .ctx()
                            .data(|data| data.get_temp::<egui::Pos2>(response.id.with("origin")))
                            .unwrap_or(entry.at.min);
                        let desired = origin + response.total_drag_delta().unwrap_or_default();
                        let (snapped, guides) = drag_position(desired, entry, entries, overlay.input(|i| i.modifiers));
                        if let Some(x) = guides[0] {
                            overlay.painter().vline(x, entry.bounds.y_range(), egui::Stroke::new(1.0, ink));
                        }
                        if let Some(y) = guides[1] {
                            overlay.painter().hline(entry.bounds.x_range(), y, egui::Stroke::new(1.0, ink));
                        }
                        let start = overlay
                            .ctx()
                            .data(|data| data.get_temp::<control_layout::Placement>(response.id.with("start")))
                            .unwrap_or_default();
                        let moved = snapped - origin;
                        let layout = draft.controls.entry(entry.name.clone()).or_default();
                        let mut offset = snapped - entry.native.min;
                        // On an axis an anchor holds, the drag changes the anchor's gap, not an
                        // offset the anchor would override.
                        let (horizontal, vertical) = anchored_axes(&start);
                        if horizontal || vertical {
                            *layout = anchored_drag(start, moved);
                            if horizontal {
                                offset.x = start.offset().x;
                            }
                            if vertical {
                                offset.y = start.offset().y;
                            }
                        } else {
                            layout.bottom = None;
                            layout.full_height = false;
                            layout.full_width = false;
                            layout.right = None;
                        }
                        layout.set_offset(offset);
                        overlay.ctx().request_repaint();
                    }
                    response.on_hover_text(format!("{} · drag to move · Shift: 1 px · Alt+Shift: free", entry.name));
                }
            });
        egui::Window::new("Edit interface")
            .id(Id::new("interface-edit-tools"))
            .order(egui::Order::Foreground)
            .default_pos(area.min + Vec2::new(16.0, 110.0))
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |tools| {
                tools.label("Drag: grid/alignment · Shift: 1 px · Alt+Shift: free");
                tools.label("Matching elements share the theme layout.");
                let names: std::collections::BTreeSet<_> =
                    entries.iter().map(|entry| entry.name.clone()).chain(draft.controls.keys().cloned()).collect();
                egui::ComboBox::from_id_salt("interface-element")
                    .selected_text(if draft.selected.is_empty() { "Select an element" } else { &draft.selected })
                    .show_ui(tools, |tools| {
                        for name in names {
                            tools.selectable_value(&mut draft.selected, name.clone(), &name);
                        }
                    });
                let can_edit =
                    draft.controls.contains_key(&draft.selected) || draft.controls.len() < control_layout::MAX_CONTROLS;
                if !can_edit {
                    tools.label("The theme already overrides 256 elements. Reset an override to add another.");
                }
                if !draft.selected.is_empty() && can_edit {
                    let mut layout = draft.controls.get(&draft.selected).copied().unwrap_or_default();
                    let before = layout;
                    let mut offset = layout.offset();
                    tools.horizontal(|tools| {
                        tools.label("Offset X");
                        tools.add(egui::DragValue::new(&mut offset.x).range(-16384..=16384));
                        tools.label("Y");
                        tools.add(egui::DragValue::new(&mut offset.y).range(-16384..=16384));
                    });
                    layout.set_offset(offset);
                    if tools.checkbox(&mut layout.full_width, "Full width").changed() {
                        layout.width = None;
                    }
                    if layout.full_width {
                        tools.label("X is the left inset; Anchor to right sets the right inset.");
                    }
                    if tools.checkbox(&mut layout.full_height, "Full height").changed() {
                        layout.height = None;
                    }
                    if layout.full_height {
                        tools.label("Y is the top inset; Anchor to bottom sets the bottom inset.");
                    }
                    tools.horizontal(|tools| {
                        for (name, dimension) in [("Width", &mut layout.width), ("Height", &mut layout.height)] {
                            if (name == "Height" && layout.full_height) || (name == "Width" && layout.full_width) {
                                continue;
                            }
                            let mut custom = dimension.is_some();
                            if tools.checkbox(&mut custom, name).changed() {
                                *dimension = custom.then_some(20);
                            }
                            if let Some(value) = dimension {
                                tools.add(egui::DragValue::new(value).range(1..=4096));
                            }
                        }
                    });
                    tools.horizontal(|tools| {
                        let mut right = layout.right.is_some();
                        if tools.checkbox(&mut right, "Anchor to right").changed() {
                            layout.right = right.then_some(0);
                        }
                        if let Some(value) = &mut layout.right {
                            tools.add(egui::DragValue::new(value).range(-16384..=16384));
                        }
                    });
                    tools.horizontal(|tools| {
                        let mut bottom = layout.bottom.is_some();
                        if tools.checkbox(&mut bottom, "Anchor to bottom").changed() {
                            layout.bottom = bottom.then_some(0);
                        }
                        if let Some(value) = &mut layout.bottom {
                            tools.add(egui::DragValue::new(value).range(-16384..=16384));
                        }
                        tools.add_enabled(
                            !draft.selected.starts_with("toolbar-"),
                            egui::Checkbox::new(&mut layout.hidden, "Hidden"),
                        );
                    });
                    anchor_fields(tools, &draft.selected, &mut layout, entries);
                    if matches!(draft.selected.as_str(), "mixer-volume" | "arrangement-volume") {
                        tools.horizontal(|tools| {
                            tools.selectable_value(&mut layout.direction, Direction::Vertical, "Vertical");
                            tools.selectable_value(&mut layout.direction, Direction::Horizontal, "Horizontal");
                        });
                    }
                    if layout != before {
                        let mut next = draft.controls.clone();
                        next.insert(draft.selected.clone(), layout);
                        let mut problems = Vec::new();
                        control_layout::validate_anchors(&mut next, &mut problems);
                        if problems.is_empty() {
                            draft.controls = next;
                            tools
                                .ctx()
                                .data_mut(|data| data.remove::<Vec<String>>(Id::new("interface-anchor-problem")));
                        } else {
                            // A cycle or an overlong chain is never drafted: the change is refused.
                            tools
                                .ctx()
                                .data_mut(|data| data.insert_temp(Id::new("interface-anchor-problem"), problems));
                        }
                    }
                    if let Some(problems) =
                        tools.ctx().data(|data| data.get_temp::<Vec<String>>(Id::new("interface-anchor-problem")))
                    {
                        for problem in problems {
                            tools.colored_label(theme::recording(), problem);
                        }
                    }
                    if tools.button("Reset selected element").clicked() {
                        draft.controls.remove(&draft.selected);
                    }
                }
                tools.horizontal(|tools| {
                    done = tools.button("Done").clicked();
                    cancel |= tools.button("Cancel").clicked();
                });
            });
        if done || cancel {
            control_layout::draft(ui.ctx(), None);
            ui.ctx().data_mut(|data| data.remove::<Draft>(id()));
            if done && draft.controls != draft.original {
                let mut skin =
                    self.themes.get(&self.settings.theme).map(|theme| theme.skin.clone()).unwrap_or_default();
                // Merge only edited entries into the latest live theme.
                let keys: std::collections::BTreeSet<_> = draft.original.keys().chain(draft.controls.keys()).collect();
                for key in keys {
                    if draft.controls.get(key) == draft.original.get(key) {
                        continue;
                    }
                    if let Some(value) = draft.controls.get(key) {
                        skin.controls.insert(key.clone(), *value);
                    } else {
                        skin.controls.remove(key);
                    }
                }
                self.save_toolbar_skin(skin);
            }
        } else {
            control_layout::draft(ui.ctx(), Some(draft.controls.clone()));
            ui.ctx().data_mut(|data| data.insert_temp(id(), draft));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn window() -> Harness<'static, App> {
        let mut app = App::new();
        app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
        let mut harness =
            Harness::builder().with_size((1280.0, 900.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        harness.run_steps(3);
        harness
    }

    fn start(harness: &mut Harness<'static, App>) {
        harness.state().start_control_editor(&harness.ctx);
        harness.run_steps(4);
    }

    #[test]
    fn a_drag_along_an_anchored_axis_changes_the_gap() {
        use suhina_style::anchor::{Anchor, Edge};
        let mut start = control_layout::Placement::default();
        start.anchors.set(Edge::Bottom, Anchor::parse(Edge::Bottom, "mixer-toggles top 6px").ok());
        start.anchors.set(Edge::Left, Anchor::parse(Edge::Left, "mixer-name right 2px").ok());
        assert_eq!(anchored_axes(&start), (true, true));
        let moved = anchored_drag(start, egui::vec2(10.0, -24.0));
        // Up by 24 leaves 24 more below the bottom edge; right by 10 adds 10 past the target.
        assert_eq!(moved.anchors.get(Edge::Bottom).map(|a| a.gap), Some(30));
        assert_eq!(moved.anchors.get(Edge::Left).map(|a| a.gap), Some(12));
        assert_eq!(anchored_axes(&control_layout::Placement::default()), (false, false));
    }

    #[test]
    fn drag_modifiers_and_peer_alignment_are_distinct() {
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(200.0));
        let rect = egui::Rect::from_min_size(egui::pos2(16.0, 16.0), Vec2::new(20.0, 10.0));
        let entry = Entry { name: "moving".into(), native: rect, at: rect, bounds };
        let peer_rect = egui::Rect::from_min_size(egui::pos2(100.0, 31.0), rect.size());
        let peer = Entry { name: "peer".into(), native: peer_rect, at: peer_rect, bounds };
        let desired = egui::pos2(18.25, 30.25);
        let (aligned, guides) = drag_position(desired, &entry, std::slice::from_ref(&peer), egui::Modifiers::NONE);
        assert_eq!(aligned.y, 31.0);
        assert_eq!(guides[1], Some(31.0));
        let (aligned_shift, guides) =
            drag_position(desired, &entry, std::slice::from_ref(&peer), egui::Modifiers::SHIFT);
        assert_eq!(aligned_shift.y, 31.0);
        assert_eq!(guides[1], Some(31.0));
        let desired_x = egui::pos2(98.75, 60.25);
        let (aligned_x, guides) = drag_position(desired_x, &entry, std::slice::from_ref(&peer), egui::Modifiers::SHIFT);
        assert_eq!(aligned_x.x, 100.0);
        assert_eq!(aligned_x.y, 60.0);
        assert_eq!(guides[0], Some(100.0));
        assert_eq!(
            drag_position(desired, &entry, &[peer], egui::Modifiers { alt: true, shift: true, ..Default::default() }),
            (desired, [None; 2])
        );
        let (fine, guides) = drag_position(desired, &entry, &[], egui::Modifiers::SHIFT);
        assert_eq!(fine, egui::pos2(18.0, 30.0));
        assert_eq!(guides, [None; 2]);
        let free = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        assert_eq!(drag_position(desired, &entry, &[], free).0, desired);
        assert_eq!(drag_position(desired, &entry, &[], egui::Modifiers::ALT).0, egui::pos2(16.0, 32.0));
    }

    #[test]
    fn noop_and_cancel_keep_the_theme_and_native_layout() {
        let mut harness = window();
        let original = harness.state().settings.theme.clone();
        let original_rect = harness.get_by_label_contains("Drums level:").rect();
        start(&mut harness);
        harness.get_by_label("Done").click();
        harness.run_steps(3);
        assert_eq!(harness.state().settings.theme, original);
        assert_eq!(harness.get_by_label_contains("Drums level:").rect(), original_rect);
        start(&mut harness);
        let at = harness.query_all_by_label_contains("Move mixer-meter instance").next().unwrap().rect().center();
        harness.hover_at(at);
        harness.drag_at(at);
        harness.run_steps(1);
        harness.hover_at(at + Vec2::new(24.0, 0.0));
        harness.run_steps(2);
        harness.drop_at(at + Vec2::new(24.0, 0.0));
        harness.run_steps(2);
        assert_ne!(harness.get_by_label_contains("Drums level:").rect(), original_rect);
        assert!(harness.state().session().is_some_and(|session| !session.can_undo()));
        harness.get_by_label("Cancel").click();
        harness.run_steps(3);
        assert_eq!(harness.state().settings.theme, original);
        assert_eq!(harness.get_by_label_contains("Drums level:").rect(), original_rect);
    }

    #[test]
    fn project_shortcuts_do_not_delete_tracks_while_placing_elements() {
        let mut harness = window();
        harness.state_mut().view.selected = Some(suhina_engine::model::TrackId(1));
        harness.state_mut().view.track_focus = true;
        let tracks = harness.state().session().unwrap().project().tracks.len();
        start(&mut harness);
        harness.key_press(egui::Key::Delete);
        harness.run_steps(3);
        assert_eq!(harness.state().session().unwrap().project().tracks.len(), tracks);
        assert!(!harness.state().session().unwrap().can_undo());
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        assert!(harness.query_by_label("Done").is_none());
    }

    #[test]
    fn done_saves_the_snapped_rectangle_and_theme_switch_discards_the_draft() {
        let mut harness = window();
        start(&mut harness);
        let at = harness.query_all_by_label_contains("Move mixer-meter instance").next().unwrap().rect().center();
        harness.hover_at(at);
        harness.drag_at(at);
        harness.run_steps(1);
        harness.hover_at(at + Vec2::new(24.0, 0.0));
        harness.run_steps(2);
        harness.drop_at(at + Vec2::new(24.0, 0.0));
        harness.run_steps(2);
        let preview = harness.get_by_label_contains("Drums level:").rect();
        assert!((preview.left() / 8.0).fract().abs() < 0.001);
        harness.get_by_label("Done").click();
        harness.run_steps(3);
        assert_eq!(harness.get_by_label_contains("Drums level:").rect(), preview);
        assert!(
            harness
                .state()
                .themes
                .get(&harness.state().settings.theme)
                .unwrap()
                .skin
                .controls
                .contains_key("mixer-meter")
        );
        start(&mut harness);
        harness.state_mut().settings.theme = theme::BUILT_IN.into();
        harness.run_steps(3);
        assert!(harness.query_by_label("Done").is_none());
        assert_eq!(harness.state().settings.theme, theme::BUILT_IN);
    }
}
