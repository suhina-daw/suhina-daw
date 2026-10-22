// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared Session and Arrangement channel control faces; interaction belongs to each view.
use super::{motion, theme, widgets};
use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Color32, Pos2, Rect, Vec2};

pub fn pan_field(painter: &egui::Painter, rect: Rect, pan: f64, hover: f32) {
    painter.fill_background(rect, 0.0, motion::mix(theme::pan_background(), theme::panel_hover(), hover));
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::pan_centre()));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::pan_centre()));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::control_selected());
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

pub fn send_field(
    painter: &egui::Painter,
    rect: Rect,
    letter: &str,
    db: f64,
    pre_fader: bool,
    pointer: Color32,
    position: f32,
) {
    // A labelled send tile with a small level rail, rather than a miniature rotary dial.
    let rail = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 1.5),
    );
    let quiet = motion::mix(theme::control_background(), pointer, 0.25);
    painter.fill_background(rail, 1.0, quiet);
    let level = Rect::from_min_size(rail.min, Vec2::new(rail.width() * position.clamp(0.0, 1.0), rail.height()));
    painter.rect_filled(level, 1.0, motion::mix(quiet, pointer, 0.4));
    // The destination stays legible; a dot denotes pre-fader mode.
    if rect.width() < 40.0 {
        // Session's two-column send grid preserves the narrow channel width.
        let title = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 12.0));
        let value =
            Rect::from_min_max(Pos2::new(rect.left(), title.bottom()), Pos2::new(rect.right(), rect.bottom() - 4.0));
        widgets::text_in(painter, title, Align2::CENTER_CENTER, letter, theme::mono_font(9.0), pointer);
        widgets::text_in(
            painter,
            value,
            Align2::CENTER_CENTER,
            &widgets::format_db(db),
            theme::mono_font(7.0),
            pointer,
        );
    } else {
        let text = rect.translate(Vec2::new(0.0, -1.0));
        widgets::text_in(
            painter,
            text,
            Align2::LEFT_CENTER,
            letter,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        widgets::text_in(
            painter,
            text,
            Align2::RIGHT_CENTER,
            &widgets::format_db(db),
            theme::mono_font(theme::TEXT_SMALL),
            pointer,
        );
    }
    if pre_fader {
        painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, pointer);
    }
}

/// A tactile vertical fader cap. Its centre marker, rather than its edges, indicates the gain.
pub fn fader_cap(painter: &egui::Painter, cap: Rect, hover: f32, state: super::skin::State) {
    let _style = super::style_class::enter("fader-cap");
    super::native_css::background_rounded(painter, cap, 2.0);
    if super::skin::paint(painter, cap, "fader.cap", state, theme::text_value(), 0.0) {
        return;
    }
    let base = motion::mix(theme::control_background(), theme::control_hover(), hover);
    let face = motion::mix(base, theme::text_value(), 0.28);
    let light = motion::mix(face, theme::text_on(face), 0.32);
    let dark = motion::mix(base, Color32::BLACK, 0.55);
    painter.rect_filled(cap.translate(Vec2::new(1.0, 2.0)), 3.0, Color32::from_black_alpha(100));
    painter.rect_filled(cap, 2.0, face);
    let inner = cap.shrink(1.0);
    let shoulder = 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            inner.left_top(),
            inner.right_top(),
            Pos2::new(inner.right() - 2.0, inner.top() + shoulder),
            Pos2::new(inner.left() + 2.0, inner.top() + shoulder),
        ],
        light,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(inner.left() + 2.0, inner.bottom() - shoulder),
            Pos2::new(inner.right() - 2.0, inner.bottom() - shoulder),
            inner.right_bottom(),
            inner.left_bottom(),
        ],
        dark,
        egui::Stroke::NONE,
    ));
    painter.rect_stroke(cap, 2.0, theme::hairline(dark), egui::StrokeKind::Inside);
    let marker = Rect::from_center_size(cap.center(), Vec2::new(cap.width() - 4.0, 5.0));
    painter.rect_filled(marker, 1.0, dark);
    painter.hline(
        marker.shrink(1.0).x_range(),
        cap.center().y.round() + 0.5,
        egui::Stroke::new(2.0, theme::text_on(dark)),
    );
}

/// Read-only track type, based on supported content rather than installed instruments.
#[allow(clippy::too_many_arguments)]
pub fn track_type(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    name: &str,
    content: (bool, bool),
    color: Color32,
) {
    let (notes, audio) = content;
    if super::control_layout::placement("track-type").hidden {
        return;
    }
    let rect = super::control_layout::place("track-type", rect, painter.clip_rect());
    let label = match content {
        (true, true) => "MIDI and audio track",
        (true, false) => "MIDI track",
        (false, true) => "Audio track",
        (false, false) => return,
    };
    let response = ui.interact(rect.intersect(painter.clip_rect()), id, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{name}: {label}")));
    let state = super::skin::State::response(&response, false);
    response.on_hover_text(label);
    let center = rect.center();
    let stroke = egui::Stroke::new(1.2, color);
    if notes {
        let x = center.x - if audio { 4.0 } else { 0.0 };
        let icon = Rect::from_center_size(Pos2::new(x, center.y), Vec2::splat(if audio { 10.0 } else { 14.0 }));
        if !super::skin::paint(painter, icon, "track.midi", state, color, 0.0) {
            // A compact note remains readable if a custom renderer has no SVG texture.
            painter.line_segment([Pos2::new(x + 2.0, center.y - 5.0), Pos2::new(x + 2.0, center.y + 3.0)], stroke);
            painter.circle_filled(Pos2::new(x, center.y + 3.0), 2.0, color);
        }
    }
    if audio {
        let x = center.x + if notes { 5.0 } else { 0.0 };
        let scale = if notes { 0.6 } else { 1.0 };
        let icon = Rect::from_center_size(Pos2::new(x, center.y), Vec2::splat(if notes { 10.0 } else { 14.0 }));
        if super::skin::paint(painter, icon, "track.audio", state, color, 0.0) {
            return;
        }
        for (dx, height) in [(-4.0, 2.0), (-2.0, 4.0), (0.0, 5.0), (2.0, 3.0), (4.0, 1.5)] {
            painter.line_segment(
                [Pos2::new(x + dx * scale, center.y - height), Pos2::new(x + dx * scale, center.y + height)],
                stroke,
            );
        }
    }
}
