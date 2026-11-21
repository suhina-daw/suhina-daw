// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The SDK's `Widget::Strands` in a device panel (docs/spec/sdk-custom-ui.md › Reusable
//! functional widgets): its inputs read from the panel's expressions, its flow's clock kept per
//! widget, and the picture drawn by the window's GPU host (`kit::strands::Provider`) or, without
//! one, as a lighter version of the same dots in egui meshes.

use std::sync::OnceLock;
use std::time::Duration;

use egui::{Color32, Id, Mesh, Rect, Vec2, pos2};

use crate::kit::strands::{self, CPU_PLAN, Frame};

/// One frame's inputs, as the panel's expressions give them.
pub(crate) struct Inputs {
    pub modes: [f32; 6],
    pub since: f32,
    pub velocity: f32,
    pub height: f32,
    pub thickness: f32,
    pub speed: f32,
    pub presence: f32,
    /// Light, mid, shadow and thread.
    pub colors: [[u8; 4]; 4],
}

/// The flow's clock of one widget: where it stands, and the host time it was last moved on.
#[derive(Clone, Copy)]
struct Clock {
    value: f32,
    at: f64,
}

/// The longest step the clock takes between two frames, so a panel shown again does not jump.
const LONGEST_STEP: f64 = 0.1;
/// The lighter version draws a few thousand dots where the GPU draws hundreds of thousands: each
/// is larger and stronger by these factors so the picture keeps its body.
const CPU_SIZE: f32 = 1.8;
const CPU_COVER: f32 = 2.4;

fn cpu_points() -> &'static [[f32; 4]] {
    static POINTS: OnceLock<Vec<[f32; 4]>> = OnceLock::new();
    POINTS.get_or_init(|| strands::points(&CPU_PLAN))
}

/// Draws the widget `id` over `bounds` this frame.
pub(crate) fn show(ui: &egui::Ui, painter: &egui::Painter, bounds: Rect, id: Id, inputs: &Inputs) {
    if !bounds.is_finite() || bounds.width() < 1.0 || bounds.height() < 1.0 {
        return;
    }
    let now = ui.input(|i| i.time);
    let rate = strands::clock_rate(&inputs.modes, inputs.speed);
    let value = match ui.data(|d| d.get_temp::<Clock>(id)) {
        Some(clock) => {
            let step = (now - clock.at).clamp(0.0, LONGEST_STEP) as f32;
            let next = clock.value + step * rate;
            if next.is_finite() { next % 10_000.0 } else { 0.0 }
        }
        None => 0.0,
    };
    ui.data_mut(|d| d.insert_temp(id, Clock { value, at: now }));
    let frame = Frame {
        modes: inputs.modes,
        since: inputs.since,
        velocity: inputs.velocity,
        height: inputs.height,
        thickness: inputs.thickness,
        speed: inputs.speed,
        presence: inputs.presence,
        clock: value,
        colors: inputs.colors.map(|c| c.map(|v| f32::from(v) / 255.0)),
    };
    // While it sounds or a strike's sparks fly, it moves; at rest it is drawn once.
    if inputs.modes.iter().any(|m| strands::unit(*m) > 0.001) || (inputs.since.is_finite() && inputs.since < 5.0) {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    let scale = ui.ctx().pixels_per_point();
    if let Some(provider) = strands::provider(ui.ctx()) {
        let size = [
            (bounds.width() * scale).round().clamp(1.0, 2048.0) as u32,
            (bounds.height() * scale).round().clamp(1.0, 2048.0) as u32,
        ];
        if let Some(texture) = provider.draw(ui.ctx(), id, size, &frame) {
            painter.image(texture, bounds, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    let shared = frame.shared(bounds.width(), bounds.height());
    let mut behind = Mesh::default();
    let mut ahead = Mesh::default();
    for point in cpu_points() {
        let Some(dot) = strands::place(*point, &shared, &frame.colors) else { continue };
        let cover = (dot.color[3] * CPU_COVER).min(1.0);
        let gain = if dot.color[3] > 0.0 { cover / dot.color[3] } else { 0.0 };
        let channel = |v: f32| ((v * gain).clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = Color32::from_rgba_premultiplied(
            channel(dot.color[0]),
            channel(dot.color[1]),
            channel(dot.color[2]),
            channel(dot.color[3]),
        );
        let centre = bounds.min + Vec2::new(dot.x, dot.y);
        let mesh = if dot.front { &mut ahead } else { &mut behind };
        mesh.add_colored_rect(Rect::from_center_size(centre, Vec2::splat(dot.size * CPU_SIZE)), color);
    }
    painter.add(behind);
    painter.add(ahead);
}
