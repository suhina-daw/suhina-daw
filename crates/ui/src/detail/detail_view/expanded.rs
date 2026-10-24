// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device's expanded panel, opened from its header, and a graph device's node list.

use super::*;

/// Where the open expanded panel's device is, kept in the window's memory while the app runs.
pub(super) fn expanded_id() -> Id {
    Id::new("device-expanded")
}

/// The device whose expanded panel is open, if one is.
pub fn expanded(ctx: &egui::Context) -> Option<(TrackId, DeviceAt)> {
    ctx.data(|d| d.get_temp::<(TrackId, DeviceAt)>(expanded_id()))
}

/// Opens the expanded panel of the device at `at` on `track`, closing any other.
pub(super) fn open_expanded(ctx: &egui::Context, track: TrackId, at: DeviceAt) {
    ctx.data_mut(|d| d.insert_temp(expanded_id(), (track, at)));
}

/// Closes the expanded panel.
pub fn close_expanded(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<(TrackId, DeviceAt)>(expanded_id()));
}

/// The open expanded panel's device in what the detail area shows, with its track. A panel
/// whose device is no longer shown (another track selected, the device removed) closes.
pub fn expanded_device(ctx: &egui::Context, detail: &Detail) -> Option<(TrackId, DeviceBox)> {
    /// The device at `at` among `devices` or inside their groups' branches.
    fn find<'a>(devices: &'a [DeviceBox], at: &DeviceAt) -> Option<&'a DeviceBox> {
        devices.iter().find_map(|device| {
            if &device.at == at {
                return Some(device);
            }
            device.group.as_ref()?.branches.iter().find_map(|branch| find(&branch.devices, at))
        })
    }
    let (track, at) = expanded(ctx)?;
    let chain = match detail {
        Detail::Devices(chain) | Detail::Clip(_, chain) => Some(chain),
        Detail::Nothing => None,
    };
    let found = chain
        .filter(|chain| chain.track == track)
        .and_then(|chain| find(&chain.devices, &at))
        .filter(|device| device.graph.as_ref().is_some_and(|g| g.expanded.is_some()));
    if found.is_none() {
        close_expanded(ctx);
    }
    found.map(|device| (track, device.clone()))
}

/// Draws `device`'s expanded panel over `area` (the main area), on top of the views: a title
/// row with its name and a close button, then its canvas, scrolled when the area is smaller.
/// Its controls change the same parameters as the compact panel, through `device.setParam`.
/// The close button and Escape close it.
pub fn show_expanded(ui: &mut Ui, area: Rect, track: TrackId, device: &DeviceBox, actions: &mut Vec<Action>) {
    let Some((graph, canvas)) = device.graph.as_ref().and_then(|g| Some((g, g.expanded.as_ref()?))) else {
        return;
    };
    egui::Area::new(Id::new("device-expanded-area")).order(egui::Order::Foreground).fixed_pos(area.min).show(
        ui.ctx(),
        |ui| {
            // The whole main area takes the pointer, so the views under it get no clicks.
            let (rect, _) = ui.allocate_exact_size(area.size(), Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::panel_background());
            let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW + PAD));
            painter.rect_filled(header, 0.0, theme::control_background());
            widgets::text_in(
                &painter,
                header.shrink2(Vec2::new(PAD * 2.0, 0.0)),
                Align2::LEFT_CENTER,
                &device.name,
                theme::ui_font(theme::TEXT),
                theme::text_primary(),
            );
            let close =
                Rect::from_min_size(Pos2::new(header.right() - 28.0, header.top() + 3.0), Vec2::new(24.0, 20.0));
            let response = widgets::button_area(ui, close, Id::new("device-expanded-close"), "Close expanded panel")
                .on_hover_text("Close the expanded panel (Esc)");
            let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            painter.rect_filled(close, 0.0, fill);
            for (from, to) in
                [(Vec2::new(-4.0, -4.0), Vec2::new(4.0, 4.0)), (Vec2::new(-4.0, 4.0), Vec2::new(4.0, -4.0))]
            {
                painter.line_segment(
                    [close.center() + from, close.center() + to],
                    egui::Stroke::new(1.3, theme::text_primary()),
                );
            }
            let escape = !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Escape));
            if response.clicked() || escape {
                close_expanded(ui.ctx());
            }
            let body =
                Rect::from_min_max(Pos2::new(rect.left() + PAD, header.bottom() + PAD), rect.max - Vec2::splat(PAD));
            let size = Vec2::new(canvas.size[0], canvas.size[1]);
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("device-expanded-scroll").show(ui, |ui| {
                    // Centred while it fits, from the top left when it scrolls.
                    let room = ui.available_size();
                    let offset = Vec2::new(((room.x - size.x) / 2.0).max(0.0), ((room.y - size.y) / 2.0).max(0.0));
                    let (space, _) = ui.allocate_exact_size(size + offset, Sense::hover());
                    let canvas_rect = Rect::from_min_size(space.min + offset, size);
                    crate::detail::device_canvas::show(
                        ui,
                        canvas_rect,
                        canvas,
                        &graph.parameters,
                        track,
                        &device.at,
                        actions,
                    );
                });
            });
        },
    );
}

/// A graph's parameter list, its node list and its canvas in `body`, scrolled when they do not
/// fit: a `.sd` device's, a built-in's (its layout or the generated list) and a plug-in's
/// generated list. A list without a canvas is a compact panel's width wherever it is, and a
/// narrower `body` scrolls it (ADR 0021 › Amendment: device panels, 5); stock controls under a
/// canvas take `body`'s width. Every change is one `device.setParam`.
pub(super) fn graph_controls(
    ui: &mut Ui,
    body: Rect,
    graph: &GraphBox,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::both().auto_shrink([false, false]).id_salt(("sd-controls", track.0, at)).show(ui, |ui| {
            let hidden = graph.canvas.as_ref().map_or(&[][..], |c| c.hidden_parameters.as_slice());
            let width = if graph.canvas.is_some() { body.width() } else { COMPACT_WIDTH };
            let cells = parameter_layout(&graph.parameters, width, hidden);
            let custom_size = graph.canvas.as_ref().map_or(Vec2::ZERO, |c| Vec2::new(c.size[0], c.size[1]));
            let nodes_height = graph.nodes.len() as f32 * NODE_ROW;
            let extent = cells.iter().fold(Vec2::new(body.width().max(custom_size.x), 0.0), |size, rect| {
                Vec2::new(size.x.max(rect.right()), size.y.max(rect.bottom()))
            });
            let (_, body) = ui.allocate_space(Vec2::new(extent.x, extent.y + nodes_height + custom_size.y));
            let painter = &ui.painter_at(body.intersect(ui.clip_rect()));
            node_rows(painter, body, graph);
            if let Some(canvas) = &graph.canvas {
                let rect = Rect::from_min_size(body.min + Vec2::new(0.0, nodes_height), custom_size);
                crate::detail::device_canvas::show(ui, rect, canvas, &graph.parameters, track, at, actions);
            }
            let origin = Vec2::new(body.left(), body.top() + nodes_height + custom_size.y);
            for (parameter, cell) in graph.parameters.iter().filter(|p| !hidden.contains(&p.id)).zip(&cells) {
                parameter_cell(ui, painter, cell.translate(origin), parameter, track, at, actions);
            }
        });
    });
}

/// A graph's visible nodes from the top of `body`, one row each, with an arrow between them
/// when they form a single chain.
pub(super) fn node_rows(painter: &egui::Painter, body: Rect, graph: &GraphBox) {
    let mut row_top = body.top();
    for (index, node) in graph.nodes.iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(body.left(), row_top), Vec2::new(body.width(), NODE_ROW));
        if row.bottom() > body.bottom() + 0.5 {
            break;
        }
        let node_rect = Rect::from_min_size(row.min, Vec2::new((row.width() - PAD) * 0.78, NODE_ROW - PAD));
        painter.rect_filled(node_rect, 1.0, theme::control_background());
        widgets::text_in(
            painter,
            node_rect.shrink(PAD / 2.0),
            Align2::CENTER_CENTER,
            node,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_secondary(),
        );
        if graph.linear && index + 1 < graph.nodes.len() {
            widgets::text_in(
                painter,
                Rect::from_min_size(Pos2::new(node_rect.right(), row.top()), Vec2::new(PAD, NODE_ROW)),
                Align2::CENTER_CENTER,
                "›",
                theme::ui_font(theme::TEXT),
                theme::text_hint(),
            );
        }
        row_top += NODE_ROW;
    }
}
